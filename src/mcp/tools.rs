//! MCP tool registry for the Kanban board.
//!
//! Each tool loads the per-project board, applies one `services::board` operation, saves
//! atomically, and returns a short text result. Tools operate on the board at
//! `project_root` (the agent's working directory).

use std::path::Path;

use serde_json::{Value, json};

use crate::model::board::{Task, TaskStatus, now_epoch_secs};
use crate::services::{board as board_service, review_dispatch};
use crate::storage::board_store;

/// Successful MCP tool output. `text` keeps backwards-compatible human-readable content;
/// `structured` is surfaced as MCP `structuredContent` for clients that support it.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCallOutput {
    pub text: String,
    pub structured: Option<Value>,
}

/// MCP tool error. Tool errors remain JSON-RPC successes with `isError: true`.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCallError {
    pub text: String,
    pub structured: Option<Value>,
}

/// Tool definitions advertised by `tools/list`.
pub fn list_json() -> Vec<Value> {
    let status_enum: Vec<&str> = TaskStatus::ALL
        .iter()
        .map(|status| status.wire_id())
        .collect();

    vec![
        tool(
            "list_tasks",
            "List board tasks, optionally filtered to a single column (status).",
            json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string", "enum": status_enum, "description": "Optional column filter." }
                }
            }),
        ),
        tool(
            "get_task",
            "Get the full details of one task by id.",
            json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }),
        ),
        tool(
            "create_task",
            "Create a new task. Defaults to the To Do column.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "description": { "type": "string" },
                    "status": { "type": "string", "enum": status_enum.clone() }
                },
                "required": ["title"]
            }),
        ),
        tool(
            "claim_task",
            "Claim a task: move it to In Progress and record the assignee. Call this before starting work.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "assignee": { "type": "string", "description": "Defaults to 'agent'." }
                },
                "required": ["id"]
            }),
        ),
        tool(
            "update_task_status",
            "Move a task to a different column. Move implementation-ready work to in_review before completion so the board can review it.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "status": { "type": "string", "enum": status_enum.clone() }
                },
                "required": ["id", "status"]
            }),
        ),
        tool(
            "complete_task",
            "Mark a task Complete, optionally with a closing note. Prefer in_review first unless the user explicitly asked to complete it.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "note": { "type": "string" }
                },
                "required": ["id"]
            }),
        ),
        tool(
            "add_task_note",
            "Append a progress note to a task so the user can follow along.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "text": { "type": "string" },
                    "author": { "type": "string" }
                },
                "required": ["id", "text"]
            }),
        ),
        tool(
            "add_task_knowledge",
            "Record a captured knowledge entry on a task (docs, API references, examples, blockers) so it accrues for later work and the user can review it.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "title": { "type": "string", "description": "Short label for the finding." },
                    "content": { "type": "string", "description": "The detail of the finding." },
                    "source": { "type": "string", "description": "Origin, e.g. 'agent' (default) or 'user'." },
                    "category": { "type": "string", "description": "Optional tag, e.g. 'api_ref', 'example', 'blocker'." },
                    "author": { "type": "string" }
                },
                "required": ["id", "title", "content"]
            }),
        ),
        tool_with_output(
            "start_work",
            "Claim or resume a task with soft-lease conflict checks. Prefer this over claim_task for agent lifecycle-aware work.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "assignee": { "type": "string", "description": "Defaults to 'agent'." },
                    "stale_after_secs": { "type": "integer", "minimum": 1, "description": "Override soft-lease stale threshold; default is 21600 (6 hours)." },
                    "force": { "type": "boolean", "description": "Take over a fresh active lease intentionally." }
                },
                "required": ["id"]
            }),
            lifecycle_output_schema(),
        ),
        tool_with_output(
            "heartbeat_task",
            "Refresh active work timestamp, optionally appending a progress note.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "assignee": { "type": "string", "description": "Defaults to 'agent'." },
                    "note": { "type": "string" }
                },
                "required": ["id"]
            }),
            lifecycle_output_schema(),
        ),
        tool_with_output(
            "pause_work",
            "Mark owned work as paused without moving columns.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "assignee": { "type": "string", "description": "Defaults to 'agent'." },
                    "reason": { "type": "string" }
                },
                "required": ["id"]
            }),
            lifecycle_output_schema(),
        ),
        tool_with_output(
            "release_task",
            "Clear active ownership/lease with an optional reason note.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "assignee": { "type": "string", "description": "Defaults to 'agent'." },
                    "reason": { "type": "string" },
                    "force": { "type": "boolean" }
                },
                "required": ["id"]
            }),
            lifecycle_output_schema(),
        ),
        tool_with_output(
            "reassign_task",
            "Transfer ownership, requiring force for active non-stale conflicts.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "assignee": { "type": "string", "description": "New assignee." },
                    "force": { "type": "boolean" }
                },
                "required": ["id", "assignee"]
            }),
            lifecycle_output_schema(),
        ),
        tool_with_output(
            "block_task",
            "Add machine-readable blocker metadata without changing columns.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "reason": { "type": "string" },
                    "category": { "type": "string" },
                    "author": { "type": "string" }
                },
                "required": ["id", "reason"]
            }),
            lifecycle_output_schema(),
        ),
        tool_with_output(
            "unblock_task",
            "Clear blocker metadata, optionally appending a note.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "note": { "type": "string" },
                    "author": { "type": "string" }
                },
                "required": ["id"]
            }),
            lifecycle_output_schema(),
        ),
        tool_with_output(
            "ready_for_review",
            "Append a handoff summary, move to In Review, clear active lifecycle metadata, and trigger the duplicate-gated review path.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "summary": { "type": "string" },
                    "author": { "type": "string" }
                },
                "required": ["id", "summary"]
            }),
            lifecycle_output_schema(),
        ),
        tool_with_output(
            "submit_review",
            "Append a structured review note/verdict while leaving completion manual and the task in In Review.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "verdict": { "type": "string", "description": "e.g. approved, changes_requested, blocked" },
                    "summary": { "type": "string" },
                    "author": { "type": "string", "description": "Defaults to 'reviewer'." }
                },
                "required": ["id", "verdict"]
            }),
            lifecycle_output_schema(),
        ),
    ]
}

/// Execute a tool by name. `Ok` carries text and optional structured content for the
/// user/agent; `Err` is a tool error result, not a transport failure.
pub fn call(
    name: &str,
    arguments: &Value,
    project_root: &Path,
) -> Result<ToolCallOutput, ToolCallError> {
    match name {
        "list_tasks" => list_tasks(arguments, project_root),
        "get_task" => get_task(arguments, project_root),
        "create_task" => create_task(arguments, project_root),
        "claim_task" => claim_task(arguments, project_root),
        "update_task_status" => update_task_status(arguments, project_root),
        "complete_task" => complete_task(arguments, project_root),
        "add_task_note" => add_task_note(arguments, project_root),
        "add_task_knowledge" => add_task_knowledge(arguments, project_root),
        "start_work" => start_work(arguments, project_root),
        "heartbeat_task" => heartbeat_task(arguments, project_root),
        "pause_work" => pause_work(arguments, project_root),
        "release_task" => release_task(arguments, project_root),
        "reassign_task" => reassign_task(arguments, project_root),
        "block_task" => block_task(arguments, project_root),
        "unblock_task" => unblock_task(arguments, project_root),
        "ready_for_review" => ready_for_review(arguments, project_root),
        "submit_review" => submit_review(arguments, project_root),
        other => Err(text_error(format!("unknown tool '{other}'"))),
    }
}

fn list_tasks(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let board = board_store::load(project_root);
    let tasks: Vec<&Task> = match optional_str(arguments, "status") {
        Some(raw) => board_service::tasks_by_status(&board, parse_status(raw)?),
        None => board.tasks.iter().collect(),
    };
    let text = serde_json::to_string_pretty(&tasks).map_err(json_error)?;
    Ok(output(text, Some(json!({ "tasks": tasks }))))
}

fn get_task(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let board = board_store::load(project_root);
    let task = board_service::get_task(&board, id)
        .ok_or_else(|| text_error(format!("no task with id '{id}'")))?;
    let text = serde_json::to_string_pretty(task).map_err(json_error)?;
    Ok(output(text, Some(json!({ "task": task }))))
}

fn create_task(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let title = require_str(arguments, "title")?;
    let description = optional_str(arguments, "description").unwrap_or_default();
    let status = match optional_str(arguments, "status") {
        Some(raw) => parse_status(raw)?,
        None => TaskStatus::Todo,
    };
    let task = board_store::update(project_root, |board| {
        board_service::create_task(board, title, description, status).clone()
    })
    .map_err(io_error)?;
    Ok(output(
        format!("Created task {} in {}.", task.id, status.column_title()),
        Some(json!({ "ok": true, "action": "create_task", "task_id": task.id, "task": task })),
    ))
}

fn claim_task(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let assignee = optional_str(arguments, "assignee").unwrap_or("agent");
    let task = board_store::update(project_root, |board| {
        board_service::claim_task(board, id, assignee).cloned()
    })
    .map_err(io_error)?
    .map_err(board_error)?;
    Ok(output(
        format!("Claimed task {id} as '{assignee}' (In Progress)."),
        Some(task_structured("claim_task", &task, Vec::new())),
    ))
}

fn update_task_status(
    arguments: &Value,
    project_root: &Path,
) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let status = parse_status(require_str(arguments, "status")?)?;
    let review = review_dispatch::set_status_and_claim_auto_review(project_root, id, status)
        .map_err(text_error)?;
    let mut message = format!("Moved task {id} to {}.", status.column_title());
    let mut review_started = None;
    if let Some(selection) = review {
        match review_dispatch::spawn_headless_review(project_root, &selection) {
            Ok(run) => {
                message.push_str(&format!(
                    " Started {} headless review (pid {}, log {}).",
                    selection.reviewer.label(),
                    run.pid,
                    run.log_path.display()
                ));
                review_started = Some(json!({
                    "reviewer": selection.reviewer.assignee_id(),
                    "pid": run.pid,
                    "log_path": run.log_path.display().to_string()
                }));
            }
            Err(error) => message.push_str(&format!(
                " Could not start headless review for {}: {error}",
                selection.reviewer.label()
            )),
        }
    }
    let board = board_store::load(project_root);
    let task = board_service::get_task(&board, id).cloned();
    Ok(output(
        message,
        Some(json!({
            "ok": true,
            "action": "update_task_status",
            "task_id": id,
            "status": status.wire_id(),
            "task": task,
            "review_started": review_started,
        })),
    ))
}

fn complete_task(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let note = optional_str(arguments, "note").map(str::to_string);
    let task = board_store::update(project_root, |board| {
        board_service::complete_task(board, id, note).cloned()
    })
    .map_err(io_error)?
    .map_err(board_error)?;
    Ok(output(
        format!("Completed task {id}."),
        Some(task_structured("complete_task", &task, Vec::new())),
    ))
}

fn add_task_note(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let text = require_str(arguments, "text")?;
    let author = optional_str(arguments, "author").map(str::to_string);
    let task = board_store::update(project_root, |board| {
        board_service::add_note(board, id, text, author).cloned()
    })
    .map_err(io_error)?
    .map_err(board_error)?;
    Ok(output(
        format!("Added note to task {id}."),
        Some(task_structured("add_task_note", &task, Vec::new())),
    ))
}

fn add_task_knowledge(
    arguments: &Value,
    project_root: &Path,
) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let title = require_str(arguments, "title")?;
    let content = require_str(arguments, "content")?;
    let source = Some(
        optional_str(arguments, "source")
            .unwrap_or("agent")
            .to_string(),
    );
    let category = optional_str(arguments, "category").map(str::to_string);
    let author = optional_str(arguments, "author").map(str::to_string);
    let task = board_store::update(project_root, |board| {
        board_service::add_knowledge(board, id, title, content, source, category, author).cloned()
    })
    .map_err(io_error)?
    .map_err(board_error)?;
    Ok(output(
        format!("Recorded knowledge on task {id}."),
        Some(task_structured("add_task_knowledge", &task, Vec::new())),
    ))
}

fn start_work(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let assignee = optional_str(arguments, "assignee").unwrap_or("agent");
    let stale_after_secs = optional_u64(arguments, "stale_after_secs")?;
    let force = optional_bool(arguments, "force").unwrap_or(false);
    let transition = board_store::update(project_root, |board| {
        board_service::start_work(board, id, assignee, stale_after_secs, force).map(|transition| {
            board_service::LifecycleTransition {
                task: transition.task.clone(),
                warnings: transition.warnings,
            }
        })
    })
    .map_err(io_error)?
    .map_err(board_error)?;
    Ok(output(
        format!("Started work on task {id} as '{assignee}'."),
        Some(task_structured(
            "start_work",
            &transition.task,
            transition.warnings,
        )),
    ))
}

fn heartbeat_task(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let assignee = optional_str(arguments, "assignee").unwrap_or("agent");
    let note = optional_str(arguments, "note").map(str::to_string);
    let task = board_store::update(project_root, |board| {
        board_service::heartbeat_task(board, id, assignee, note).cloned()
    })
    .map_err(io_error)?
    .map_err(board_error)?;
    Ok(output(
        format!("Heartbeat recorded for task {id} as '{assignee}'."),
        Some(task_structured("heartbeat_task", &task, Vec::new())),
    ))
}

fn pause_work(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let assignee = optional_str(arguments, "assignee").unwrap_or("agent");
    let reason = optional_str(arguments, "reason").map(str::to_string);
    let task = board_store::update(project_root, |board| {
        board_service::pause_work(board, id, assignee, reason).cloned()
    })
    .map_err(io_error)?
    .map_err(board_error)?;
    Ok(output(
        format!("Paused work on task {id} as '{assignee}'."),
        Some(task_structured("pause_work", &task, Vec::new())),
    ))
}

fn release_task(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let assignee = optional_str(arguments, "assignee").unwrap_or("agent");
    let reason = optional_str(arguments, "reason").map(str::to_string);
    let force = optional_bool(arguments, "force").unwrap_or(false);
    let task = board_store::update(project_root, |board| {
        board_service::release_task(board, id, assignee, reason, force).cloned()
    })
    .map_err(io_error)?
    .map_err(board_error)?;
    Ok(output(
        format!("Released task {id}."),
        Some(task_structured("release_task", &task, Vec::new())),
    ))
}

fn reassign_task(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let assignee = require_str(arguments, "assignee")?;
    let force = optional_bool(arguments, "force").unwrap_or(false);
    let transition = board_store::update(project_root, |board| {
        board_service::reassign_task(board, id, assignee, force).map(|transition| {
            board_service::LifecycleTransition {
                task: transition.task.clone(),
                warnings: transition.warnings,
            }
        })
    })
    .map_err(io_error)?
    .map_err(board_error)?;
    Ok(output(
        format!("Reassigned task {id} to '{assignee}'."),
        Some(task_structured(
            "reassign_task",
            &transition.task,
            transition.warnings,
        )),
    ))
}

fn block_task(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let reason = require_str(arguments, "reason")?;
    let category = optional_str(arguments, "category").map(str::to_string);
    let author = optional_str(arguments, "author").map(str::to_string);
    let task = board_store::update(project_root, |board| {
        board_service::block_task(board, id, reason, category, author).cloned()
    })
    .map_err(io_error)?
    .map_err(board_error)?;
    Ok(output(
        format!("Blocked task {id}."),
        Some(task_structured("block_task", &task, Vec::new())),
    ))
}

fn unblock_task(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let note = optional_str(arguments, "note").map(str::to_string);
    let author = optional_str(arguments, "author").map(str::to_string);
    let task = board_store::update(project_root, |board| {
        board_service::unblock_task(board, id, note, author).cloned()
    })
    .map_err(io_error)?
    .map_err(board_error)?;
    Ok(output(
        format!("Unblocked task {id}."),
        Some(task_structured("unblock_task", &task, Vec::new())),
    ))
}

fn ready_for_review(
    arguments: &Value,
    project_root: &Path,
) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let summary = require_str(arguments, "summary")?.to_string();
    let author = optional_str(arguments, "author").map(str::to_string);
    let review = review_dispatch::ready_for_review_and_claim_auto_review(
        project_root,
        id,
        summary,
        author.clone(),
    )
    .map_err(text_error)?;
    let mut message = format!("Moved task {id} to In Review with handoff summary.");
    let mut review_started = None;
    if let Some(selection) = review {
        match review_dispatch::spawn_headless_review(project_root, &selection) {
            Ok(run) => {
                message.push_str(&format!(
                    " Started {} headless review (pid {}, log {}).",
                    selection.reviewer.label(),
                    run.pid,
                    run.log_path.display()
                ));
                review_started = Some(json!({
                    "reviewer": selection.reviewer.assignee_id(),
                    "pid": run.pid,
                    "log_path": run.log_path.display().to_string()
                }));
            }
            Err(error) => message.push_str(&format!(
                " Could not start headless review for {}: {error}",
                selection.reviewer.label()
            )),
        }
    }
    let board = board_store::load(project_root);
    let task = board_service::get_task(&board, id)
        .cloned()
        .ok_or_else(|| text_error(format!("no task with id '{id}'")))?;
    let mut structured = task_structured("ready_for_review", &task, Vec::new());
    structured["review_started"] = review_started.unwrap_or(Value::Null);
    Ok(output(message, Some(structured)))
}

fn submit_review(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let verdict = require_str(arguments, "verdict")?;
    let summary = optional_str(arguments, "summary").unwrap_or_default();
    let author = optional_str(arguments, "author")
        .unwrap_or("reviewer")
        .to_string();
    let task = board_store::update(project_root, |board| {
        board_service::submit_review(board, id, verdict, summary, Some(author)).cloned()
    })
    .map_err(io_error)?
    .map_err(board_error)?;
    Ok(output(
        format!("Submitted review for task {id}; task remains In Review."),
        Some(task_structured("submit_review", &task, Vec::new())),
    ))
}

fn parse_status(raw: &str) -> Result<TaskStatus, ToolCallError> {
    TaskStatus::from_wire(raw).ok_or_else(|| text_error(format!("unknown status '{raw}'")))
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": input_schema })
}

fn tool_with_output(
    name: &str,
    description: &str,
    input_schema: Value,
    output_schema: Value,
) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
        "outputSchema": output_schema,
    })
}

fn lifecycle_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ok": { "type": "boolean" },
            "action": { "type": "string" },
            "task_id": { "type": "string" },
            "task": { "type": ["object", "null"] },
            "lifecycle": { "type": ["object", "null"] },
            "warnings": { "type": "array", "items": { "type": "string" } },
            "conflict": { "type": ["object", "null"] },
            "review_started": { "type": ["object", "null"] }
        },
        "required": ["ok", "action", "task_id", "warnings"]
    })
}

fn require_str<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, ToolCallError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| text_error(format!("missing required string parameter '{key}'")))
}

fn optional_str<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn optional_bool(arguments: &Value, key: &str) -> Option<bool> {
    arguments.get(key).and_then(Value::as_bool)
}

fn optional_u64(arguments: &Value, key: &str) -> Result<Option<u64>, ToolCallError> {
    let Some(value) = arguments.get(key) else {
        return Ok(None);
    };
    let Some(raw) = value.as_u64() else {
        return Err(text_error(format!(
            "parameter '{key}' must be a positive integer"
        )));
    };
    if raw == 0 {
        return Err(text_error(format!(
            "parameter '{key}' must be greater than zero"
        )));
    }
    Ok(Some(raw))
}

fn output(text: impl Into<String>, structured: Option<Value>) -> ToolCallOutput {
    ToolCallOutput {
        text: text.into(),
        structured,
    }
}

fn text_error(text: impl Into<String>) -> ToolCallError {
    ToolCallError {
        text: text.into(),
        structured: None,
    }
}

fn io_error(error: impl std::fmt::Display) -> ToolCallError {
    text_error(error.to_string())
}

fn json_error(error: impl std::fmt::Display) -> ToolCallError {
    text_error(error.to_string())
}

fn board_error(error: board_service::BoardError) -> ToolCallError {
    match error {
        board_service::BoardError::TaskNotFound(id) => {
            text_error(format!("no task with id '{id}'"))
        }
        board_service::BoardError::OwnershipConflict(conflict) => {
            let text = board_service::BoardError::OwnershipConflict(conflict.clone()).to_string();
            ToolCallError {
                text,
                structured: Some(json!({
                    "ok": false,
                    "action": "ownership_conflict",
                    "task_id": conflict.task_id,
                    "task": null,
                    "lifecycle": null,
                    "warnings": [],
                    "conflict": {
                        "current_assignee": conflict.current_assignee,
                        "requested_assignee": conflict.requested_assignee,
                        "heartbeat_at": conflict.heartbeat_at,
                        "claimed_at": conflict.claimed_at,
                        "stale_after_secs": conflict.stale_after_secs,
                        "now": conflict.now,
                    }
                })),
            }
        }
    }
}

fn task_structured(action: &str, task: &Task, warnings: Vec<String>) -> Value {
    let now = now_epoch_secs();
    json!({
        "ok": true,
        "action": action,
        "task_id": task.id,
        "task": task,
        "lifecycle": {
            "assignee": task.assignee,
            "claimed_at": task.claimed_at,
            "heartbeat_at": task.heartbeat_at,
            "stale_after_secs": task.stale_after_secs,
            "paused": task.paused,
            "blocked": task.blocked,
            "stale": board_service::task_is_stale(task, now),
            "indicators": board_service::lifecycle_indicators(task, now),
        },
        "warnings": warnings,
        "conflict": null,
    })
}
