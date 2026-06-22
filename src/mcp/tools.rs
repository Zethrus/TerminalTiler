//! MCP tool registry for the Kanban board.
//!
//! Each tool loads the per-project board, applies one `services::board` operation, saves
//! atomically, and returns a short text result. Tools operate on the board at
//! `project_root` (the agent's working directory).

use std::path::Path;

use serde_json::{Value, json};

use crate::model::board::{Board, Task, TaskStatus, now_epoch_secs};
use crate::services::{agent_config, board as board_service, review_dispatch};
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
            "List board tasks, optionally filtered by column, assignee, blocked state, or availability.",
            json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string", "enum": status_enum, "description": "Optional column filter." },
                    "available_only": { "type": "boolean", "description": "Only tasks that are not blocked and have no active fresh assignee lease." },
                    "assignee": { "type": "string", "description": "Only tasks assigned to this id." },
                    "blocked": { "type": "boolean", "description": "Filter blocked or unblocked tasks." }
                }
            }),
        ),
        tool_with_output(
            "get_board_summary",
            "Return compact board counts, lifecycle counts, and recommended next available tasks.",
            json!({ "type": "object", "properties": {} }),
            board_summary_output_schema(),
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
            "get_task_brief",
            "Get a concise markdown brief for one task, including lifecycle, notes, knowledge, and attachments.",
            json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }),
        ),
        tool_with_output(
            "diagnose_mcp",
            "Inspect TerminalTiler MCP setup for this project without changing configuration.",
            json!({ "type": "object", "properties": {} }),
            mcp_diagnostics_output_schema(),
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
                    "assignee": { "type": "string", "description": "Defaults to 'agent'." },
                    "force": { "type": "boolean", "description": "Take over a fresh active lease intentionally." }
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
                    "status": { "type": "string", "enum": status_enum.clone() },
                    "assignee": { "type": "string", "description": "Caller/owner id; defaults to 'agent'." },
                    "author": { "type": "string", "description": "Alias for assignee for status changes." },
                    "force": { "type": "boolean", "description": "Override a fresh active lease owned by someone else." }
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
                    "note": { "type": "string" },
                    "author": { "type": "string", "description": "Caller/owner id; defaults to 'agent'." },
                    "assignee": { "type": "string", "description": "Alias for author." },
                    "force": { "type": "boolean", "description": "Override a fresh active lease owned by someone else." }
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
                    "author": { "type": "string" },
                    "assignee": { "type": "string", "description": "Alias for author." },
                    "force": { "type": "boolean", "description": "Override a fresh active lease owned by someone else." }
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
        "get_board_summary" => get_board_summary(arguments, project_root),
        "get_task" => get_task(arguments, project_root),
        "get_task_brief" => get_task_brief(arguments, project_root),
        "diagnose_mcp" => diagnose_mcp(arguments, project_root),
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
    let status = optional_str(arguments, "status")
        .map(parse_status)
        .transpose()?;
    let assignee = optional_str(arguments, "assignee");
    let blocked = optional_bool(arguments, "blocked");
    let available_only = optional_bool(arguments, "available_only").unwrap_or(false);
    let now = now_epoch_secs();
    let tasks: Vec<&Task> = board
        .tasks
        .iter()
        .filter(|task| status.is_none_or(|status| task.status == status))
        .filter(|task| assignee.is_none_or(|assignee| task.assignee.as_deref() == Some(assignee)))
        .filter(|task| blocked.is_none_or(|blocked| task.blocked.is_some() == blocked))
        .filter(|task| {
            !available_only
                || (task.status == TaskStatus::Todo
                    && task.blocked.is_none()
                    && (!has_fresh_active_lease(task, now)))
        })
        .collect();
    let text = serde_json::to_string_pretty(&tasks).map_err(json_error)?;
    Ok(output(
        text,
        Some(json!({
            "ok": true,
            "action": "list_tasks",
            "tasks": tasks,
            "count": tasks.len(),
            "filters": {
                "status": status.map(|status| status.wire_id()),
                "available_only": available_only,
                "assignee": assignee,
                "blocked": blocked,
            }
        })),
    ))
}

fn get_board_summary(
    _arguments: &Value,
    project_root: &Path,
) -> Result<ToolCallOutput, ToolCallError> {
    let board = board_store::load(project_root);
    let structured = board_summary_value(&board);
    let text = board_summary_text(&board);
    Ok(output(text, Some(structured)))
}

fn get_task(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let board = board_store::load(project_root);
    let task = board_service::get_task(&board, id)
        .ok_or_else(|| text_error(format!("no task with id '{id}'")))?;
    let text = serde_json::to_string_pretty(task).map_err(json_error)?;
    Ok(output(
        text,
        Some(json!({ "ok": true, "action": "get_task", "task_id": id, "task": task })),
    ))
}

fn get_task_brief(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let board = board_store::load(project_root);
    let task = board_service::get_task(&board, id)
        .ok_or_else(|| text_error(format!("no task with id '{id}'")))?;
    let text = task_brief_markdown(task);
    Ok(output(
        text.clone(),
        Some(
            json!({ "ok": true, "action": "get_task_brief", "task_id": id, "task": task, "brief": text }),
        ),
    ))
}

fn diagnose_mcp(_arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let diagnostics = agent_config::diagnose_mcp(project_root);
    let text = format!(
        "Project: {}\nBoard: {} ({})\nMCP binary: {} ({})\nClaude: {} ({})\nCodex: {} ({})",
        diagnostics.project_root.display(),
        diagnostics.board_path.display(),
        if diagnostics.board_exists {
            "present"
        } else {
            "missing"
        },
        diagnostics.mcp_binary_path.display(),
        if diagnostics.mcp_binary_exists {
            "present"
        } else {
            "PATH lookup or missing"
        },
        diagnostics.claude_config_path.display(),
        diagnostics.claude_detail,
        diagnostics
            .codex_config_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unresolved>".to_string()),
        diagnostics.codex_detail,
    );
    Ok(output(
        text,
        Some(json!({ "ok": true, "action": "diagnose_mcp", "diagnostics": diagnostics.to_json() })),
    ))
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
    let force = optional_bool(arguments, "force").unwrap_or(false);
    let task = board_store::update(project_root, |board| {
        board_service::start_work(board, id, assignee, None, force).map(|transition| {
            board_service::LifecycleTransition {
                task: transition.task.clone(),
                warnings: transition.warnings,
            }
        })
    })
    .map_err(io_error)?
    .map_err(board_error)?;
    Ok(output(
        format!("Claimed task {id} as '{assignee}' (In Progress)."),
        Some(task_structured("claim_task", &task.task, task.warnings)),
    ))
}

fn update_task_status(
    arguments: &Value,
    project_root: &Path,
) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let status = parse_status(require_str(arguments, "status")?)?;
    let actor = actor_argument(arguments);
    let force = optional_bool(arguments, "force").unwrap_or(false);
    let transition = board_store::update(
        project_root,
        |board| -> Result<board_service::LifecycleTransition<Task>, board_service::BoardError> {
            let transition = board_service::set_status_as(board, id, status, actor.clone(), force)?;
            Ok(board_service::LifecycleTransition {
                task: transition.task.clone(),
                warnings: transition.warnings,
            })
        },
    )
    .map_err(io_error)?
    .map_err(board_error)?;
    let review = if status == TaskStatus::InReview {
        claim_auto_review(project_root, id).map_err(text_error)?
    } else {
        None
    };
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
    let task = board_service::get_task(&board, id)
        .cloned()
        .or(Some(transition.task));
    Ok(output(
        message,
        Some(json!({
            "ok": true,
            "action": "update_task_status",
            "task_id": id,
            "status": status.wire_id(),
            "task": task,
            "warnings": transition.warnings,
            "review_started": review_started,
        })),
    ))
}

fn complete_task(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let note = optional_str(arguments, "note").map(str::to_string);
    let author = Some(actor_argument(arguments));
    let force = optional_bool(arguments, "force").unwrap_or(false);
    let task = board_store::update(project_root, |board| {
        board_service::complete_task_as(board, id, note, author, force).map(|transition| {
            board_service::LifecycleTransition {
                task: transition.task.clone(),
                warnings: transition.warnings,
            }
        })
    })
    .map_err(io_error)?
    .map_err(board_error)?;
    Ok(output(
        format!("Completed task {id}."),
        Some(task_structured("complete_task", &task.task, task.warnings)),
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
    let author = Some(actor_argument(arguments));
    let force = optional_bool(arguments, "force").unwrap_or(false);
    let transition = board_store::update(
        project_root,
        |board| -> Result<board_service::LifecycleTransition<Task>, board_service::BoardError> {
            let transition = board_service::ready_for_review_as(
                board,
                id,
                summary.clone(),
                author.clone(),
                force,
            )?;
            Ok(board_service::LifecycleTransition {
                task: transition.task.clone(),
                warnings: transition.warnings,
            })
        },
    )
    .map_err(io_error)?
    .map_err(board_error)?;
    let review = claim_auto_review(project_root, id).map_err(text_error)?;
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
        .unwrap_or(transition.task);
    let mut structured = task_structured("ready_for_review", &task, transition.warnings);
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
    tool_with_output(name, description, input_schema, basic_output_schema())
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

fn basic_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ok": { "type": "boolean" },
            "action": { "type": "string" }
        },
        "required": ["ok", "action"]
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

fn board_summary_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ok": { "type": "boolean" },
            "action": { "type": "string" },
            "total": { "type": "integer", "minimum": 0 },
            "by_status": {
                "type": "object",
                "additionalProperties": { "type": "integer", "minimum": 0 }
            },
            "lifecycle": {
                "type": "object",
                "properties": {
                    "active": { "type": "integer", "minimum": 0 },
                    "stale": { "type": "integer", "minimum": 0 },
                    "blocked": { "type": "integer", "minimum": 0 },
                    "in_review": { "type": "integer", "minimum": 0 }
                },
                "required": ["active", "stale", "blocked", "in_review"]
            },
            "available": { "type": "array", "items": { "type": "object" } }
        },
        "required": ["ok", "action", "total", "by_status", "lifecycle", "available"]
    })
}

fn mcp_diagnostics_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ok": { "type": "boolean" },
            "action": { "type": "string" },
            "diagnostics": {
                "type": "object",
                "properties": {
                    "project_root": { "type": "string" },
                    "board_path": { "type": "string" },
                    "board_exists": { "type": "boolean" },
                    "mcp_binary_path": { "type": "string" },
                    "mcp_binary_exists": { "type": "boolean" },
                    "claude": {
                        "type": "object",
                        "properties": {
                            "config_path": { "type": "string" },
                            "configured": { "type": "boolean" },
                            "detail": { "type": "string" }
                        },
                        "required": ["config_path", "configured", "detail"]
                    },
                    "codex": {
                        "type": "object",
                        "properties": {
                            "config_path": { "type": ["string", "null"] },
                            "configured": { "type": "boolean" },
                            "detail": { "type": "string" }
                        },
                        "required": ["config_path", "configured", "detail"]
                    }
                },
                "required": [
                    "project_root",
                    "board_path",
                    "board_exists",
                    "mcp_binary_path",
                    "mcp_binary_exists",
                    "claude",
                    "codex"
                ]
            }
        },
        "required": ["ok", "action", "diagnostics"]
    })
}

pub fn board_summary_value(board: &Board) -> Value {
    let now = now_epoch_secs();
    let mut by_status = serde_json::Map::new();
    for status in TaskStatus::ALL {
        by_status.insert(
            status.wire_id().to_string(),
            json!(
                board
                    .tasks
                    .iter()
                    .filter(|task| task.status == status)
                    .count()
            ),
        );
    }
    let active = board
        .tasks
        .iter()
        .filter(|task| has_fresh_active_lease(task, now))
        .count();
    let stale = board
        .tasks
        .iter()
        .filter(|task| task.assignee.is_some() && board_service::task_is_stale(task, now))
        .count();
    let blocked = board
        .tasks
        .iter()
        .filter(|task| task.blocked.is_some())
        .count();
    let review = board
        .tasks
        .iter()
        .filter(|task| task.status == TaskStatus::InReview)
        .count();
    let available: Vec<&Task> = board
        .tasks
        .iter()
        .filter(|task| {
            task.status == TaskStatus::Todo
                && task.blocked.is_none()
                && !has_fresh_active_lease(task, now)
        })
        .take(5)
        .collect();

    json!({
        "ok": true,
        "action": "get_board_summary",
        "total": board.tasks.len(),
        "by_status": by_status,
        "lifecycle": {
            "active": active,
            "stale": stale,
            "blocked": blocked,
            "in_review": review,
        },
        "available": available,
    })
}

pub fn board_summary_text(board: &Board) -> String {
    let now = now_epoch_secs();
    let active = board
        .tasks
        .iter()
        .filter(|task| has_fresh_active_lease(task, now))
        .count();
    let stale = board
        .tasks
        .iter()
        .filter(|task| task.assignee.is_some() && board_service::task_is_stale(task, now))
        .count();
    let blocked = board
        .tasks
        .iter()
        .filter(|task| task.blocked.is_some())
        .count();
    let review = board
        .tasks
        .iter()
        .filter(|task| task.status == TaskStatus::InReview)
        .count();
    let mut lines = vec![
        format!("Total tasks: {}", board.tasks.len()),
        format!("Lifecycle: {active} active, {stale} stale, {blocked} blocked, {review} in review"),
    ];
    for status in TaskStatus::ALL {
        let count = board
            .tasks
            .iter()
            .filter(|task| task.status == status)
            .count();
        lines.push(format!("{}: {count}", status.column_title()));
    }
    lines.join("\n")
}

pub fn task_brief_markdown(task: &Task) -> String {
    let mut text = format!(
        "# {} ({})\n\n- id: `{}`\n- status: `{}`\n",
        task.title,
        task.status.column_title(),
        task.id,
        task.status.wire_id()
    );
    if let Some(assignee) = task.assignee.as_deref() {
        text.push_str(&format!("- assignee: `{assignee}`\n"));
    }
    if let Some(claimed_at) = task.claimed_at {
        text.push_str(&format!("- claimed_at: {claimed_at}\n"));
    }
    if let Some(heartbeat_at) = task.heartbeat_at {
        text.push_str(&format!("- heartbeat_at: {heartbeat_at}\n"));
    }
    for indicator in board_service::lifecycle_indicators(task, now_epoch_secs()) {
        text.push_str(&format!("- lifecycle: {indicator}\n"));
    }
    if !task.description.trim().is_empty() {
        text.push_str("\n## Description\n\n");
        text.push_str(task.description.trim());
        text.push('\n');
    }
    if let Some(instructions) = task.additional_instructions.as_deref() {
        text.push_str("\n## Additional instructions\n\n");
        text.push_str(instructions.trim());
        text.push('\n');
    }
    if let Some(blocked) = task.blocked.as_ref() {
        text.push_str("\n## Blocked\n\n");
        text.push_str(&blocked.reason);
        text.push('\n');
    }
    if !task.notes.is_empty() {
        text.push_str("\n## Notes\n\n");
        for note in &task.notes {
            let author = note.author.as_deref().unwrap_or("unknown");
            text.push_str(&format!(
                "- [{}] {}: {}\n",
                note.created_at, author, note.text
            ));
        }
    }
    if !task.knowledge.is_empty() {
        text.push_str("\n## Knowledge\n\n");
        for entry in &task.knowledge {
            text.push_str(&format!("- **{}**: {}\n", entry.title, entry.content));
        }
    }
    if !task.attachments.is_empty() {
        text.push_str("\n## Attachments\n\n");
        for attachment in &task.attachments {
            text.push_str(&format!("- {} (`{}`)\n", attachment.name, attachment.path));
        }
    }
    text
}

pub fn workflow_guide_markdown() -> String {
    [
        "# TerminalTiler Kanban MCP workflow",
        "",
        "1. Call `get_board_summary` or `list_tasks` to find work.",
        "2. Call `start_work` or legacy `claim_task` with your `assignee` before editing.",
        "3. Send `heartbeat_task`, `add_task_note`, and `add_task_knowledge` while working.",
        "4. Call `ready_for_review` with `author` and a handoff summary when implementation is ready.",
        "5. Reviewers call `submit_review`; only call `complete_task` when explicitly closing the task.",
        "",
        "Fresh active leases owned by another assignee return an `ownership_conflict` tool error unless `force` is set.",
    ]
    .join("\n")
}

fn actor_argument(arguments: &Value) -> String {
    optional_str(arguments, "author")
        .or_else(|| optional_str(arguments, "assignee"))
        .unwrap_or("agent")
        .to_string()
}

fn has_fresh_active_lease(task: &Task, now: u64) -> bool {
    task.heartbeat_at.or(task.claimed_at).is_some()
        && task.paused.is_none()
        && !board_service::task_is_stale(task, now)
}

fn claim_auto_review(
    project_root: &Path,
    task_id: &str,
) -> Result<Option<review_dispatch::ReviewSelection>, String> {
    board_store::update(
        project_root,
        |board| -> Result<Option<review_dispatch::ReviewSelection>, board_service::BoardError> {
            let task = board_service::get_task(board, task_id)
                .cloned()
                .ok_or_else(|| board_service::BoardError::TaskNotFound(task_id.to_string()))?;
            if !task.needs_auto_review() {
                return Ok(None);
            }

            let reviewer = board_service::reviewer_for_task(board, &task);
            let yolo = board.automation.yolo_default;
            let task = board_service::start_review(board, task_id, reviewer)?.clone();
            Ok(Some(review_dispatch::ReviewSelection {
                task,
                reviewer,
                yolo,
            }))
        },
    )
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
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
#[cfg(test)]
mod tests {
    use super::*;

    fn tool_definition<'a>(tools: &'a [Value], name: &str) -> &'a Value {
        tools
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("missing tool definition for {name}"))
    }

    fn required_fields(schema: &Value) -> Vec<&str> {
        schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| field.as_str().unwrap())
            .collect()
    }

    #[test]
    fn summary_and_diagnostic_tools_advertise_matching_output_schemas() {
        let tools = list_json();
        let summary_schema = &tool_definition(&tools, "get_board_summary")["outputSchema"];
        let summary_required = required_fields(summary_schema);
        assert!(summary_required.contains(&"total"));
        assert!(summary_required.contains(&"by_status"));
        assert!(summary_required.contains(&"lifecycle"));
        assert!(summary_required.contains(&"available"));
        assert!(!summary_required.contains(&"task_id"));
        assert!(!summary_required.contains(&"warnings"));

        let diagnostics_schema = &tool_definition(&tools, "diagnose_mcp")["outputSchema"];
        let diagnostics_required = required_fields(diagnostics_schema);
        assert!(diagnostics_required.contains(&"diagnostics"));
        assert!(!diagnostics_required.contains(&"task_id"));
        assert!(!diagnostics_required.contains(&"warnings"));

        let lifecycle_schema = &tool_definition(&tools, "start_work")["outputSchema"];
        let lifecycle_required = required_fields(lifecycle_schema);
        assert!(lifecycle_required.contains(&"task_id"));
        assert!(lifecycle_required.contains(&"warnings"));
    }
}
