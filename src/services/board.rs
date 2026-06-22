//! Board operations shared by the GTK UI and the `terminaltiler-mcp` server.
//!
//! These are pure functions over an in-memory [`Board`] — no GTK, no disk I/O — so there
//! is exactly one implementation of "claim a task", "complete a task", etc. for both
//! front-ends. Persistence is the caller's job (see `storage::board_store`).

use uuid::Uuid;

use crate::model::agent_run::AgentKind;
use crate::model::board::{
    Board, KnowledgeEntry, Task, TaskAttachment, TaskBlockedMetadata, TaskNote, TaskPausedMetadata,
    TaskReviewMetadata, TaskStatus, now_epoch_secs,
};

/// Default soft-lease staleness threshold for active MCP work: six hours.
pub const DEFAULT_STALE_AFTER_SECS: u64 = 6 * 60 * 60;

/// Errors a board mutation can produce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardError {
    /// No task with the given id exists on the board.
    TaskNotFound(String),
    /// Another assignee currently owns a fresh active soft lease for the task.
    OwnershipConflict(TaskOwnershipConflict),
}

/// Details for an ownership conflict that MCP callers can expose as structured content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskOwnershipConflict {
    pub task_id: String,
    pub current_assignee: String,
    pub requested_assignee: String,
    pub heartbeat_at: Option<u64>,
    pub claimed_at: Option<u64>,
    pub stale_after_secs: u64,
    pub now: u64,
}

impl std::fmt::Display for BoardError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BoardError::TaskNotFound(id) => write!(formatter, "no task with id '{id}'"),
            BoardError::OwnershipConflict(conflict) => write!(
                formatter,
                "task '{}' is already owned by '{}' with a fresh active lease; '{}' must wait, use force, or take over after staleness",
                conflict.task_id, conflict.current_assignee, conflict.requested_assignee
            ),
        }
    }
}

impl std::error::Error for BoardError {}

/// Append a new task to the board and return it.
pub fn create_task(
    board: &mut Board,
    title: impl Into<String>,
    description: impl Into<String>,
    status: TaskStatus,
) -> &Task {
    let now = now_epoch_secs();
    board.tasks.push(Task {
        id: Uuid::new_v4().simple().to_string(),
        title: title.into(),
        description: description.into(),
        status,
        assignee: None,
        created_at: now,
        updated_at: now,
        notes: Vec::new(),
        review: TaskReviewMetadata::default(),
        additional_instructions: None,
        knowledge: Vec::new(),
        attachments: Vec::new(),
        claimed_at: None,
        heartbeat_at: None,
        stale_after_secs: None,
        paused: None,
        blocked: None,
    });
    board
        .tasks
        .last()
        .expect("task was just pushed and must exist")
}

/// Look up a task by id.
pub fn get_task<'a>(board: &'a Board, id: &str) -> Option<&'a Task> {
    board.tasks.iter().find(|task| task.id == id)
}

/// Tasks currently in a given column.
pub fn tasks_by_status(board: &Board, status: TaskStatus) -> Vec<&Task> {
    board
        .tasks
        .iter()
        .filter(|task| task.status == status)
        .collect()
}

/// Move a task to a new column.
pub fn set_status<'a>(
    board: &'a mut Board,
    id: &str,
    status: TaskStatus,
) -> Result<&'a Task, BoardError> {
    let task = task_mut(board, id)?;
    task.status = status;
    if matches!(status, TaskStatus::Todo | TaskStatus::InProgress) {
        task.review = TaskReviewMetadata::default();
    }
    if status == TaskStatus::InReview {
        clear_active_lease(task);
    }
    if matches!(
        status,
        TaskStatus::Todo | TaskStatus::Complete | TaskStatus::Cancelled
    ) {
        clear_active_lifecycle(task);
    }
    task.updated_at = now_epoch_secs();
    Ok(&*task)
}

/// Claim a task for an agent: move it to In Progress and record the assignee.
#[allow(dead_code)]
pub fn claim_task<'a>(
    board: &'a mut Board,
    id: &str,
    assignee: impl Into<String>,
) -> Result<&'a Task, BoardError> {
    Ok(start_work(board, id, assignee, None, false)?.task)
}

/// Start or resume active work with soft-lease conflict checks.
pub fn start_work<'a>(
    board: &'a mut Board,
    id: &str,
    assignee: impl Into<String>,
    stale_after_secs: Option<u64>,
    force: bool,
) -> Result<LifecycleTransition<&'a Task>, BoardError> {
    let assignee = assignee.into();
    let now = now_epoch_secs();
    let task = task_mut(board, id)?;
    let mut warnings = Vec::new();
    guard_active_owner(task, &assignee, now, force, &mut warnings)?;
    task.status = TaskStatus::InProgress;
    task.assignee = Some(assignee);
    task.claimed_at = Some(now);
    task.heartbeat_at = Some(now);
    task.stale_after_secs = Some(stale_after_secs.unwrap_or(DEFAULT_STALE_AFTER_SECS));
    task.paused = None;
    task.review = TaskReviewMetadata::default();
    task.updated_at = now;
    Ok(LifecycleTransition {
        task: &*task,
        warnings,
    })
}

/// Tasks already owned by an assignee, grouped by lifecycle state so agents can resume
/// existing work before claiming something new.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MyWork<'a> {
    pub assignee: String,
    pub active: Vec<&'a Task>,
    pub stale: Vec<&'a Task>,
    pub paused: Vec<&'a Task>,
    pub in_review: Vec<&'a Task>,
}

/// Group board-order tasks owned by `assignee` into resume-focused buckets.
pub fn get_my_work<'a>(board: &'a Board, assignee: impl Into<String>, now: u64) -> MyWork<'a> {
    let assignee = assignee.into();
    let mut active = Vec::new();
    let mut stale = Vec::new();
    let mut paused = Vec::new();
    let mut in_review = Vec::new();

    for task in board
        .tasks
        .iter()
        .filter(|task| task.assignee.as_deref() == Some(assignee.as_str()))
    {
        if task.status == TaskStatus::InReview {
            in_review.push(task);
        }
        if task.paused.is_some() {
            paused.push(task);
        } else if task.assignee.is_some() && task_is_stale(task, now) {
            stale.push(task);
        } else if task.status == TaskStatus::InProgress && has_fresh_active_lease(task, now) {
            active.push(task);
        }
    }

    MyWork {
        assignee,
        active,
        stale,
        paused,
        in_review,
    }
}

/// Select the first board-order To Do task that can be claimed automatically.
///
/// Blocked tasks and tasks with a fresh active lease are skipped. Stale or paused leases
/// are intentionally eligible so [`start_work`] can emit takeover warnings while applying
/// the normal lifecycle metadata.
pub fn next_available_work(board: &Board, now: u64) -> Option<&Task> {
    board.tasks.iter().find(|task| {
        task.status == TaskStatus::Todo
            && task.blocked.is_none()
            && !has_fresh_active_lease(task, now)
    })
}

/// Atomically claim the next available task in board order within an already-loaded board.
pub fn start_next_work(
    board: &mut Board,
    assignee: impl Into<String>,
    stale_after_secs: Option<u64>,
) -> Result<Option<LifecycleTransition<Task>>, BoardError> {
    let assignee = assignee.into();
    let now = now_epoch_secs();
    let Some(task_id) = next_available_work(board, now).map(|task| task.id.clone()) else {
        return Ok(None);
    };
    let transition = start_work(board, &task_id, assignee, stale_after_secs, false)?;
    Ok(Some(LifecycleTransition {
        task: transition.task.clone(),
        warnings: transition.warnings,
    }))
}

/// Refresh active-work heartbeat, optionally appending a progress note.
pub fn heartbeat_task<'a>(
    board: &'a mut Board,
    id: &str,
    assignee: impl Into<String>,
    note: Option<String>,
) -> Result<&'a Task, BoardError> {
    let assignee = assignee.into();
    let now = now_epoch_secs();
    let task = task_mut(board, id)?;
    guard_active_owner(task, &assignee, now, false, &mut Vec::new())?;
    task.assignee = Some(assignee.clone());
    if task.claimed_at.is_none() {
        task.claimed_at = Some(now);
    }
    task.heartbeat_at = Some(now);
    task.stale_after_secs
        .get_or_insert(DEFAULT_STALE_AFTER_SECS);
    task.paused = None;
    task.updated_at = now;
    if let Some(text) = note.filter(|text| !text.trim().is_empty()) {
        task.notes.push(TaskNote {
            text,
            author: Some(assignee),
            created_at: now,
        });
    }
    Ok(&*task)
}

/// Mark owned work as paused without moving columns or clearing the assignee.
pub fn pause_work<'a>(
    board: &'a mut Board,
    id: &str,
    assignee: impl Into<String>,
    reason: Option<String>,
) -> Result<&'a Task, BoardError> {
    let assignee = assignee.into();
    let now = now_epoch_secs();
    let task = task_mut(board, id)?;
    guard_active_owner(task, &assignee, now, false, &mut Vec::new())?;
    task.assignee = Some(assignee.clone());
    task.paused = Some(TaskPausedMetadata {
        reason: reason.filter(|text| !text.trim().is_empty()),
        author: Some(assignee),
        paused_at: now,
    });
    task.heartbeat_at = Some(now);
    task.updated_at = now;
    Ok(&*task)
}

/// Clear active ownership and lease metadata with an optional release note.
pub fn release_task<'a>(
    board: &'a mut Board,
    id: &str,
    assignee: impl Into<String>,
    reason: Option<String>,
    force: bool,
) -> Result<&'a Task, BoardError> {
    let assignee = assignee.into();
    let now = now_epoch_secs();
    let task = task_mut(board, id)?;
    guard_active_owner(task, &assignee, now, force, &mut Vec::new())?;
    if let Some(text) = reason.filter(|text| !text.trim().is_empty()) {
        task.notes.push(TaskNote {
            text,
            author: Some(assignee),
            created_at: now,
        });
    }
    clear_active_lifecycle(task);
    task.updated_at = now;
    Ok(&*task)
}

/// Transfer ownership. Fresh active leases owned by someone else require `force`.
pub fn reassign_task<'a>(
    board: &'a mut Board,
    id: &str,
    assignee: impl Into<String>,
    force: bool,
) -> Result<LifecycleTransition<&'a Task>, BoardError> {
    let assignee = assignee.into();
    let now = now_epoch_secs();
    let task = task_mut(board, id)?;
    let mut warnings = Vec::new();
    guard_active_owner(task, &assignee, now, force, &mut warnings)?;
    task.assignee = Some(assignee);
    task.claimed_at = Some(now);
    task.heartbeat_at = Some(now);
    task.stale_after_secs
        .get_or_insert(DEFAULT_STALE_AFTER_SECS);
    task.paused = None;
    task.updated_at = now;
    Ok(LifecycleTransition {
        task: &*task,
        warnings,
    })
}

/// Add machine-readable blocker metadata without changing columns.
pub fn block_task<'a>(
    board: &'a mut Board,
    id: &str,
    reason: impl Into<String>,
    category: Option<String>,
    author: Option<String>,
) -> Result<&'a Task, BoardError> {
    let now = now_epoch_secs();
    let task = task_mut(board, id)?;
    task.blocked = Some(TaskBlockedMetadata {
        reason: reason.into(),
        category: category.filter(|text| !text.trim().is_empty()),
        author: author.filter(|text| !text.trim().is_empty()),
        blocked_at: now,
    });
    task.updated_at = now;
    Ok(&*task)
}

/// Clear blocker metadata, optionally appending an unblock note.
pub fn unblock_task<'a>(
    board: &'a mut Board,
    id: &str,
    note: Option<String>,
    author: Option<String>,
) -> Result<&'a Task, BoardError> {
    let now = now_epoch_secs();
    let task = task_mut(board, id)?;
    task.blocked = None;
    if let Some(text) = note.filter(|text| !text.trim().is_empty()) {
        task.notes.push(TaskNote {
            text,
            author,
            created_at: now,
        });
    }
    task.updated_at = now;
    Ok(&*task)
}

/// Move owned work to review, append a handoff summary, and clear active lifecycle state.
#[allow(dead_code)]
pub fn ready_for_review<'a>(
    board: &'a mut Board,
    id: &str,
    summary: impl Into<String>,
    author: Option<String>,
) -> Result<&'a Task, BoardError> {
    let now = now_epoch_secs();
    let task = task_mut(board, id)?;
    let summary = summary.into();
    if !summary.trim().is_empty() {
        task.notes.push(TaskNote {
            text: summary,
            author,
            created_at: now,
        });
    }
    task.status = TaskStatus::InReview;
    clear_active_lease(task);
    task.updated_at = now;
    Ok(&*task)
}

/// Move owned work to review, appending a handoff summary and rejecting fresh
/// active leases owned by another assignee unless `force` is explicit.
pub fn ready_for_review_as<'a>(
    board: &'a mut Board,
    id: &str,
    summary: impl Into<String>,
    author: Option<String>,
    force: bool,
) -> Result<LifecycleTransition<&'a Task>, BoardError> {
    let now = now_epoch_secs();
    let task = task_mut(board, id)?;
    let actor = author.as_deref().unwrap_or("agent");
    let mut warnings = Vec::new();
    guard_active_owner(task, actor, now, force, &mut warnings)?;
    let summary = summary.into();
    if !summary.trim().is_empty() {
        task.notes.push(TaskNote {
            text: summary,
            author,
            created_at: now,
        });
    }
    task.status = TaskStatus::InReview;
    clear_active_lease(task);
    task.updated_at = now;
    Ok(LifecycleTransition {
        task: &*task,
        warnings,
    })
}

/// Append a structured review note/verdict while leaving the task in In Review.
pub fn submit_review<'a>(
    board: &'a mut Board,
    id: &str,
    verdict: impl Into<String>,
    summary: impl Into<String>,
    author: Option<String>,
) -> Result<&'a Task, BoardError> {
    let now = now_epoch_secs();
    let task = task_mut(board, id)?;
    task.status = TaskStatus::InReview;
    let verdict = verdict.into();
    let summary = summary.into();
    let text = if summary.trim().is_empty() {
        format!("Review verdict: {verdict}")
    } else {
        format!("Review verdict: {verdict}\n\n{summary}")
    };
    task.notes.push(TaskNote {
        text,
        author,
        created_at: now,
    });
    task.updated_at = now;
    Ok(&*task)
}

/// Mark a task complete, optionally appending a closing progress note.
#[allow(dead_code)]
pub fn complete_task<'a>(
    board: &'a mut Board,
    id: &str,
    note: Option<String>,
) -> Result<&'a Task, BoardError> {
    let task = task_mut(board, id)?;
    task.status = TaskStatus::Complete;
    let now = now_epoch_secs();
    task.updated_at = now;
    let author = task.assignee.clone();
    clear_active_lifecycle(task);
    if let Some(text) = note.filter(|text| !text.trim().is_empty()) {
        task.notes.push(TaskNote {
            text,
            author,
            created_at: now,
        });
    }
    Ok(&*task)
}

/// Mark owned work complete. If another assignee has a fresh active lease, callers must
/// either identify as that assignee or set `force`.
pub fn complete_task_as<'a>(
    board: &'a mut Board,
    id: &str,
    note: Option<String>,
    author: Option<String>,
    force: bool,
) -> Result<LifecycleTransition<&'a Task>, BoardError> {
    let task = task_mut(board, id)?;
    let now = now_epoch_secs();
    let actor = author.as_deref().unwrap_or("agent");
    let mut warnings = Vec::new();
    guard_active_owner(task, actor, now, force, &mut warnings)?;
    task.status = TaskStatus::Complete;
    task.updated_at = now;
    let note_author = author.or_else(|| task.assignee.clone());
    clear_active_lifecycle(task);
    if let Some(text) = note.filter(|text| !text.trim().is_empty()) {
        task.notes.push(TaskNote {
            text,
            author: note_author,
            created_at: now,
        });
    }
    Ok(LifecycleTransition {
        task: &*task,
        warnings,
    })
}

/// Move a task to a new column on behalf of an MCP/agent caller. Fresh active leases
/// owned by someone else require `force`.
pub fn set_status_as<'a>(
    board: &'a mut Board,
    id: &str,
    status: TaskStatus,
    actor: impl Into<String>,
    force: bool,
) -> Result<LifecycleTransition<&'a Task>, BoardError> {
    let actor = actor.into();
    let now = now_epoch_secs();
    let task = task_mut(board, id)?;
    let mut warnings = Vec::new();
    guard_active_owner(task, &actor, now, force, &mut warnings)?;
    task.status = status;
    if matches!(status, TaskStatus::Todo | TaskStatus::InProgress) {
        task.review = TaskReviewMetadata::default();
    }
    if status == TaskStatus::InReview {
        clear_active_lease(task);
    }
    if matches!(
        status,
        TaskStatus::Todo | TaskStatus::Complete | TaskStatus::Cancelled
    ) {
        clear_active_lifecycle(task);
    }
    task.updated_at = now;
    Ok(LifecycleTransition {
        task: &*task,
        warnings,
    })
}

/// Record that a code review was dispatched for a task. The task remains/enters In Review.
pub fn start_review<'a>(
    board: &'a mut Board,
    id: &str,
    reviewer: AgentKind,
) -> Result<&'a Task, BoardError> {
    let task = task_mut(board, id)?;
    let now = now_epoch_secs();
    task.status = TaskStatus::InReview;
    task.review.last_started_at = Some(now);
    task.review.last_reviewer = Some(reviewer);
    task.review.last_error = None;
    task.review.attempts = task.review.attempts.saturating_add(1);
    task.updated_at = now;
    Ok(&*task)
}

/// Record that a previously claimed review could not be launched without changing the
/// task's current column. The failure remains visible in metadata and notes for
/// diagnosis/retry even if another surface has already moved the task.
pub fn record_review_error<'a>(
    board: &'a mut Board,
    id: &str,
    reviewer: AgentKind,
    error: impl Into<String>,
) -> Result<&'a Task, BoardError> {
    let now = now_epoch_secs();
    let task = task_mut(board, id)?;
    let error = error.into();
    task.review.last_reviewer = Some(reviewer);
    task.review.last_error = Some(error.clone());
    task.notes.push(TaskNote {
        text: format!("Review launch failed for {}: {error}", reviewer.label()),
        author: Some("terminaltiler".to_string()),
        created_at: now,
    });
    task.updated_at = now;
    Ok(&*task)
}

/// Resolve the reviewer for a task: recognized assignee first, then board default.
pub fn reviewer_for_task(board: &Board, task: &Task) -> AgentKind {
    task.assignee
        .as_deref()
        .and_then(AgentKind::from_assignee_id)
        .or(board.automation.default_reviewer)
        .or(board.automation.default_agent)
        .unwrap_or(AgentKind::Claude)
}

/// Resolve the implementation agent for a task: recognized assignee first, then
/// implementation default, reviewer default, and finally Claude as the hard fallback.
pub fn implementation_agent_for_task(board: &Board, task: &Task) -> AgentKind {
    task.assignee
        .as_deref()
        .and_then(AgentKind::from_assignee_id)
        .or(board.automation.default_agent)
        .or(board.automation.default_reviewer)
        .unwrap_or(AgentKind::Claude)
}

/// Append a progress note to a task.
pub fn add_note<'a>(
    board: &'a mut Board,
    id: &str,
    text: impl Into<String>,
    author: Option<String>,
) -> Result<&'a Task, BoardError> {
    let now = now_epoch_secs();
    let task = task_mut(board, id)?;
    task.notes.push(TaskNote {
        text: text.into(),
        author,
        created_at: now,
    });
    task.updated_at = now;
    Ok(&*task)
}

/// Set (or clear, when blank) the additional instructions for a task.
pub fn set_additional_instructions<'a>(
    board: &'a mut Board,
    id: &str,
    instructions: impl Into<String>,
) -> Result<&'a Task, BoardError> {
    let instructions = instructions.into();
    let task = task_mut(board, id)?;
    let trimmed = instructions.trim();
    task.additional_instructions = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    };
    task.updated_at = now_epoch_secs();
    Ok(&*task)
}

/// Append a captured knowledge entry to a task.
#[allow(clippy::too_many_arguments)]
pub fn add_knowledge<'a>(
    board: &'a mut Board,
    id: &str,
    title: impl Into<String>,
    content: impl Into<String>,
    source: Option<String>,
    category: Option<String>,
    author: Option<String>,
) -> Result<&'a Task, BoardError> {
    let now = now_epoch_secs();
    let task = task_mut(board, id)?;
    task.knowledge.push(KnowledgeEntry {
        title: title.into(),
        content: content.into(),
        source,
        category,
        author,
        created_at: now,
    });
    task.updated_at = now;
    Ok(&*task)
}

/// Attach an already-copied file to a task. The caller is responsible for placing the file
/// on disk (see the UI attachment helper); this only records the metadata.
pub fn add_attachment<'a>(
    board: &'a mut Board,
    id: &str,
    attachment: TaskAttachment,
) -> Result<&'a Task, BoardError> {
    let task = task_mut(board, id)?;
    task.attachments.push(attachment);
    task.updated_at = now_epoch_secs();
    Ok(&*task)
}

/// Remove an attachment (matched by relative path) from a task. Returns the removed entry so
/// the caller can delete the backing file. Errors if the task is missing; returns `Ok(None)`
/// when the task exists but has no attachment at that path.
pub fn remove_attachment(
    board: &mut Board,
    id: &str,
    path: &str,
) -> Result<Option<TaskAttachment>, BoardError> {
    let task = task_mut(board, id)?;
    let Some(index) = task
        .attachments
        .iter()
        .position(|attachment| attachment.path == path)
    else {
        return Ok(None);
    };
    let removed = task.attachments.remove(index);
    task.updated_at = now_epoch_secs();
    Ok(Some(removed))
}

/// Remove a task from the board.
pub fn delete_task(board: &mut Board, id: &str) -> Result<(), BoardError> {
    let index = index_of(board, id).ok_or_else(|| BoardError::TaskNotFound(id.to_string()))?;
    board.tasks.remove(index);
    Ok(())
}

fn index_of(board: &Board, id: &str) -> Option<usize> {
    board.tasks.iter().position(|task| task.id == id)
}

fn task_mut<'a>(board: &'a mut Board, id: &str) -> Result<&'a mut Task, BoardError> {
    board
        .tasks
        .iter_mut()
        .find(|task| task.id == id)
        .ok_or_else(|| BoardError::TaskNotFound(id.to_string()))
}

/// Lifecycle mutation result, including warnings for stale/paused takeovers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleTransition<T> {
    pub task: T,
    pub warnings: Vec<String>,
}

/// Whether a task's active soft lease is stale at `now`.
pub fn task_is_stale(task: &Task, now: u64) -> bool {
    let Some(anchor) = task.heartbeat_at.or(task.claimed_at) else {
        return false;
    };
    now.saturating_sub(anchor) > task.stale_after_secs.unwrap_or(DEFAULT_STALE_AFTER_SECS)
}

/// Human-readable lifecycle labels used by light UI indicators.
pub fn lifecycle_indicators(task: &Task, now: u64) -> Vec<&'static str> {
    let mut indicators = Vec::new();
    if task.blocked.is_some() {
        indicators.push("blocked");
    }
    if task.paused.is_some() {
        indicators.push("paused");
    } else if task.assignee.is_some() && task_is_stale(task, now) {
        indicators.push("stale");
    } else if task.assignee.is_some() && task.heartbeat_at.or(task.claimed_at).is_some() {
        indicators.push("active");
    }
    indicators
}

/// Whether a task carries a fresh active lease at `now`.
pub fn has_fresh_active_lease(task: &Task, now: u64) -> bool {
    has_active_lease(task) && task.paused.is_none() && !task_is_stale(task, now)
}

fn guard_active_owner(
    task: &Task,
    requested_assignee: &str,
    now: u64,
    force: bool,
    warnings: &mut Vec<String>,
) -> Result<(), BoardError> {
    let Some(current_assignee) = task.assignee.as_deref() else {
        return Ok(());
    };
    if current_assignee == requested_assignee {
        return Ok(());
    }
    if task.paused.is_some() {
        warnings.push(format!("took over paused task from '{current_assignee}'"));
        return Ok(());
    }
    if !has_active_lease(task) {
        return Ok(());
    }
    if task_is_stale(task, now) {
        warnings.push(format!("took over stale task from '{current_assignee}'"));
        return Ok(());
    }
    if force {
        warnings.push(format!(
            "force took over fresh active task from '{current_assignee}'"
        ));
        return Ok(());
    }
    Err(BoardError::OwnershipConflict(TaskOwnershipConflict {
        task_id: task.id.clone(),
        current_assignee: current_assignee.to_string(),
        requested_assignee: requested_assignee.to_string(),
        heartbeat_at: task.heartbeat_at,
        claimed_at: task.claimed_at,
        stale_after_secs: task.stale_after_secs.unwrap_or(DEFAULT_STALE_AFTER_SECS),
        now,
    }))
}

fn has_active_lease(task: &Task) -> bool {
    task.heartbeat_at.or(task.claimed_at).is_some()
}

fn clear_active_lifecycle(task: &mut Task) {
    task.assignee = None;
    clear_active_lease(task);
}

fn clear_active_lease(task: &mut Task) {
    task.claimed_at = None;
    task.heartbeat_at = None;
    task.stale_after_secs = None;
    task.paused = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_then_transition_through_columns() {
        let mut board = Board::default();
        let id = create_task(&mut board, "Build feature", "details", TaskStatus::Todo)
            .id
            .clone();

        let claimed = claim_task(&mut board, &id, "claude").unwrap();
        assert_eq!(claimed.status, TaskStatus::InProgress);
        assert_eq!(claimed.assignee.as_deref(), Some("claude"));

        set_status(&mut board, &id, TaskStatus::InReview).unwrap();
        assert_eq!(get_task(&board, &id).unwrap().status, TaskStatus::InReview);

        let done = complete_task(&mut board, &id, Some("shipped".into())).unwrap();
        assert_eq!(done.status, TaskStatus::Complete);
        assert_eq!(done.latest_note(), Some("shipped"));
        assert_eq!(done.notes[0].author.as_deref(), Some("claude"));
    }

    #[test]
    fn review_metadata_gates_auto_review_and_manual_retry() {
        let mut board = Board::default();
        let id = create_task(&mut board, "Review", "", TaskStatus::Todo)
            .id
            .clone();

        set_status(&mut board, &id, TaskStatus::InReview).unwrap();
        assert!(get_task(&board, &id).unwrap().needs_auto_review());

        let reviewed = start_review(&mut board, &id, AgentKind::Codex).unwrap();
        assert_eq!(reviewed.status, TaskStatus::InReview);
        assert!(!reviewed.needs_auto_review());
        assert_eq!(reviewed.review.last_reviewer, Some(AgentKind::Codex));
        assert_eq!(reviewed.review.attempts, 1);

        let retried = start_review(&mut board, &id, AgentKind::Claude).unwrap();
        assert_eq!(retried.review.last_reviewer, Some(AgentKind::Claude));
        assert_eq!(retried.review.attempts, 2);

        claim_task(&mut board, &id, "claude").unwrap();
        assert!(get_task(&board, &id).unwrap().review.is_default());
    }

    #[test]
    fn review_error_preserves_concurrent_status_transition() {
        let mut board = Board::default();
        let id = create_task(&mut board, "Review", "", TaskStatus::Todo)
            .id
            .clone();

        start_review(&mut board, &id, AgentKind::Codex).unwrap();
        set_status(&mut board, &id, TaskStatus::Complete).unwrap();

        let recorded =
            record_review_error(&mut board, &id, AgentKind::Codex, "spawn failed").unwrap();
        assert_eq!(recorded.status, TaskStatus::Complete);
        assert_eq!(recorded.review.last_reviewer, Some(AgentKind::Codex));
        assert_eq!(recorded.review.last_error.as_deref(), Some("spawn failed"));
        assert_eq!(
            recorded.latest_note(),
            Some("Review launch failed for Codex: spawn failed")
        );
    }

    #[test]
    fn reviewer_prefers_recognized_assignee_then_board_default() {
        let mut board = Board::default();
        board.automation.default_reviewer = Some(AgentKind::Claude);
        let id = create_task(&mut board, "Review", "", TaskStatus::InReview)
            .id
            .clone();

        assert_eq!(
            reviewer_for_task(&board, get_task(&board, &id).unwrap()),
            AgentKind::Claude
        );

        board.tasks[0].assignee = Some("codex".into());
        assert_eq!(
            reviewer_for_task(&board, get_task(&board, &id).unwrap()),
            AgentKind::Codex
        );
    }

    #[test]
    fn implementation_agent_prefers_assignee_then_agent_then_reviewer_then_claude() {
        let mut board = Board::default();
        board.automation.default_agent = Some(AgentKind::Codex);
        board.automation.default_reviewer = Some(AgentKind::Claude);
        let id = create_task(&mut board, "Implement", "", TaskStatus::Todo)
            .id
            .clone();

        assert_eq!(
            implementation_agent_for_task(&board, get_task(&board, &id).unwrap()),
            AgentKind::Codex
        );

        board.tasks[0].assignee = Some("claude".into());
        assert_eq!(
            implementation_agent_for_task(&board, get_task(&board, &id).unwrap()),
            AgentKind::Claude
        );

        board.tasks[0].assignee = Some("unknown".into());
        board.automation.default_agent = None;
        board.automation.default_reviewer = Some(AgentKind::Codex);
        assert_eq!(
            implementation_agent_for_task(&board, get_task(&board, &id).unwrap()),
            AgentKind::Codex
        );

        board.automation.default_reviewer = None;
        assert_eq!(
            implementation_agent_for_task(&board, get_task(&board, &id).unwrap()),
            AgentKind::Claude
        );
    }

    #[test]
    fn tasks_by_status_filters_columns() {
        let mut board = Board::default();
        create_task(&mut board, "a", "", TaskStatus::Todo);
        create_task(&mut board, "b", "", TaskStatus::Todo);
        create_task(&mut board, "c", "", TaskStatus::Complete);

        assert_eq!(tasks_by_status(&board, TaskStatus::Todo).len(), 2);
        assert_eq!(tasks_by_status(&board, TaskStatus::Complete).len(), 1);
        assert_eq!(tasks_by_status(&board, TaskStatus::Cancelled).len(), 0);
    }

    #[test]
    fn missing_task_is_reported() {
        let mut board = Board::default();
        let error = set_status(&mut board, "ghost", TaskStatus::Complete).unwrap_err();
        assert_eq!(error, BoardError::TaskNotFound("ghost".into()));
        assert!(delete_task(&mut board, "ghost").is_err());
    }

    #[test]
    fn instructions_knowledge_and_attachments_round_trip() {
        let mut board = Board::default();
        let id = create_task(&mut board, "Task", "desc", TaskStatus::Todo)
            .id
            .clone();

        let task = set_additional_instructions(&mut board, &id, "  use bunny CDN  ").unwrap();
        assert_eq!(
            task.additional_instructions.as_deref(),
            Some("use bunny CDN")
        );
        assert!(task.has_instructions());

        // Blank input clears it.
        let task = set_additional_instructions(&mut board, &id, "   ").unwrap();
        assert_eq!(task.additional_instructions, None);

        let task = add_knowledge(
            &mut board,
            &id,
            "Bunny CDN base url",
            "https://docs.bunny.net/cdn",
            Some("agent".into()),
            Some("api_ref".into()),
            Some("claude".into()),
        )
        .unwrap();
        assert_eq!(task.knowledge.len(), 1);
        assert_eq!(task.knowledge[0].title, "Bunny CDN base url");
        assert_eq!(task.knowledge[0].category.as_deref(), Some("api_ref"));

        let attachment = TaskAttachment {
            path: format!(".terminaltiler/attachments/{id}/shot.png"),
            name: "shot.png".into(),
            mime_type: Some("image/png".into()),
            size_bytes: 1234,
            added_at: 0,
        };
        let task = add_attachment(&mut board, &id, attachment.clone()).unwrap();
        assert_eq!(task.attachments.len(), 1);

        let removed = remove_attachment(&mut board, &id, &attachment.path).unwrap();
        assert_eq!(removed, Some(attachment));
        assert!(get_task(&board, &id).unwrap().attachments.is_empty());

        // Removing a missing path is a no-op, not an error.
        assert_eq!(remove_attachment(&mut board, &id, "nope").unwrap(), None);
    }

    #[test]
    fn new_task_ops_report_missing_task() {
        let mut board = Board::default();
        assert!(set_additional_instructions(&mut board, "ghost", "x").is_err());
        assert!(add_knowledge(&mut board, "ghost", "t", "c", None, None, None).is_err());
        assert!(remove_attachment(&mut board, "ghost", "p").is_err());
    }

    #[test]
    fn blank_completion_note_is_ignored() {
        let mut board = Board::default();
        let id = create_task(&mut board, "task", "", TaskStatus::Todo)
            .id
            .clone();
        let done = complete_task(&mut board, &id, Some("   ".into())).unwrap();
        assert!(done.notes.is_empty());
    }

    #[test]
    fn lifecycle_start_heartbeat_pause_release_and_block_round_trip() {
        let mut board = Board::default();
        let id = create_task(&mut board, "Lifecycle", "", TaskStatus::Todo)
            .id
            .clone();

        let started = start_work(&mut board, &id, "codex", Some(60), false).unwrap();
        assert!(started.warnings.is_empty());
        assert_eq!(started.task.status, TaskStatus::InProgress);
        assert_eq!(started.task.assignee.as_deref(), Some("codex"));
        assert_eq!(started.task.stale_after_secs, Some(60));
        assert!(started.task.claimed_at.is_some());
        assert!(started.task.heartbeat_at.is_some());

        let heartbeat =
            heartbeat_task(&mut board, &id, "codex", Some("still working".into())).unwrap();
        assert_eq!(heartbeat.notes.len(), 1);
        assert_eq!(heartbeat.paused, None);

        let paused = pause_work(&mut board, &id, "codex", Some("waiting".into())).unwrap();
        assert_eq!(
            paused
                .paused
                .as_ref()
                .and_then(|paused| paused.reason.as_deref()),
            Some("waiting")
        );
        assert_eq!(
            lifecycle_indicators(paused, now_epoch_secs()),
            vec!["paused"]
        );

        let blocked = block_task(
            &mut board,
            &id,
            "Need API key",
            Some("dependency".into()),
            Some("codex".into()),
        )
        .unwrap();
        assert_eq!(
            blocked.blocked.as_ref().unwrap().category.as_deref(),
            Some("dependency")
        );

        let unblocked = unblock_task(
            &mut board,
            &id,
            Some("API key received".into()),
            Some("codex".into()),
        )
        .unwrap();
        assert!(unblocked.blocked.is_none());
        assert_eq!(unblocked.notes.len(), 2);

        let released =
            release_task(&mut board, &id, "codex", Some("handing back".into()), false).unwrap();
        assert_eq!(released.assignee, None);
        assert_eq!(released.claimed_at, None);
        assert_eq!(released.heartbeat_at, None);
        assert_eq!(released.paused, None);
        assert_eq!(released.notes.len(), 3);
    }

    #[test]
    fn lifecycle_soft_lease_conflict_stale_and_force_takeover() {
        let mut board = Board::default();
        let id = create_task(&mut board, "Lease", "", TaskStatus::Todo)
            .id
            .clone();

        start_work(&mut board, &id, "alice", Some(60), false).unwrap();
        let conflict = start_work(&mut board, &id, "bob", Some(60), false).unwrap_err();
        assert!(matches!(conflict, BoardError::OwnershipConflict(_)));

        let task = board.tasks.iter_mut().find(|task| task.id == id).unwrap();
        task.heartbeat_at = Some(now_epoch_secs().saturating_sub(120));
        let stale_takeover = start_work(&mut board, &id, "bob", Some(60), false).unwrap();
        assert_eq!(stale_takeover.task.assignee.as_deref(), Some("bob"));
        assert_eq!(
            stale_takeover.warnings,
            vec!["took over stale task from 'alice'"]
        );

        let forced = reassign_task(&mut board, &id, "carol", true).unwrap();
        assert_eq!(forced.task.assignee.as_deref(), Some("carol"));
        assert_eq!(
            forced.warnings,
            vec!["force took over fresh active task from 'bob'"]
        );
    }

    #[test]
    fn my_work_groups_owned_active_stale_paused_and_review_tasks() {
        let mut board = Board::default();
        let active_id = create_task(&mut board, "Active", "", TaskStatus::Todo)
            .id
            .clone();
        let stale_id = create_task(&mut board, "Stale", "", TaskStatus::Todo)
            .id
            .clone();
        let paused_id = create_task(&mut board, "Paused", "", TaskStatus::Todo)
            .id
            .clone();
        let review_id = create_task(&mut board, "Review", "", TaskStatus::Todo)
            .id
            .clone();
        let other_id = create_task(&mut board, "Other", "", TaskStatus::Todo)
            .id
            .clone();

        start_work(&mut board, &active_id, "agent", Some(60), false).unwrap();
        start_work(&mut board, &stale_id, "agent", Some(60), false).unwrap();
        start_work(&mut board, &paused_id, "agent", Some(60), false).unwrap();
        pause_work(&mut board, &paused_id, "agent", Some("waiting".into())).unwrap();
        start_work(&mut board, &review_id, "agent", Some(60), false).unwrap();
        ready_for_review(&mut board, &review_id, "done", Some("agent".into())).unwrap();
        start_work(&mut board, &other_id, "someone-else", Some(60), false).unwrap();

        let now = now_epoch_secs();
        board
            .tasks
            .iter_mut()
            .find(|task| task.id == stale_id)
            .unwrap()
            .heartbeat_at = Some(now.saturating_sub(120));

        let work = get_my_work(&board, "agent", now);
        assert_eq!(work.assignee, "agent");
        assert_eq!(
            work.active
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec![active_id.as_str()]
        );
        assert_eq!(
            work.stale
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec![stale_id.as_str()]
        );
        assert_eq!(
            work.paused
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec![paused_id.as_str()]
        );
        assert_eq!(
            work.in_review
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec![review_id.as_str()]
        );
    }

    #[test]
    fn next_available_work_skips_blocked_and_fresh_leases_in_board_order() {
        let mut board = Board::default();
        let blocked_id = create_task(&mut board, "Blocked", "", TaskStatus::Todo)
            .id
            .clone();
        let leased_id = create_task(&mut board, "Leased", "", TaskStatus::Todo)
            .id
            .clone();
        let available_id = create_task(&mut board, "Available", "", TaskStatus::Todo)
            .id
            .clone();

        block_task(&mut board, &blocked_id, "blocked", None, None).unwrap();
        start_work(&mut board, &leased_id, "alice", Some(60), false).unwrap();
        board
            .tasks
            .iter_mut()
            .find(|task| task.id == leased_id)
            .unwrap()
            .status = TaskStatus::Todo;

        let selected = next_available_work(&board, now_epoch_secs()).unwrap();
        assert_eq!(selected.id, available_id);
    }

    #[test]
    fn start_next_work_claims_first_available_and_warns_on_stale_or_paused_takeover() {
        let mut board = Board::default();
        let stale_id = create_task(&mut board, "Stale takeover", "", TaskStatus::Todo)
            .id
            .clone();
        let paused_id = create_task(&mut board, "Paused takeover", "", TaskStatus::Todo)
            .id
            .clone();

        start_work(&mut board, &stale_id, "alice", Some(60), false).unwrap();
        board
            .tasks
            .iter_mut()
            .find(|task| task.id == stale_id)
            .unwrap()
            .status = TaskStatus::Todo;
        board
            .tasks
            .iter_mut()
            .find(|task| task.id == stale_id)
            .unwrap()
            .heartbeat_at = Some(now_epoch_secs().saturating_sub(120));

        start_work(&mut board, &paused_id, "carol", Some(60), false).unwrap();
        pause_work(&mut board, &paused_id, "carol", Some("paused".into())).unwrap();
        board
            .tasks
            .iter_mut()
            .find(|task| task.id == paused_id)
            .unwrap()
            .status = TaskStatus::Todo;

        let stale_claim = start_next_work(&mut board, "bob", Some(30))
            .unwrap()
            .expect("stale task claim");
        assert_eq!(stale_claim.task.id, stale_id);
        assert_eq!(stale_claim.task.assignee.as_deref(), Some("bob"));
        assert_eq!(stale_claim.task.stale_after_secs, Some(30));
        assert_eq!(
            stale_claim.warnings,
            vec!["took over stale task from 'alice'"]
        );

        let paused_claim = start_next_work(&mut board, "dana", None)
            .unwrap()
            .expect("paused task claim");
        assert_eq!(paused_claim.task.id, paused_id);
        assert_eq!(
            paused_claim.warnings,
            vec!["took over paused task from 'carol'"]
        );

        assert!(
            start_next_work(&mut board, "agent", None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn legacy_claim_blocks_fresh_takeover_without_force() {
        let mut board = Board::default();
        let id = create_task(&mut board, "Legacy claim guard", "", TaskStatus::Todo)
            .id
            .clone();

        claim_task(&mut board, &id, "alice").unwrap();
        let conflict = claim_task(&mut board, &id, "bob").unwrap_err();

        assert!(matches!(conflict, BoardError::OwnershipConflict(_)));
        assert_eq!(
            get_task(&board, &id).unwrap().assignee.as_deref(),
            Some("alice")
        );
    }

    #[test]
    fn owned_review_and_completion_block_non_owner_but_allow_force_and_stale() {
        let mut board = Board::default();
        let id = create_task(&mut board, "Owned transitions", "", TaskStatus::Todo)
            .id
            .clone();

        start_work(&mut board, &id, "alice", Some(60), false).unwrap();
        let review_conflict =
            ready_for_review_as(&mut board, &id, "done", Some("bob".into()), false).unwrap_err();
        assert!(matches!(review_conflict, BoardError::OwnershipConflict(_)));

        let complete_conflict = complete_task_as(
            &mut board,
            &id,
            Some("done".into()),
            Some("bob".into()),
            false,
        )
        .unwrap_err();
        assert!(matches!(
            complete_conflict,
            BoardError::OwnershipConflict(_)
        ));

        let task = board.tasks.iter_mut().find(|task| task.id == id).unwrap();
        task.heartbeat_at = Some(now_epoch_secs().saturating_sub(120));
        let stale_handoff =
            ready_for_review_as(&mut board, &id, "handoff", Some("bob".into()), false).unwrap();
        assert_eq!(stale_handoff.task.status, TaskStatus::InReview);
        assert_eq!(
            stale_handoff.warnings,
            vec!["took over stale task from 'alice'"]
        );

        start_work(&mut board, &id, "alice", Some(60), false).unwrap();
        let forced = complete_task_as(
            &mut board,
            &id,
            Some("ship".into()),
            Some("bob".into()),
            true,
        )
        .unwrap();
        assert_eq!(forced.task.status, TaskStatus::Complete);
        assert_eq!(
            forced.warnings,
            vec!["force took over fresh active task from 'alice'"]
        );
    }

    #[test]
    fn ready_for_review_clears_active_lifecycle_and_submit_review_keeps_review_column() {
        let mut board = Board::default();
        let id = create_task(&mut board, "Review handoff", "", TaskStatus::Todo)
            .id
            .clone();
        start_work(&mut board, &id, "codex", None, false).unwrap();

        let ready = ready_for_review(
            &mut board,
            &id,
            "Implemented and tested",
            Some("codex".into()),
        )
        .unwrap();
        assert_eq!(ready.status, TaskStatus::InReview);
        assert_eq!(ready.assignee.as_deref(), Some("codex"));
        assert_eq!(ready.claimed_at, None);
        assert_eq!(ready.heartbeat_at, None);
        assert_eq!(ready.stale_after_secs, None);
        assert_eq!(ready.paused, None);

        let reviewed = submit_review(
            &mut board,
            &id,
            "changes_requested",
            "Fix missing edge case",
            Some("codex-reviewer".into()),
        )
        .unwrap();
        assert_eq!(reviewed.status, TaskStatus::InReview);
        assert!(
            reviewed
                .latest_note()
                .unwrap()
                .contains("changes_requested")
        );
    }

    #[test]
    fn legacy_status_handoff_to_review_clears_active_lease_but_keeps_assignee() {
        let mut board = Board::default();
        let id = create_task(&mut board, "Legacy review handoff", "", TaskStatus::Todo)
            .id
            .clone();
        claim_task(&mut board, &id, "claude").unwrap();

        let reviewed = set_status(&mut board, &id, TaskStatus::InReview).unwrap();
        assert_eq!(reviewed.status, TaskStatus::InReview);
        assert_eq!(reviewed.assignee.as_deref(), Some("claude"));
        assert_eq!(reviewed.claimed_at, None);
        assert_eq!(reviewed.heartbeat_at, None);
        assert_eq!(reviewed.stale_after_secs, None);
        assert_eq!(reviewed.paused, None);

        let restarted = start_work(&mut board, &id, "codex", None, false).unwrap();
        assert_eq!(restarted.task.assignee.as_deref(), Some("codex"));
        assert!(restarted.warnings.is_empty());
    }
}
