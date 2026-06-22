//! Kanban board data model.
//!
//! A board is per-project and persisted to `<project_root>/.terminaltiler/board.json`.
//! It is the shared source of truth between the GTK board UI and the `terminaltiler-mcp`
//! server that AI agents talk to. The types here are pure data (no GTK, no I/O) so they
//! compile on every platform and can be reused by the MCP binary.

use serde::{Deserialize, Serialize};

/// Schema version for `board.json`. Bump when the on-disk shape changes incompatibly.
pub const BOARD_VERSION: u32 = 1;

/// Kanban column a task currently lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    InProgress,
    InReview,
    Complete,
    Cancelled,
}

impl TaskStatus {
    /// Columns in left-to-right board order.
    pub const ALL: [TaskStatus; 5] = [
        TaskStatus::Todo,
        TaskStatus::InProgress,
        TaskStatus::InReview,
        TaskStatus::Complete,
        TaskStatus::Cancelled,
    ];

    /// Human-readable column heading shown in the UI.
    pub fn column_title(self) -> &'static str {
        match self {
            TaskStatus::Todo => "To Do",
            TaskStatus::InProgress => "In Progress",
            TaskStatus::InReview => "In Review",
            TaskStatus::Complete => "Complete",
            TaskStatus::Cancelled => "Cancelled",
        }
    }

    /// Stable wire identifier, matching the serde representation. Used by the MCP tools
    /// so agents pass `"in_progress"` etc.
    pub fn wire_id(self) -> &'static str {
        match self {
            TaskStatus::Todo => "todo",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::InReview => "in_review",
            TaskStatus::Complete => "complete",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    /// Parse a wire identifier back into a status.
    pub fn from_wire(value: &str) -> Option<Self> {
        TaskStatus::ALL
            .into_iter()
            .find(|status| status.wire_id() == value)
    }
}

/// A single progress note appended to a task (typically by an agent via MCP).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskNote {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub created_at: u64,
}

/// A board task / card.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<TaskNote>,
}

impl Task {
    /// Latest progress note text, if any — handy for the agents panel summary line.
    pub fn latest_note(&self) -> Option<&str> {
        self.notes.last().map(|note| note.text.as_str())
    }
}

/// The whole board: an ordered list of tasks plus a schema version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Board {
    #[serde(default = "default_board_version")]
    pub version: u32,
    #[serde(default)]
    pub tasks: Vec<Task>,
}

fn default_board_version() -> u32 {
    BOARD_VERSION
}

impl Default for Board {
    fn default() -> Self {
        Self {
            version: BOARD_VERSION,
            tasks: Vec::new(),
        }
    }
}

/// Current Unix time in whole seconds, saturating to 0 before the epoch.
pub fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_wire_ids_round_trip() {
        for status in TaskStatus::ALL {
            assert_eq!(TaskStatus::from_wire(status.wire_id()), Some(status));
        }
        assert_eq!(TaskStatus::from_wire("nonsense"), None);
    }

    #[test]
    fn status_serde_matches_wire_id() {
        for status in TaskStatus::ALL {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, format!("\"{}\"", status.wire_id()));
        }
    }

    #[test]
    fn board_default_is_empty_current_version() {
        let board = Board::default();
        assert_eq!(board.version, BOARD_VERSION);
        assert!(board.tasks.is_empty());
    }
}
