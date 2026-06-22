//! Kanban board data model.
//!
//! A board is per-project and persisted to `<project_root>/.terminaltiler/board.json`.
//! It is the shared source of truth between the GTK board UI and the `terminaltiler-mcp`
//! server that AI agents talk to. The types here are pure data (no GTK, no I/O) so they
//! compile on every platform and can be reused by the MCP binary.

use serde::{Deserialize, Serialize};

use crate::model::agent_run::AgentKind;

/// Schema version for `board.json`. Bump when the on-disk shape changes incompatibly.
pub const BOARD_VERSION: u32 = 2;

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

/// Board-wide defaults for agent automation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardAutomation {
    /// Default implementation agent used by the UI's default run action.
    #[serde(
        default = "default_agent_kind",
        skip_serializing_if = "Option::is_none"
    )]
    pub default_agent: Option<AgentKind>,
    /// Default reviewer when a task in review has no recognized assignee.
    #[serde(
        default = "default_agent_kind",
        skip_serializing_if = "Option::is_none"
    )]
    pub default_reviewer: Option<AgentKind>,
    /// Whether UI-dispatched agent runs should use each CLI's unsafe/no-approval flag by default.
    #[serde(default)]
    pub yolo_default: bool,
}

impl Default for BoardAutomation {
    fn default() -> Self {
        Self {
            default_agent: default_agent_kind(),
            default_reviewer: default_agent_kind(),
            yolo_default: false,
        }
    }
}

fn default_agent_kind() -> Option<AgentKind> {
    Some(AgentKind::Claude)
}

/// Review dispatch bookkeeping for a task.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskReviewMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_started_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reviewer: Option<AgentKind>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub attempts: u32,
}

impl TaskReviewMetadata {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// Paused ownership metadata. Paused tasks keep their column and assignee, but no longer
/// advertise an active soft lease.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskPausedMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub paused_at: u64,
}

/// Machine-readable blocker metadata. Blocked state is intentionally metadata-only so the
/// five board columns remain stable and legacy boards continue to load unchanged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskBlockedMetadata {
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub blocked_at: u64,
}

/// A single progress note appended to a task (typically by an agent via MCP).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskNote {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub created_at: u64,
}

/// A discrete piece of knowledge captured for a task — typically researched and recorded
/// by an agent via the `add_task_knowledge` MCP tool as it works.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub title: String,
    pub content: String,
    /// Origin of the entry, e.g. `"agent"` or `"user"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Free-form tag, e.g. `"api_ref"`, `"blocker"`, `"example"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub created_at: u64,
}

/// A file attachment for a task. The file is copied into the project under
/// `.terminaltiler/attachments/<task_id>/` and referenced here by a path relative to the
/// project root, so the board stays portable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskAttachment {
    /// Path relative to the project root, e.g.
    /// `".terminaltiler/attachments/<task_id>/shot.png"`.
    pub path: String,
    /// Original filename, shown in the UI.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub size_bytes: u64,
    pub added_at: u64,
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
    /// When the current assignee first claimed or resumed active work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_at: Option<u64>,
    /// Last active-work heartbeat from the assignee.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heartbeat_at: Option<u64>,
    /// Per-task soft-lease threshold. Missing values use the service default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_after_secs: Option<u64>,
    /// Paused ownership metadata. A paused task can still have an assignee.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused: Option<TaskPausedMetadata>,
    /// Blocker metadata. This does not affect the task's column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<TaskBlockedMetadata>,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<TaskNote>,
    #[serde(default, skip_serializing_if = "TaskReviewMetadata::is_default")]
    pub review: TaskReviewMetadata,
    /// Extra instructions for agents working this task, injected into the seed prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additional_instructions: Option<String>,
    /// Knowledge captured for this task (by agents or the user).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knowledge: Vec<KnowledgeEntry>,
    /// Files attached for additional context.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<TaskAttachment>,
}

impl Task {
    /// Latest progress note text, if any — handy for the agents panel summary line.
    pub fn latest_note(&self) -> Option<&str> {
        self.notes.last().map(|note| note.text.as_str())
    }

    /// Whether this task carries non-empty additional instructions.
    pub fn has_instructions(&self) -> bool {
        self.additional_instructions
            .as_deref()
            .is_some_and(|text| !text.trim().is_empty())
    }

    /// Whether the UI should start one automatic review for this task.
    pub fn needs_auto_review(&self) -> bool {
        self.status == TaskStatus::InReview && self.review.last_started_at.is_none()
    }
}

/// The whole board: an ordered list of tasks plus a schema version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Board {
    #[serde(default = "default_board_version")]
    pub version: u32,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub automation: BoardAutomation,
}

fn default_board_version() -> u32 {
    BOARD_VERSION
}

impl Default for Board {
    fn default() -> Self {
        Self {
            version: BOARD_VERSION,
            tasks: Vec::new(),
            automation: BoardAutomation::default(),
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
        assert_eq!(board.automation.default_agent, Some(AgentKind::Claude));
        assert_eq!(board.automation.default_reviewer, Some(AgentKind::Claude));
        assert!(!board.automation.yolo_default);
    }

    #[test]
    fn old_board_json_loads_with_automation_and_review_defaults() {
        let raw = r#"{
            "version": 1,
            "tasks": [{
                "id": "task-1",
                "title": "Review me",
                "description": "legacy",
                "status": "in_review",
                "created_at": 10,
                "updated_at": 11
            }]
        }"#;

        let board: Board = serde_json::from_str(raw).unwrap();
        assert_eq!(board.automation, BoardAutomation::default());
        assert_eq!(board.tasks[0].review, TaskReviewMetadata::default());
        assert!(board.tasks[0].needs_auto_review());
    }

    #[test]
    fn old_board_json_loads_with_new_task_fields_defaulted() {
        let raw = r#"{
            "version": 1,
            "tasks": [{
                "id": "task-1",
                "title": "Legacy task",
                "description": "no extra fields",
                "status": "todo",
                "created_at": 10,
                "updated_at": 11
            }]
        }"#;

        let board: Board = serde_json::from_str(raw).unwrap();
        let task = &board.tasks[0];
        assert_eq!(task.additional_instructions, None);
        assert!(task.knowledge.is_empty());
        assert!(task.attachments.is_empty());
        assert_eq!(task.claimed_at, None);
        assert_eq!(task.heartbeat_at, None);
        assert_eq!(task.stale_after_secs, None);
        assert_eq!(task.paused, None);
        assert_eq!(task.blocked, None);
        assert!(!task.has_instructions());
    }
}
