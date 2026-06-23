//! Shared code-review dispatch for Kanban tasks.
//!
//! Both the visible GTK board and the headless MCP path use this module to claim review
//! metadata while holding the board lock. That single claim point prevents duplicate
//! auto-reviews when a task enters `In Review` from more than one surface.

#[cfg(test)]
use std::cell::RefCell;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

use crate::model::agent_run::AgentKind;
use crate::model::board::{Task, TaskStatus, now_epoch_secs};
use crate::services::{agent_config, board as board_service};
use crate::storage::board_store;

#[cfg(test)]
static DISABLE_HEADLESS_REVIEW_SPAWN: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
thread_local! {
    static FORCED_HEADLESS_REVIEW_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_test_disable_headless_review_spawn(disabled: bool) {
    DISABLE_HEADLESS_REVIEW_SPAWN.store(disabled, Ordering::SeqCst);
}

#[cfg(test)]
pub(crate) fn set_test_headless_review_spawn_error(error: Option<&str>) {
    FORCED_HEADLESS_REVIEW_ERROR.with(|forced| {
        *forced.borrow_mut() = error.map(str::to_string);
    });
}

/// A task/reviewer pair that has already been recorded in board review metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewSelection {
    pub task: Task,
    pub reviewer: AgentKind,
    pub yolo: bool,
}

/// Details of a spawned headless review process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadlessReviewRun {
    pub pid: u32,
    pub log_path: PathBuf,
}

/// Result of a review-capable board transition performed while holding the board lock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewTransition {
    pub task: Task,
    pub warnings: Vec<String>,
    pub selection: Option<ReviewSelection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewDispatchError {
    Board(board_service::BoardError),
    Storage(String),
}

impl std::fmt::Display for ReviewDispatchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReviewDispatchError::Board(error) => write!(formatter, "{error}"),
            ReviewDispatchError::Storage(error) => write!(formatter, "{error}"),
        }
    }
}

/// Concrete process plan used by [`spawn_headless_review`]. Kept separate so tests can
/// assert cwd/log placement without launching a real agent CLI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadlessReviewProcessSpec {
    pub program: String,
    pub args: Vec<String>,
    pub current_dir: PathBuf,
    pub log_path: PathBuf,
}

/// Move a task to `status` and, when that status is `InReview`, claim one pending
/// automatic review under the same board lock.
#[allow(dead_code)]
pub fn set_status_and_claim_auto_review(
    project_root: &Path,
    task_id: &str,
    status: TaskStatus,
) -> Result<Option<ReviewSelection>, String> {
    set_status_as_and_claim_auto_review(project_root, task_id, status, "user", true)
        .map(|transition| transition.selection)
        .map_err(|error| error.to_string())
}

/// Move a task to a status with ownership guards and claim review metadata atomically
/// when the destination is `InReview`.
pub fn set_status_as_and_claim_auto_review(
    project_root: &Path,
    task_id: &str,
    status: TaskStatus,
    actor: impl Into<String>,
    force: bool,
) -> Result<ReviewTransition, ReviewDispatchError> {
    let actor = actor.into();
    transition_and_claim_auto_review(project_root, task_id, |board| {
        let transition = board_service::set_status_as(board, task_id, status, actor, force)?;
        Ok((transition.task.clone(), transition.warnings))
    })
}

/// Mark work ready for review and claim one pending automatic review under the same board
/// lock. This is the MCP lifecycle helper path equivalent of moving to In Review, with
/// handoff note and lifecycle cleanup handled by the board service.
#[allow(dead_code)]
pub fn ready_for_review_and_claim_auto_review(
    project_root: &Path,
    task_id: &str,
    summary: String,
    author: Option<String>,
) -> Result<Option<ReviewSelection>, String> {
    ready_for_review_as_and_claim_auto_review(project_root, task_id, summary, author, true)
        .map(|transition| transition.selection)
        .map_err(|error| error.to_string())
}

/// Mark work ready for review with ownership guards and claim review metadata atomically.
pub fn ready_for_review_as_and_claim_auto_review(
    project_root: &Path,
    task_id: &str,
    summary: String,
    author: Option<String>,
    force: bool,
) -> Result<ReviewTransition, ReviewDispatchError> {
    transition_and_claim_auto_review(project_root, task_id, |board| {
        let transition =
            board_service::ready_for_review_as(board, task_id, summary.clone(), author, force)?;
        Ok((transition.task.clone(), transition.warnings))
    })
}

/// Claim a pending review for the visible UI path. `force` keeps the existing manual
/// retry behavior; automatic calls pass `false` and are duplicate-gated by metadata.
pub fn claim_pending_review(
    project_root: &Path,
    task_id: &str,
    requested_agent: Option<AgentKind>,
    requested_yolo: Option<bool>,
    force: bool,
) -> Result<Option<ReviewSelection>, String> {
    board_store::update(
        project_root,
        |board| -> Result<Option<ReviewSelection>, board_service::BoardError> {
            let task = match board_service::get_task(board, task_id).cloned() {
                Some(task) => task,
                None => return Ok(None),
            };
            if !force && !task.needs_auto_review() {
                return Ok(None);
            }

            let reviewer =
                requested_agent.unwrap_or_else(|| board_service::reviewer_for_task(board, &task));
            let yolo = requested_yolo.unwrap_or(board.automation.yolo_default);
            let task = board_service::start_review(board, task_id, reviewer)?.clone();
            Ok(Some(ReviewSelection {
                task,
                reviewer,
                yolo,
            }))
        },
    )
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

fn transition_and_claim_auto_review(
    project_root: &Path,
    task_id: &str,
    transition: impl FnOnce(
        &mut crate::model::board::Board,
    ) -> Result<(Task, Vec<String>), board_service::BoardError>,
) -> Result<ReviewTransition, ReviewDispatchError> {
    board_store::update(
        project_root,
        |board| -> Result<ReviewTransition, board_service::BoardError> {
            let (mut task, warnings) = transition(board)?;
            let selection = if task.status == TaskStatus::InReview && task.needs_auto_review() {
                let reviewer = board_service::reviewer_for_task(board, &task);
                let yolo = board.automation.yolo_default;
                task = board_service::start_review(board, task_id, reviewer)?.clone();
                Some(ReviewSelection {
                    task: task.clone(),
                    reviewer,
                    yolo,
                })
            } else {
                None
            };
            Ok(ReviewTransition {
                task,
                warnings,
                selection,
            })
        },
    )
    .map_err(|error| ReviewDispatchError::Storage(error.to_string()))?
    .map_err(ReviewDispatchError::Board)
}

/// Persist a review launch failure on the task as both metadata and a progress note.
pub fn record_review_spawn_failure(
    project_root: &Path,
    task_id: &str,
    reviewer: AgentKind,
    error: &str,
) -> Result<Task, String> {
    board_store::update(project_root, |board| {
        board_service::record_review_error(board, task_id, reviewer, error).cloned()
    })
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

/// Build the process spec for a headless reviewer. The cwd and log path are always rooted
/// at the configured project root, never the app or MCP process cwd.
pub fn build_headless_review_spec(
    project_root: &Path,
    selection: &ReviewSelection,
) -> HeadlessReviewProcessSpec {
    let log_path = board_store::board_dir(project_root)
        .join("reviews")
        .join(format!(
            "review-{}-{}.log",
            selection.task.id,
            now_epoch_secs()
        ));

    let prompt = build_headless_review_prompt(selection.reviewer, &selection.task);

    HeadlessReviewProcessSpec {
        program: selection.reviewer.binary().to_string(),
        args: build_headless_review_args(project_root, selection.reviewer, selection.yolo, prompt),
        current_dir: project_root.to_path_buf(),
        log_path,
    }
}

fn build_headless_review_args(
    project_root: &Path,
    agent: AgentKind,
    yolo: bool,
    prompt: String,
) -> Vec<String> {
    let mut args = match agent {
        AgentKind::Claude => vec!["--print".to_string()],
        AgentKind::Codex => {
            let mut args = agent_config::codex_project_mcp_overrides(project_root);
            args.push("exec".to_string());
            args
        }
    };
    if yolo {
        args.push(agent.yolo_flag().to_string());
    }
    args.push(prompt);
    args
}

/// Spawn a detached headless reviewer for a selection that has already been recorded in
/// the board. Agent MCP configuration is rewritten for this `project_root` immediately
/// before spawn so TerminalTiler MCP access uses `--project-root <project_root>`.
pub fn spawn_headless_review(
    project_root: &Path,
    selection: &ReviewSelection,
) -> Result<HeadlessReviewRun, String> {
    #[cfg(test)]
    if let Some(error) = FORCED_HEADLESS_REVIEW_ERROR.with(|forced| forced.borrow().clone()) {
        return Err(error);
    }

    if selection.reviewer == AgentKind::Claude {
        agent_config::connect_claude(project_root)?;
    }

    let spec = build_headless_review_spec(project_root, selection);

    #[cfg(test)]
    if DISABLE_HEADLESS_REVIEW_SPAWN.load(Ordering::SeqCst) {
        return Ok(HeadlessReviewRun {
            pid: 0,
            log_path: spec.log_path,
        });
    }

    fs::create_dir_all(
        spec.log_path
            .parent()
            .ok_or_else(|| "review log path has no parent directory".to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let log = File::create(&spec.log_path).map_err(|error| error.to_string())?;
    let err_log = log.try_clone().map_err(|error| error.to_string())?;

    let child = spawn_from_spec(&spec, log, err_log).map_err(|error| error.to_string())?;
    Ok(HeadlessReviewRun {
        pid: child.id(),
        log_path: spec.log_path,
    })
}

fn spawn_from_spec(
    spec: &HeadlessReviewProcessSpec,
    stdout: File,
    stderr: File,
) -> std::io::Result<Child> {
    Command::new(&spec.program)
        .args(&spec.args)
        .current_dir(&spec.current_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
}

fn build_headless_review_prompt(agent: AgentKind, task: &Task) -> String {
    let mut prompt = format!(
        "Run a code review for TerminalTiler Kanban task {id} titled \"{title}\".",
        id = task.id,
        title = task.title
    );
    let description = task.description.trim();
    if !description.is_empty() {
        prompt.push(' ');
        prompt.push_str(description);
    }
    prompt.push_str(&format!(
        " Your current working directory is the configured project root for this board. Inspect only this project/worktree for issues related to the task. You may call get_my_work for resume context, but review this explicit task id. Use the terminaltiler MCP tools to call submit_review with author \"{}-reviewer\", a verdict, and a concise severity-rated review summary. Leave the task in In Review; do not call complete_task.",
        agent.assignee_id()
    ));
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::board::Board;
    use crate::services::board::create_task;
    use std::fs;
    use uuid::Uuid;

    fn temp_root(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("terminaltiler-{prefix}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn status_transition_claims_one_review_under_project_root() {
        let root = temp_root("review-claim");
        let id = board_store::update(&root, |board| {
            create_task(board, "Review me", "", TaskStatus::Todo)
                .id
                .clone()
        })
        .unwrap();

        let first = set_status_and_claim_auto_review(&root, &id, TaskStatus::InReview)
            .unwrap()
            .expect("review selection");
        assert_eq!(first.reviewer, AgentKind::Claude);
        assert_eq!(first.task.review.attempts, 1);

        let second = set_status_and_claim_auto_review(&root, &id, TaskStatus::InReview).unwrap();
        assert!(second.is_none(), "metadata prevents duplicate auto-review");

        let board = board_store::load(&root);
        let task = board_service::get_task(&board, &id).unwrap();
        assert_eq!(task.status, TaskStatus::InReview);
        assert_eq!(task.review.attempts, 1);
        assert_eq!(task.review.last_reviewer, Some(AgentKind::Claude));
    }

    #[test]
    fn ui_and_mcp_claim_paths_share_duplicate_gate() {
        let root = temp_root("review-duplicate");
        let id = board_store::update(&root, |board| {
            create_task(board, "Review once", "", TaskStatus::Todo)
                .id
                .clone()
        })
        .unwrap();

        let mcp_selection = set_status_and_claim_auto_review(&root, &id, TaskStatus::InReview)
            .unwrap()
            .expect("mcp auto review");
        assert_eq!(mcp_selection.task.review.attempts, 1);

        let ui_selection = claim_pending_review(&root, &id, None, None, false).unwrap();
        assert!(ui_selection.is_none());
    }

    #[test]
    fn spawn_failure_does_not_resurrect_concurrently_moved_task() {
        let root = temp_root("review-failure-status");
        let id = board_store::update(&root, |board| {
            create_task(board, "Review then move", "", TaskStatus::Todo)
                .id
                .clone()
        })
        .unwrap();

        let selection = set_status_and_claim_auto_review(&root, &id, TaskStatus::InReview)
            .unwrap()
            .expect("review selection");

        board_store::update(&root, |board| {
            board_service::set_status(board, &id, TaskStatus::Complete).unwrap();
        })
        .unwrap();

        let recorded = record_review_spawn_failure(
            &root,
            &selection.task.id,
            selection.reviewer,
            "missing reviewer binary",
        )
        .unwrap();

        assert_eq!(recorded.status, TaskStatus::Complete);
        assert_eq!(
            recorded.review.last_error.as_deref(),
            Some("missing reviewer binary")
        );

        let board = board_store::load(&root);
        let task = board_service::get_task(&board, &id).unwrap();
        assert_eq!(task.status, TaskStatus::Complete);
        assert_eq!(
            task.latest_note(),
            Some("Review launch failed for Claude: missing reviewer binary")
        );
    }

    #[test]
    fn forced_ui_review_can_retry_after_auto_review() {
        let root = temp_root("review-force");
        let id = board_store::update(&root, |board| {
            create_task(board, "Review again", "", TaskStatus::InReview)
                .id
                .clone()
        })
        .unwrap();

        let first = claim_pending_review(&root, &id, None, None, false).unwrap();
        assert!(first.is_some());
        let forced = claim_pending_review(&root, &id, Some(AgentKind::Codex), Some(true), true)
            .unwrap()
            .expect("manual retry");
        assert_eq!(forced.reviewer, AgentKind::Codex);
        assert!(forced.yolo);
        assert_eq!(forced.task.review.attempts, 2);
    }

    #[test]
    fn headless_spec_uses_project_root_cwd_and_project_log_dir() {
        let root = temp_root("review-spec");
        let mut board = Board::default();
        let task =
            create_task(&mut board, "Review target", "details", TaskStatus::InReview).clone();
        let selection = ReviewSelection {
            task,
            reviewer: AgentKind::Codex,
            yolo: true,
        };

        let spec = build_headless_review_spec(&root, &selection);
        assert_eq!(spec.program, "codex");
        assert_eq!(spec.current_dir, root);
        assert_eq!(spec.args[0], "-C");
        assert_eq!(spec.args[1], spec.current_dir.to_string_lossy());
        assert!(
            spec.args
                .iter()
                .any(|arg| arg.starts_with("mcp_servers.terminaltiler.args="))
        );
        assert!(spec.args.iter().any(|arg| arg == "exec"));
        assert!(
            spec.args
                .iter()
                .any(|arg| arg.contains("dangerously-bypass"))
        );
        assert!(
            spec.args
                .last()
                .unwrap()
                .contains("configured project root")
        );
        assert_eq!(
            spec.log_path.parent().unwrap(),
            board_store::board_dir(&spec.current_dir).join("reviews")
        );
    }

    #[test]
    fn headless_review_uses_non_interactive_cli_modes() {
        let mut board = Board::default();
        let task =
            create_task(&mut board, "Review target", "details", TaskStatus::InReview).clone();

        let claude = build_headless_review_args(
            Path::new("/tmp/project"),
            AgentKind::Claude,
            false,
            build_headless_review_prompt(AgentKind::Claude, &task),
        );
        assert_eq!(claude[0], "--print");
        assert!(claude[1].contains("claude-reviewer"));
        assert!(claude[1].contains("get_my_work"));

        let codex = build_headless_review_args(
            Path::new("/tmp/project"),
            AgentKind::Codex,
            false,
            build_headless_review_prompt(AgentKind::Codex, &task),
        );
        assert_eq!(codex[0], "-C");
        assert!(codex.iter().any(|arg| arg == "exec"));
        let prompt = codex.last().unwrap();
        assert!(prompt.contains("codex-reviewer"));
        assert!(prompt.contains("get_my_work"));
    }
}
