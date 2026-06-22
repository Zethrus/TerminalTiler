//! Board operations shared by the GTK UI and the `terminaltiler-mcp` server.
//!
//! These are pure functions over an in-memory [`Board`] — no GTK, no disk I/O — so there
//! is exactly one implementation of "claim a task", "complete a task", etc. for both
//! front-ends. Persistence is the caller's job (see `storage::board_store`).

use uuid::Uuid;

use crate::model::agent_run::AgentKind;
use crate::model::board::{Board, Task, TaskNote, TaskReviewMetadata, TaskStatus, now_epoch_secs};

/// Errors a board mutation can produce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardError {
    /// No task with the given id exists on the board.
    TaskNotFound(String),
}

impl std::fmt::Display for BoardError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BoardError::TaskNotFound(id) => write!(formatter, "no task with id '{id}'"),
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
    task.updated_at = now_epoch_secs();
    Ok(&*task)
}

/// Claim a task for an agent: move it to In Progress and record the assignee.
pub fn claim_task<'a>(
    board: &'a mut Board,
    id: &str,
    assignee: impl Into<String>,
) -> Result<&'a Task, BoardError> {
    let task = task_mut(board, id)?;
    task.status = TaskStatus::InProgress;
    task.assignee = Some(assignee.into());
    task.review = TaskReviewMetadata::default();
    task.updated_at = now_epoch_secs();
    Ok(&*task)
}

/// Mark a task complete, optionally appending a closing progress note.
pub fn complete_task<'a>(
    board: &'a mut Board,
    id: &str,
    note: Option<String>,
) -> Result<&'a Task, BoardError> {
    let task = task_mut(board, id)?;
    task.status = TaskStatus::Complete;
    let now = now_epoch_secs();
    task.updated_at = now;
    if let Some(text) = note.filter(|text| !text.trim().is_empty()) {
        task.notes.push(TaskNote {
            text,
            author: task.assignee.clone(),
            created_at: now,
        });
    }
    Ok(&*task)
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
    task.review.attempts = task.review.attempts.saturating_add(1);
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
    fn blank_completion_note_is_ignored() {
        let mut board = Board::default();
        let id = create_task(&mut board, "task", "", TaskStatus::Todo)
            .id
            .clone();
        let done = complete_task(&mut board, &id, Some("   ".into())).unwrap();
        assert!(done.notes.is_empty());
    }
}
