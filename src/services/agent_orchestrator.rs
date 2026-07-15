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

use crate::model::agent_run::{
    AgentKind, AgentLaunchSpec, AgentRun, AgentRunKind, AgentRunOptions, AgentRunState,
};
use crate::model::assets::WorkspaceAssets;
use crate::model::board::Task;
use crate::model::layout::{WorkingDirectory, default_tile_spec};
use crate::model::preset::ApplicationDensity;
use crate::services::agent_config;
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
        let launch_spec = build_agent_launch_spec(project_root, agent, task, options);
        // Managed agent processes are spawned from a structured argv. Keeping the
        // prompt out of a shell command prevents quoting bugs and command injection.
        spec.startup_command = None;

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
            Some(launch_spec.argv()),
        );
        let terminal = session.widget();

        let run = AgentRun {
            id: run_id,
            task_id: task.id.clone(),
            task_title: task.title.clone(),
            agent_kind: agent,
            run_kind: options.kind,
            yolo: options.yolo,
            state: AgentRunState::Starting,
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
        let mut runs = self.runs.borrow_mut();
        refresh_completed_runs(&mut runs);
        runs.iter().map(|active| active.run.clone()).collect()
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
        refresh_completed_runs(&mut runs);
        if let Some(active) = runs.iter_mut().find(|active| active.run.id == run_id) {
            terminate_active_run_immediately(active, "agent run stopped by user");
        }
    }

    /// Queue text for one managed agent. The caller controls whether a trailing
    /// newline is included, making this usable for both draft text and submitted
    /// prompts without routing through a command shell.
    #[allow(dead_code)]
    pub fn queue_prompt(&self, run_id: &str, text: &str) -> bool {
        let mut runs = self.runs.borrow_mut();
        refresh_completed_runs(&mut runs);
        let Some(active) = runs.iter_mut().find(|active| active.run.id == run_id) else {
            return false;
        };
        active.run.state = AgentRunState::WaitingForInput;
        let sent = active.session.send_text(text);
        active.run.state = if sent {
            AgentRunState::Running
        } else {
            AgentRunState::Failed
        };
        sent
    }

    /// Send the terminal interrupt control character to one managed run.
    #[allow(dead_code)]
    pub fn interrupt(&self, run_id: &str) -> bool {
        let mut runs = self.runs.borrow_mut();
        refresh_completed_runs(&mut runs);
        let Some(active) = runs.iter_mut().find(|active| active.run.id == run_id) else {
            return false;
        };
        let sent = active.session.send_text("\u{3}");
        active.run.state = if sent {
            AgentRunState::Interrupted
        } else {
            AgentRunState::Failed
        };
        sent
    }

    /// Reflect that a provider is paused on a permission/approval gate.
    #[allow(dead_code)]
    pub fn mark_waiting_for_approval(&self, run_id: &str) -> bool {
        let mut runs = self.runs.borrow_mut();
        let Some(active) = runs.iter_mut().find(|active| active.run.id == run_id) else {
            return false;
        };
        if active.run.state.is_active() {
            active.run.state = AgentRunState::WaitingForApproval;
            true
        } else {
            false
        }
    }

    /// Stop every live run for a specific board task.
    pub fn stop_task(&self, task_id: &str, reason: &str) {
        let mut runs = self.runs.borrow_mut();
        refresh_completed_runs(&mut runs);
        for active in runs
            .iter_mut()
            .filter(|active| active.run.task_id == task_id)
        {
            terminate_active_run_immediately(active, reason);
        }
    }

    /// Terminate every live agent process owned by this board.
    pub fn terminate_all(&self, reason: &str) {
        let mut runs = self.runs.borrow_mut();
        refresh_completed_runs(&mut runs);
        for active in runs.iter_mut() {
            terminate_active_run_immediately(active, reason);
        }
    }
}

fn refresh_completed_runs(runs: &mut [ActiveRun]) {
    for active in runs {
        if active.run.state == AgentRunState::Starting && active.session.has_active_process() {
            active.run.state = AgentRunState::Running;
        } else if active.run.state.is_active() && !active.session.has_active_process() {
            active.run.state = match active.session.last_exit_status() {
                Some(0) | None => AgentRunState::Completed,
                Some(_) => AgentRunState::Failed,
            };
        }
    }
}

fn terminate_active_run_immediately(active: &mut ActiveRun, reason: &str) {
    if active.run.state.is_active() {
        active.run.state = AgentRunState::Stopping;
        active.session.terminate_immediately(reason);
        active.run.state = AgentRunState::Cancelled;
    }
}

fn build_agent_launch_spec(
    project_root: &Path,
    agent: AgentKind,
    task: &Task,
    options: AgentRunOptions,
) -> AgentLaunchSpec {
    let prompt = match options.kind {
        AgentRunKind::Implementation => build_implementation_prompt(project_root, agent, task),
        AgentRunKind::Review => build_review_prompt(project_root, agent, task),
    };

    let mut args = Vec::new();
    if agent == AgentKind::Codex {
        args.extend(agent_config::codex_project_mcp_overrides(project_root));
    }
    if options.yolo
        && let Some(flag) = agent.yolo_flag()
    {
        args.push(flag.to_string());
    }
    args.extend(agent.interactive_args(prompt));
    AgentLaunchSpec {
        program: agent.binary().to_string(),
        args,
    }
}

fn build_implementation_prompt(project_root: &Path, agent: AgentKind, task: &Task) -> String {
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
    append_task_context(&mut prompt, project_root, task);
    prompt.push_str(
        " Before and while implementing, research the relevant docs, APIs, and code context \
         for this task and record each useful finding by calling add_task_knowledge (a short \
         title plus the detail).",
    );
    prompt.push_str(&format!(
        " Use the terminaltiler MCP lifecycle tools with launched-agent assignee \"{assignee}\": \
         call get_my_work with assignee \"{assignee}\" first for resume context, \
         call start_work with assignee \"{assignee}\" to claim or resume it, call \
         heartbeat_task with assignee \"{assignee}\" and add_task_note to report progress, \
         and when implementation is ready for review call ready_for_review with author \"{assignee}\" \
         and a handoff summary. Do not mark the task Complete; completion remains a manual \
         board decision after review.",
        assignee = agent.assignee_id()
    ));
    prompt
}

fn build_review_prompt(project_root: &Path, agent: AgentKind, task: &Task) -> String {
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
    append_task_context(&mut prompt, project_root, task);
    prompt.push_str(&format!(
        " Inspect the current worktree/branch for issues related to this task. You may call get_my_work for resume context, but review this explicit task id. Use the terminaltiler MCP tools to call submit_review with author \"{}-reviewer\", a verdict, and a concise severity-rated review summary. Leave the task in In Review; do not call complete_task.",
        agent.assignee_id()
    ));
    prompt
}

/// Append the task's additional instructions and attachment paths (when present) to a prompt.
/// Attachments are referenced by absolute path so the agent can open them directly.
fn append_task_context(prompt: &mut String, project_root: &Path, task: &Task) {
    if let Some(instructions) = task
        .additional_instructions
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        prompt.push_str(" Additional instructions: ");
        prompt.push_str(instructions);
        if !instructions.ends_with('.') {
            prompt.push('.');
        }
    }
    if !task.attachments.is_empty() {
        let paths: Vec<String> = task
            .attachments
            .iter()
            .map(|attachment| project_root.join(&attachment.path).display().to_string())
            .collect();
        prompt.push_str(" Reference attachments (read these for context): ");
        prompt.push_str(&paths.join(", "));
        prompt.push('.');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::board::TaskStatus;
    use crate::services::board::create_task;

    #[test]
    fn agent_launch_keeps_prompt_as_a_single_argument() {
        let mut board = crate::model::board::Board::default();
        let task =
            create_task(&mut board, "Fix it's bug", "do the thing", TaskStatus::Todo).clone();

        let launch = build_agent_launch_spec(
            Path::new("/tmp/project"),
            AgentKind::Claude,
            &task,
            AgentRunOptions::implementation(false),
        );
        assert_eq!(launch.program, "claude");
        assert_eq!(launch.args.len(), 1);
        let prompt = &launch.args[0];
        assert!(prompt.contains("Fix it's bug"));
        assert!(prompt.contains("get_my_work with assignee \"claude\""));
        assert!(prompt.contains("start_work"));
        assert!(prompt.contains("start_work with assignee \"claude\""));
        assert!(prompt.contains("heartbeat_task with assignee \"claude\""));
        assert!(prompt.contains("ready_for_review"));
        assert!(prompt.contains("ready_for_review with author \"claude\""));
        assert!(prompt.contains("Do not mark the task Complete"));
        // Auto-gather directive is always present on implementation runs.
        assert!(prompt.contains("add_task_knowledge"));
    }

    #[test]
    fn implementation_prompt_uses_launched_agent_assignee_for_lifecycle_tools() {
        let mut board = crate::model::board::Board::default();
        let task = create_task(&mut board, "Lease-safe work", "", TaskStatus::Todo).clone();

        let launch = build_agent_launch_spec(
            Path::new("/tmp/project"),
            AgentKind::Codex,
            &task,
            AgentRunOptions::implementation(false),
        );
        let prompt = launch.args.last().unwrap();
        assert!(prompt.contains("launched-agent assignee \"codex\""));
        assert!(prompt.contains("get_my_work with assignee \"codex\""));
        assert!(prompt.contains("start_work with assignee \"codex\""));
        assert!(prompt.contains("heartbeat_task with assignee \"codex\""));
        assert!(prompt.contains("ready_for_review with author \"codex\""));
    }

    #[test]
    fn implementation_prompt_injects_instructions_and_attachments() {
        let mut board = crate::model::board::Board::default();
        let id = create_task(&mut board, "Task", "do it", TaskStatus::Todo)
            .id
            .clone();
        crate::services::board::set_additional_instructions(&mut board, &id, "use bunny CDN")
            .unwrap();
        crate::services::board::add_attachment(
            &mut board,
            &id,
            crate::model::board::TaskAttachment {
                path: format!(".terminaltiler/attachments/{id}/shot.png"),
                name: "shot.png".into(),
                mime_type: Some("image/png".into()),
                size_bytes: 1,
                added_at: 0,
            },
        )
        .unwrap();
        let task = crate::services::board::get_task(&board, &id)
            .unwrap()
            .clone();

        let launch = build_agent_launch_spec(
            Path::new("/tmp/project"),
            AgentKind::Claude,
            &task,
            AgentRunOptions::implementation(false),
        );
        let prompt = launch.args.last().unwrap();
        assert!(prompt.contains("Additional instructions: use bunny CDN."));
        assert!(prompt.contains("/tmp/project/.terminaltiler/attachments/"));
        assert!(prompt.contains("shot.png"));
    }

    #[test]
    fn yolo_flags_are_cli_specific() {
        let mut board = crate::model::board::Board::default();
        let task = create_task(&mut board, "Task", "", TaskStatus::Todo).clone();

        let codex = build_agent_launch_spec(
            Path::new("/tmp/project"),
            AgentKind::Codex,
            &task,
            AgentRunOptions::implementation(true),
        );
        assert_eq!(codex.program, "codex");
        assert!(codex.args.iter().any(|arg| arg == "-C"));
        assert!(codex.args.iter().any(|arg| arg == "/tmp/project"));
        assert!(
            codex
                .args
                .iter()
                .any(|arg| arg.contains("mcp_servers.terminaltiler.command"))
        );
        assert!(
            codex
                .args
                .iter()
                .any(|arg| arg.contains("mcp_servers.terminaltiler.args"))
        );
        assert!(
            codex
                .args
                .iter()
                .any(|arg| arg == "--dangerously-bypass-approvals-and-sandbox")
        );

        let claude = build_agent_launch_spec(
            Path::new("/tmp/project"),
            AgentKind::Claude,
            &task,
            AgentRunOptions::implementation(true),
        );
        assert_eq!(claude.program, "claude");
        assert_eq!(claude.args[0], "--dangerously-skip-permissions");

        let opencode = build_agent_launch_spec(
            Path::new("/tmp/project"),
            AgentKind::Opencode,
            &task,
            AgentRunOptions::implementation(true),
        );
        assert!(opencode.args.iter().all(|arg| !arg.contains("approve")));

        let copilot = build_agent_launch_spec(
            Path::new("/tmp/project"),
            AgentKind::Copilot,
            &task,
            AgentRunOptions::implementation(true),
        );
        assert_eq!(copilot.args[0], "--yolo");

        let grok = build_agent_launch_spec(
            Path::new("/tmp/project"),
            AgentKind::Grok,
            &task,
            AgentRunOptions::implementation(true),
        );
        assert_eq!(grok.args[0], "--always-approve");
    }

    #[test]
    fn review_command_prompts_for_note_and_leaves_in_review() {
        let mut board = crate::model::board::Board::default();
        let task = create_task(&mut board, "Review target", "", TaskStatus::InReview).clone();

        let launch = build_agent_launch_spec(
            Path::new("/tmp/project"),
            AgentKind::Codex,
            &task,
            AgentRunOptions::review(false),
        );
        assert_eq!(launch.program, "codex");
        let prompt = launch.args.last().unwrap();
        assert!(prompt.contains("Run a code review"));
        assert!(prompt.contains("get_my_work"));
        assert!(prompt.contains("submit_review"));
        assert!(prompt.contains("codex-reviewer"));
        assert!(prompt.contains("Leave the task in In Review"));
        assert!(prompt.contains("do not call complete_task"));
    }

    #[test]
    fn all_supported_agents_build_provider_specific_interactive_argv() {
        let mut board = crate::model::board::Board::default();
        let task = create_task(&mut board, "Task", "", TaskStatus::Todo).clone();

        for agent in AgentKind::ALL {
            let launch = build_agent_launch_spec(
                Path::new("/tmp/project"),
                agent,
                &task,
                AgentRunOptions::implementation(false),
            );
            assert_eq!(launch.program, agent.binary());
            assert!(!launch.args.is_empty());
            assert!(
                launch
                    .args
                    .last()
                    .unwrap()
                    .contains("TerminalTiler Kanban task")
            );
        }
    }
}
