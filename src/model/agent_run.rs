//! Live agent-run model.
//!
//! An [`AgentRun`] tracks one dispatched task being worked by an agent CLI in a live
//! terminal session. Runs are in-memory only — the durable record of agent work is the
//! task's `assignee` + progress notes on the board (written by the agent over MCP).
//!
//! Pure data only (no GTK) so it stays platform-agnostic; the orchestrator that spawns
//! terminals lives in `services::agent_orchestrator`.

use serde::{Deserialize, Serialize};

/// Which agent CLI backs a managed or detected run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Claude,
    Codex,
    Opencode,
    Copilot,
    Grok,
}

impl AgentKind {
    /// Agent kinds offered in the dispatch picker, in display order.
    pub const ALL: [AgentKind; 5] = [
        AgentKind::Claude,
        AgentKind::Codex,
        AgentKind::Opencode,
        AgentKind::Copilot,
        AgentKind::Grok,
    ];

    /// Display label for buttons and run rows.
    pub fn label(self) -> &'static str {
        match self {
            AgentKind::Claude => "Claude",
            AgentKind::Codex => "Codex",
            AgentKind::Opencode => "OpenCode",
            AgentKind::Copilot => "GitHub Copilot",
            AgentKind::Grok => "Grok",
        }
    }

    /// Executable invoked to start the agent.
    pub fn binary(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
            AgentKind::Opencode => "opencode",
            AgentKind::Copilot => "copilot",
            AgentKind::Grok => "grok",
        }
    }

    /// Assignee string recorded on a task when this agent claims it.
    pub fn assignee_id(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
            AgentKind::Opencode => "opencode",
            AgentKind::Copilot => "copilot",
            AgentKind::Grok => "grok",
        }
    }

    /// Parse either a stable assignee id or display label into an agent kind.
    pub fn from_assignee_id(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|agent| {
            value.eq_ignore_ascii_case(agent.assignee_id())
                || value.eq_ignore_ascii_case(agent.label())
                || value.eq_ignore_ascii_case(agent.binary())
        })
    }

    /// CLI flag that enables the agent's unsafe/no-approval mode.
    pub fn yolo_flag(self) -> Option<&'static str> {
        match self {
            AgentKind::Claude => Some("--dangerously-skip-permissions"),
            AgentKind::Codex => Some("--dangerously-bypass-approvals-and-sandbox"),
            AgentKind::Opencode => None,
            AgentKind::Copilot => Some("--yolo"),
            AgentKind::Grok => Some("--always-approve"),
        }
    }

    pub fn interactive_args(self, prompt: String) -> Vec<String> {
        match self {
            AgentKind::Claude | AgentKind::Codex | AgentKind::Grok => vec![prompt],
            AgentKind::Opencode => vec!["--prompt".into(), prompt],
            AgentKind::Copilot => vec!["--interactive".into(), prompt],
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentLaunchSpec {
    pub program: String,
    pub args: Vec<String>,
}

impl AgentLaunchSpec {
    pub fn argv(&self) -> Vec<String> {
        std::iter::once(self.program.clone())
            .chain(self.args.clone())
            .collect()
    }
}

/// Why an agent terminal was dispatched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentRunKind {
    Implementation,
    Review,
}

impl AgentRunKind {
    pub fn label(self) -> &'static str {
        match self {
            AgentRunKind::Implementation => "Implementation",
            AgentRunKind::Review => "Review",
        }
    }
}

/// Options controlling a single agent CLI invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentRunOptions {
    pub kind: AgentRunKind,
    pub yolo: bool,
}

impl AgentRunOptions {
    pub fn implementation(yolo: bool) -> Self {
        Self {
            kind: AgentRunKind::Implementation,
            yolo,
        }
    }

    pub fn review(yolo: bool) -> Self {
        Self {
            kind: AgentRunKind::Review,
            yolo,
        }
    }
}

/// Lifecycle state of an agent run.
// Some provider-reported states are consumed through the extension runtime in Pro
// builds before the open-core board UI renders them directly.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentRunState {
    Starting,
    Running,
    WaitingForInput,
    WaitingForApproval,
    Interrupted,
    Stopping,
    Completed,
    Cancelled,
    Failed,
}

impl AgentRunState {
    /// Short label for the status chip.
    pub fn label(self) -> &'static str {
        match self {
            AgentRunState::Starting => "Starting",
            AgentRunState::Running => "Running",
            AgentRunState::WaitingForInput => "Waiting for input",
            AgentRunState::WaitingForApproval => "Waiting for approval",
            AgentRunState::Interrupted => "Interrupted",
            AgentRunState::Stopping => "Stopping",
            AgentRunState::Completed => "Completed",
            AgentRunState::Cancelled => "Cancelled",
            AgentRunState::Failed => "Failed",
        }
    }

    /// Whether the run's terminal process is still active.
    pub fn is_active(self) -> bool {
        matches!(
            self,
            AgentRunState::Starting
                | AgentRunState::Running
                | AgentRunState::WaitingForInput
                | AgentRunState::WaitingForApproval
                | AgentRunState::Interrupted
                | AgentRunState::Stopping
        )
    }
}

/// One dispatched task being executed by an agent in a live terminal session.
#[derive(Clone, Debug)]
pub struct AgentRun {
    pub id: String,
    pub task_id: String,
    pub task_title: String,
    pub agent_kind: AgentKind,
    pub run_kind: AgentRunKind,
    pub yolo: bool,
    pub state: AgentRunState,
}

#[cfg(test)]
mod tests {
    use super::AgentKind;

    #[test]
    fn no_approval_flags_match_each_supported_cli() {
        assert_eq!(
            AgentKind::Claude.yolo_flag(),
            Some("--dangerously-skip-permissions")
        );
        assert_eq!(
            AgentKind::Codex.yolo_flag(),
            Some("--dangerously-bypass-approvals-and-sandbox")
        );
        assert_eq!(AgentKind::Opencode.yolo_flag(), None);
        assert_eq!(AgentKind::Copilot.yolo_flag(), Some("--yolo"));
        assert_eq!(AgentKind::Grok.yolo_flag(), Some("--always-approve"));
    }
}
