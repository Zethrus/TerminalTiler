//! Parallel agent orchestration for the Kanban board.
//!
//! Dispatching a task spawns a real terminal session running an agent CLI (Claude/Codex)
//! in the project root, seeded with a prompt that points the agent at the board's MCP
//! tools. Each dispatch is independent, so many agents can work concurrently. The board UI
//! owns the spawned terminals (so they are viewable); the agent reports progress back
//! through the MCP server.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use uuid::Uuid;

use crate::model::agent_run::{AgentKind, AgentRun, AgentRunKind, AgentRunOptions, AgentRunState};
use crate::model::assets::WorkspaceAssets;
use crate::model::board::Task;
use crate::model::layout::{WorkingDirectory, default_tile_spec};
use crate::model::preset::ApplicationDensity;
use crate::services::stats::StatsRecorder;
use crate::terminal::session::TerminalSession;

struct ActiveRun {
    run: AgentRun,
    session: TerminalSession,
}

/// Tracks all agent runs for a board and owns their live terminal sessions.
#[derive(Clone, Default)]
pub struct AgentOrchestrator {
    runs: Rc<RefCell<Vec<ActiveRun>>>,
}

/// Result of a dispatch: the run record plus the live terminal widget to display.
pub struct DispatchedAgent {
    pub run: AgentRun,
    pub terminal: vte4::Terminal,
}

impl AgentOrchestrator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawn an agent to work/review `task` in `project_root` and register the run.
    pub fn dispatch(
        &self,
        project_root: &Path,
        agent: AgentKind,
        task: &Task,
        options: AgentRunOptions,
        use_dark_palette: bool,
        density: ApplicationDensity,
    ) -> DispatchedAgent {
        let run_id = Uuid::new_v4().simple().to_string();
        let tile_id = format!("agent-{run_id}");

        let mut spec = default_tile_spec(0);
        spec.id = tile_id.clone();
        spec.title = match options.kind {
            AgentRunKind::Implementation => task.title.clone(),
            AgentRunKind::Review => format!("Review: {}", task.title),
        };
        spec.agent_label = if options.yolo {
            format!("{} YOLO", agent.label())
        } else {
            agent.label().to_string()
        };
        spec.accent_class = "accent-amber".into();
        spec.working_directory = WorkingDirectory::Absolute(project_root.to_path_buf());
        spec.startup_command = Some(build_agent_command(agent, task, options));

        let assets = WorkspaceAssets::default();
        let session = TerminalSession::spawn(
            &spec,
            project_root,
            &assets,
            use_dark_palette,
            density,
            0,
            &[],
            None,
            StatsRecorder::default(),
        );
        let terminal = session.widget();

        let run = AgentRun {
            id: run_id,
            task_id: task.id.clone(),
            task_title: task.title.clone(),
            agent_kind: agent,
            run_kind: options.kind,
            yolo: options.yolo,
            state: AgentRunState::Running,
        };

        self.runs.borrow_mut().push(ActiveRun {
            run: run.clone(),
            session,
        });

        DispatchedAgent { run, terminal }
    }

    /// Snapshot of all runs, with active runs whose process has exited promoted to
    /// `Completed` so the UI reflects reality without a separate exit callback.
    pub fn runs(&self) -> Vec<AgentRun> {
        self.runs
            .borrow()
            .iter()
            .map(|active| {
                let mut run = active.run.clone();
                if run.state.is_active() && !active.session.has_active_process() {
                    run.state = AgentRunState::Completed;
                }
                run
            })
            .collect()
    }

    /// Whether any registered agent session still has a live process.
    pub fn has_active_processes(&self) -> bool {
        self.runs
            .borrow()
            .iter()
            .any(|active| active.run.state.is_active() && active.session.has_active_process())
    }

    /// Stop a run: terminate its process and mark it cancelled.
    pub fn stop(&self, run_id: &str) {
        let mut runs = self.runs.borrow_mut();
        if let Some(active) = runs.iter_mut().find(|active| active.run.id == run_id) {
            terminate_active_run(active, "agent run stopped by user");
        }
    }

    /// Stop every live run for a specific board task.
    pub fn stop_task(&self, task_id: &str, reason: &str) {
        let mut runs = self.runs.borrow_mut();
        for active in runs
            .iter_mut()
            .filter(|active| active.run.task_id == task_id)
        {
            terminate_active_run(active, reason);
        }
    }

    /// Terminate every live agent process owned by this board.
    pub fn terminate_all(&self, reason: &str) {
        let mut runs = self.runs.borrow_mut();
        for active in runs.iter_mut() {
            terminate_active_run(active, reason);
        }
    }
}

fn terminate_active_run(active: &mut ActiveRun, reason: &str) {
    if active.run.state.is_active() && active.session.has_active_process() {
        active.session.terminate(reason);
        active.run.state = AgentRunState::Cancelled;
    }
}

fn build_agent_command(agent: AgentKind, task: &Task, options: AgentRunOptions) -> String {
    let prompt = match options.kind {
        AgentRunKind::Implementation => build_implementation_prompt(task),
        AgentRunKind::Review => build_review_prompt(agent, task),
    };

    let mut parts = vec![agent.binary().to_string()];
    if options.yolo {
        parts.push(agent.yolo_flag().to_string());
    }
    parts.push(shell_quote(&prompt));
    parts.join(" ")
}

fn build_implementation_prompt(task: &Task) -> String {
    let mut prompt = format!(
        "You are working on TerminalTiler Kanban task {id} titled \"{title}\".",
        id = task.id,
        title = task.title
    );
    let description = task.description.trim();
    if !description.is_empty() {
        prompt.push(' ');
        prompt.push_str(description);
    }
    prompt.push_str(
        " Use the terminaltiler MCP tools: call claim_task to mark it In Progress, \
         add_task_note to report progress, and when implementation is ready for review call \
         update_task_status with status \"in_review\". Do not mark the task Complete; \
         completion remains a manual board decision after review.",
    );
    prompt
}

fn build_review_prompt(agent: AgentKind, task: &Task) -> String {
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
        " Inspect the current worktree/branch for issues related to this task. Use the terminaltiler MCP tools to call add_task_note with author \"{}-reviewer\" and a concise severity-rated review summary. Leave the task in In Review; do not call complete_task.",
        agent.assignee_id()
    ));
    prompt
}

/// POSIX single-quote escaping so the prompt survives the shell as one argument.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::board::TaskStatus;
    use crate::services::board::create_task;

    #[test]
    fn agent_command_quotes_prompt_and_targets_binary() {
        let mut board = crate::model::board::Board::default();
        let task =
            create_task(&mut board, "Fix it's bug", "do the thing", TaskStatus::Todo).clone();

        let command = build_agent_command(
            AgentKind::Claude,
            &task,
            AgentRunOptions::implementation(false),
        );
        assert!(command.starts_with("claude '"));
        assert!(command.contains("Fix it'\\''s bug"));
        assert!(command.contains("update_task_status"));
        assert!(command.contains("in_review"));
        assert!(command.contains("Do not mark the task Complete"));
    }

    #[test]
    fn yolo_flags_are_cli_specific() {
        let mut board = crate::model::board::Board::default();
        let task = create_task(&mut board, "Task", "", TaskStatus::Todo).clone();

        let codex = build_agent_command(
            AgentKind::Codex,
            &task,
            AgentRunOptions::implementation(true),
        );
        assert!(codex.starts_with("codex --dangerously-bypass-approvals-and-sandbox "));

        let claude = build_agent_command(
            AgentKind::Claude,
            &task,
            AgentRunOptions::implementation(true),
        );
        assert!(claude.starts_with("claude --dangerously-skip-permissions "));
    }

    #[test]
    fn review_command_prompts_for_note_and_leaves_in_review() {
        let mut board = crate::model::board::Board::default();
        let task = create_task(&mut board, "Review target", "", TaskStatus::InReview).clone();

        let command = build_agent_command(AgentKind::Codex, &task, AgentRunOptions::review(false));
        assert!(command.starts_with("codex '"));
        assert!(command.contains("Run a code review"));
        assert!(command.contains("add_task_note"));
        assert!(command.contains("codex-reviewer"));
        assert!(command.contains("Leave the task in In Review"));
        assert!(command.contains("do not call complete_task"));
    }
}
