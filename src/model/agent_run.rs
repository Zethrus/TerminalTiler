//! Live agent-run model.
//!
//! An [`AgentRun`] tracks one dispatched task being worked by an agent CLI in a live
//! terminal session. Runs are in-memory only — the durable record of agent work is the
//! task's `assignee` + progress notes on the board (written by the agent over MCP).
//!
//! Pure data only (no GTK) so it stays platform-agnostic; the orchestrator that spawns
//! terminals lives in `services::agent_orchestrator`.

use serde::{Deserialize, Serialize};

/// Which agent CLI backs a run. Claude and Codex are supported first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Claude,
    Codex,
}

impl AgentKind {
    /// Agent kinds offered in the dispatch picker, in display order.
    pub const ALL: [AgentKind; 2] = [AgentKind::Claude, AgentKind::Codex];

    /// Display label for buttons and run rows.
    pub fn label(self) -> &'static str {
        match self {
            AgentKind::Claude => "Claude",
            AgentKind::Codex => "Codex",
        }
    }

    /// Executable invoked to start the agent.
    pub fn binary(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
        }
    }

    /// Assignee string recorded on a task when this agent claims it.
    pub fn assignee_id(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
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
    pub fn yolo_flag(self) -> &'static str {
        match self {
            AgentKind::Claude => "--dangerously-skip-permissions",
            AgentKind::Codex => "--dangerously-bypass-approvals-and-sandbox",
        }
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentRunState {
    Running,
    Completed,
    Cancelled,
}

impl AgentRunState {
    /// Short label for the status chip.
    pub fn label(self) -> &'static str {
        match self {
            AgentRunState::Running => "Running",
            AgentRunState::Completed => "Completed",
            AgentRunState::Cancelled => "Cancelled",
        }
    }

    /// Whether the run's terminal process is still active.
    pub fn is_active(self) -> bool {
        matches!(self, AgentRunState::Running)
    }
}

/// One dispatched task being executed by an agent in a live terminal session.
#[derive(Clone, Debug)]
pub struct AgentRun {
    pub id: String,
    pub task_title: String,
    pub agent_kind: AgentKind,
    pub run_kind: AgentRunKind,
    pub yolo: bool,
    pub state: AgentRunState,
}
