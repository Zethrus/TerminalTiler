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
        tool_with_output(
            "get_my_work",
            "Return tasks already owned by this assignee, grouped as active, stale, paused, and in_review so agents can resume safely.",
            json!({
                "type": "object",
                "properties": {
                    "assignee": { "type": "string", "description": "Owner id to inspect; defaults to 'agent'." }
                }
            }),
            my_work_output_schema(),
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
            "start_next_work",
            "Atomically claim the first available unblocked To Do task in board order, skipping fresh active leases.",
            json!({
                "type": "object",
                "properties": {
                    "assignee": { "type": "string", "description": "Defaults to 'agent'." },
                    "stale_after_secs": { "type": "integer", "minimum": 1, "description": "Override soft-lease stale threshold for the claimed task; default is 21600 (6 hours)." }
                }
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
                    "changed_files": string_or_string_array_schema("Optional handoff file list."),
                    "tests": string_or_string_array_schema("Optional tests run."),
                    "risks": string_or_string_array_schema("Optional remaining risks."),
                    "next_steps": string_or_string_array_schema("Optional follow-up steps."),
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
                    "severity": { "type": "string", "description": "Optional severity label, e.g. info, low, medium, high, critical." },
                    "findings": string_or_string_array_schema("Optional finding list."),
                    "recommendation": { "type": "string", "description": "Optional final reviewer recommendation." },
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
        "get_my_work" => get_my_work(arguments, project_root),
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
        "start_next_work" => start_next_work(arguments, project_root),
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
                    && (!board_service::has_fresh_active_lease(task, now)))
        })
        .collect();
    let text = serde_json::to_string_pretty(&tasks).map_err(json_error)?;
    Ok(output(
        text,
        Some(with_context(
            json!({
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
            }),
            project_root,
        )),
    ))
}

fn get_board_summary(
    _arguments: &Value,
    project_root: &Path,
) -> Result<ToolCallOutput, ToolCallError> {
    let board = board_store::load(project_root);
    let structured = with_context(board_summary_value(&board), project_root);
    let text = board_summary_text(&board);
    Ok(output(text, Some(structured)))
}

fn get_my_work(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let assignee = optional_str(arguments, "assignee").unwrap_or("agent");
    let board = board_store::load(project_root);
    let now = now_epoch_secs();
    let work = board_service::get_my_work(&board, assignee, now);
    let active_count = work.active.len();
    let stale_count = work.stale.len();
    let paused_count = work.paused.len();
    let in_review_count = work.in_review.len();
    let text = format!(
        "Owned work for '{assignee}': {} active, {} stale, {} paused, {} in review.",
        active_count, stale_count, paused_count, in_review_count
    );
    Ok(output(
        text,
        Some(with_context(
            json!({
                "ok": true,
                "action": "get_my_work",
                "assignee": work.assignee,
                "groups": {
                    "active": work.active,
                    "stale": work.stale,
                    "paused": work.paused,
                    "in_review": work.in_review,
                },
                "counts": {
                    "active": active_count,
                    "stale": stale_count,
                    "paused": paused_count,
                    "in_review": in_review_count,
                },
            }),
            project_root,
        )),
    ))
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
        "Status: {}\nProject: {}\nBoard: {} ({})\nProcess cwd: {}\nMCP binary: {} ({})\nClaude [{}]: {} ({})\nCodex [{}]: {} ({})\nCodex config root: {}",
        diagnostics.status.as_str(),
        diagnostics.project_root.display(),
        diagnostics.board_path.display(),
        if diagnostics.board_exists {
            "present"
        } else {
            "missing"
        },
        diagnostics
            .process_cwd
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unresolved>".to_string()),
        diagnostics.mcp_binary_path.display(),
        if diagnostics.mcp_binary_exists {
            "present"
        } else {
            "PATH lookup or missing"
        },
        diagnostics.claude_status.as_str(),
        diagnostics.claude_config_path.display(),
        diagnostics.claude_detail,
        diagnostics.codex_status.as_str(),
        diagnostics
            .codex_config_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unresolved>".to_string()),
        diagnostics.codex_detail,
        diagnostics
            .codex_config_root
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unresolved>".to_string()),
    );
    Ok(output(
        text,
        Some(with_context(
            json!({ "ok": true, "action": "diagnose_mcp", "diagnostics": diagnostics.to_json() }),
            project_root,
        )),
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
        Some(with_context(
            json!({ "ok": true, "action": "create_task", "task_id": task.id, "task": task }),
            project_root,
        )),
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
        Some(task_structured(
            "claim_task",
            &task.task,
            task.warnings,
            project_root,
        )),
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
    let transition = review_dispatch::set_status_as_and_claim_auto_review(
        project_root,
        id,
        status,
        actor,
        force,
    )
    .map_err(review_dispatch_error)?;
    let mut message = format!("Moved task {id} to {}.", status.column_title());
    let (review_started, review_error) =
        spawn_claimed_review(project_root, transition.selection.as_ref(), &mut message);
    let board = board_store::load(project_root);
    let task = board_service::get_task(&board, id)
        .cloned()
        .unwrap_or(transition.task);
    Ok(output(
        message,
        Some(with_context(
            json!({
                "ok": true,
                "action": "update_task_status",
                "task_id": id,
                "status": status.wire_id(),
                "task": task,
                "warnings": transition.warnings,
                "review_started": review_started,
                "review_error": review_error,
            }),
            project_root,
        )),
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
        Some(task_structured(
            "complete_task",
            &task.task,
            task.warnings,
            project_root,
        )),
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
        Some(task_structured(
            "add_task_note",
            &task,
            Vec::new(),
            project_root,
        )),
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
        Some(task_structured(
            "add_task_knowledge",
            &task,
            Vec::new(),
            project_root,
        )),
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
    .map_err(|error| board_error_with_context(error, project_root))?;
    Ok(output(
        format!("Started work on task {id} as '{assignee}'."),
        Some(task_structured(
            "start_work",
            &transition.task,
            transition.warnings,
            project_root,
        )),
    ))
}

fn start_next_work(
    arguments: &Value,
    project_root: &Path,
) -> Result<ToolCallOutput, ToolCallError> {
    let assignee = optional_str(arguments, "assignee").unwrap_or("agent");
    let stale_after_secs = optional_u64(arguments, "stale_after_secs")?;
    let transition = board_store::update(project_root, |board| {
        board_service::start_next_work(board, assignee, stale_after_secs)
    })
    .map_err(io_error)?
    .map_err(|error| board_error_with_context(error, project_root))?;

    match transition {
        Some(transition) => Ok(output(
            format!(
                "Started next available work on task {} as '{assignee}'.",
                transition.task.id
            ),
            Some(task_structured(
                "start_next_work",
                &transition.task,
                transition.warnings,
                project_root,
            )),
        )),
        None => Ok(output(
            "No available unblocked To Do task without a fresh active lease.".to_string(),
            Some(with_context(
                json!({
                    "ok": true,
                    "action": "start_next_work",
                    "task_id": Value::Null,
                    "task": Value::Null,
                    "lifecycle": Value::Null,
                    "warnings": [],
                    "conflict": Value::Null,
                    "reason": "no_available_task",
                }),
                project_root,
            )),
        )),
    }
}

fn heartbeat_task(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let assignee = optional_str(arguments, "assignee").unwrap_or("agent");
    let note = optional_str(arguments, "note").map(str::to_string);
    let task = board_store::update(project_root, |board| {
        board_service::heartbeat_task(board, id, assignee, note).cloned()
    })
    .map_err(io_error)?
    .map_err(|error| board_error_with_context(error, project_root))?;
    Ok(output(
        format!("Heartbeat recorded for task {id} as '{assignee}'."),
        Some(task_structured(
            "heartbeat_task",
            &task,
            Vec::new(),
            project_root,
        )),
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
    .map_err(|error| board_error_with_context(error, project_root))?;
    Ok(output(
        format!("Paused work on task {id} as '{assignee}'."),
        Some(task_structured(
            "pause_work",
            &task,
            Vec::new(),
            project_root,
        )),
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
    .map_err(|error| board_error_with_context(error, project_root))?;
    Ok(output(
        format!("Released task {id}."),
        Some(task_structured(
            "release_task",
            &task,
            Vec::new(),
            project_root,
        )),
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
    .map_err(|error| board_error_with_context(error, project_root))?;
    Ok(output(
        format!("Reassigned task {id} to '{assignee}'."),
        Some(task_structured(
            "reassign_task",
            &transition.task,
            transition.warnings,
            project_root,
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
    .map_err(|error| board_error_with_context(error, project_root))?;
    Ok(output(
        format!("Blocked task {id}."),
        Some(task_structured(
            "block_task",
            &task,
            Vec::new(),
            project_root,
        )),
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
    .map_err(|error| board_error_with_context(error, project_root))?;
    Ok(output(
        format!("Unblocked task {id}."),
        Some(task_structured(
            "unblock_task",
            &task,
            Vec::new(),
            project_root,
        )),
    ))
}

fn ready_for_review(
    arguments: &Value,
    project_root: &Path,
) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let summary = require_str(arguments, "summary")?.to_string();
    let changed_files = optional_string_list(arguments, "changed_files")?;
    let tests = optional_string_list(arguments, "tests")?;
    let risks = optional_string_list(arguments, "risks")?;
    let next_steps = optional_string_list(arguments, "next_steps")?;
    let handoff_note = format_handoff_note(&summary, &changed_files, &tests, &risks, &next_steps);
    let author = Some(actor_argument(arguments));
    let force = optional_bool(arguments, "force").unwrap_or(false);
    let transition = review_dispatch::ready_for_review_as_and_claim_auto_review(
        project_root,
        id,
        handoff_note,
        author,
        force,
    )
    .map_err(|error| review_dispatch_error_with_context(error, project_root))?;
    let mut message = format!("Moved task {id} to In Review with handoff summary.");
    let (review_started, review_error) =
        spawn_claimed_review(project_root, transition.selection.as_ref(), &mut message);
    let board = board_store::load(project_root);
    let task = board_service::get_task(&board, id)
        .cloned()
        .unwrap_or(transition.task);
    let mut structured =
        task_structured("ready_for_review", &task, transition.warnings, project_root);
    structured["review_started"] = review_started.unwrap_or(Value::Null);
    structured["review_error"] = review_error.unwrap_or(Value::Null);
    structured["handoff"] = json!({
        "summary": summary,
        "changed_files": changed_files,
        "tests": tests,
        "risks": risks,
        "next_steps": next_steps,
    });
    Ok(output(message, Some(structured)))
}

fn submit_review(arguments: &Value, project_root: &Path) -> Result<ToolCallOutput, ToolCallError> {
    let id = require_str(arguments, "id")?;
    let verdict = require_str(arguments, "verdict")?;
    let summary = optional_str(arguments, "summary").unwrap_or_default();
    let severity = optional_str(arguments, "severity").map(str::to_string);
    let findings = optional_string_list(arguments, "findings")?;
    let recommendation = optional_str(arguments, "recommendation").map(str::to_string);
    let review_summary = format_review_summary(
        summary,
        severity.as_deref(),
        &findings,
        recommendation.as_deref(),
    );
    let author = optional_str(arguments, "author")
        .unwrap_or("reviewer")
        .to_string();
    let task = board_store::update(project_root, |board| {
        board_service::submit_review(board, id, verdict, review_summary, Some(author)).cloned()
    })
    .map_err(io_error)?
    .map_err(|error| board_error_with_context(error, project_root))?;
    Ok(output(
        format!("Submitted review for task {id}; task remains In Review."),
        {
            let mut structured = task_structured("submit_review", &task, Vec::new(), project_root);
            structured["review"] = json!({
                "verdict": verdict,
                "summary": summary,
                "severity": severity,
                "findings": findings,
                "recommendation": recommendation,
            });
            Some(structured)
        },
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
        "title": tool_title(name),
        "description": description,
        "inputSchema": input_schema,
        "outputSchema": output_schema,
    })
}

fn tool_title(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn string_or_string_array_schema(description: &str) -> Value {
    json!({
        "description": description,
        "oneOf": [
            { "type": "string" },
            { "type": "array", "items": { "type": "string" } }
        ]
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

fn context_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "project_root": { "type": "string" },
            "board_path": { "type": "string" },
            "process_cwd": { "type": ["string", "null"] }
        },
        "required": ["project_root", "board_path", "process_cwd"]
    })
}

fn lifecycle_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ok": { "type": "boolean" },
            "action": { "type": "string" },
            "context": context_output_schema(),
            "task_id": { "type": ["string", "null"] },
            "task": { "type": ["object", "null"] },
            "lifecycle": { "type": ["object", "null"] },
            "warnings": { "type": "array", "items": { "type": "string" } },
            "conflict": { "type": ["object", "null"] },
            "review_started": { "type": ["object", "null"] },
            "review_error": { "type": ["object", "null"] },
            "reason": { "type": ["string", "null"] }
        },
        "required": ["ok", "action", "context", "task_id", "warnings"]
    })
}

fn my_work_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ok": { "type": "boolean" },
            "action": { "type": "string" },
            "context": context_output_schema(),
            "assignee": { "type": "string" },
            "groups": {
                "type": "object",
                "properties": {
                    "active": { "type": "array", "items": { "type": "object" } },
                    "stale": { "type": "array", "items": { "type": "object" } },
                    "paused": { "type": "array", "items": { "type": "object" } },
                    "in_review": { "type": "array", "items": { "type": "object" } }
                },
                "required": ["active", "stale", "paused", "in_review"]
            },
            "counts": {
                "type": "object",
                "properties": {
                    "active": { "type": "integer", "minimum": 0 },
                    "stale": { "type": "integer", "minimum": 0 },
                    "paused": { "type": "integer", "minimum": 0 },
                    "in_review": { "type": "integer", "minimum": 0 }
                },
                "required": ["active", "stale", "paused", "in_review"]
            }
        },
        "required": ["ok", "action", "context", "assignee", "groups", "counts"]
    })
}

fn board_summary_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ok": { "type": "boolean" },
            "action": { "type": "string" },
            "context": context_output_schema(),
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
            "available": { "type": "array", "items": { "type": "object" } },
            "queues": {
                "type": "object",
                "properties": {
                    "available": { "type": "array", "items": { "type": "object" } },
                    "stale": { "type": "array", "items": { "type": "object" } },
                    "blocked": { "type": "array", "items": { "type": "object" } },
                    "in_review": { "type": "array", "items": { "type": "object" } }
                },
                "required": ["available", "stale", "blocked", "in_review"]
            }
        },
        "required": ["ok", "action", "context", "total", "by_status", "lifecycle", "available", "queues"]
    })
}

fn mcp_diagnostics_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ok": { "type": "boolean" },
            "action": { "type": "string" },
            "context": context_output_schema(),
            "diagnostics": {
                "type": "object",
                "properties": {
                    "status": { "type": "string" },
                    "project_root": { "type": "string" },
                    "board_path": { "type": "string" },
                    "board_exists": { "type": "boolean" },
                    "process_cwd": { "type": ["string", "null"] },
                    "mcp_binary_path": { "type": "string" },
                    "mcp_binary_exists": { "type": "boolean" },
                    "claude": {
                        "type": "object",
                        "properties": {
                            "config_path": { "type": "string" },
                            "configured": { "type": "boolean" },
                            "status": { "type": "string" },
                            "detail": { "type": "string" },
                            "bound_project_root": { "type": ["string", "null"] },
                            "command": { "type": ["string", "null"] },
                            "args": { "type": "array", "items": { "type": "string" } }
                        },
                        "required": ["config_path", "configured", "status", "detail", "bound_project_root", "command", "args"]
                    },
                    "codex": {
                        "type": "object",
                        "properties": {
                            "config_root": { "type": ["string", "null"] },
                            "config_path": { "type": ["string", "null"] },
                            "configured": { "type": "boolean" },
                            "status": { "type": "string" },
                            "detail": { "type": "string" },
                            "bound_project_root": { "type": ["string", "null"] },
                            "command": { "type": ["string", "null"] },
                            "args": { "type": "array", "items": { "type": "string" } }
                        },
                        "required": ["config_root", "config_path", "configured", "status", "detail", "bound_project_root", "command", "args"]
                    }
                },
                "required": [
                    "status",
                    "project_root",
                    "board_path",
                    "board_exists",
                    "process_cwd",
                    "mcp_binary_path",
                    "mcp_binary_exists",
                    "claude",
                    "codex"
                ]
            }
        },
        "required": ["ok", "action", "context", "diagnostics"]
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
        .filter(|task| board_service::has_fresh_active_lease(task, now))
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
                && !board_service::has_fresh_active_lease(task, now)
        })
        .take(5)
        .collect();
    let stale_tasks: Vec<&Task> = board
        .tasks
        .iter()
        .filter(|task| task.assignee.is_some() && board_service::task_is_stale(task, now))
        .take(5)
        .collect();
    let blocked_tasks: Vec<&Task> = board
        .tasks
        .iter()
        .filter(|task| task.blocked.is_some())
        .take(5)
        .collect();
    let in_review_tasks: Vec<&Task> = board
        .tasks
        .iter()
        .filter(|task| task.status == TaskStatus::InReview)
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
        "available": available.clone(),
        "queues": {
            "available": available,
            "stale": stale_tasks,
            "blocked": blocked_tasks,
            "in_review": in_review_tasks,
        },
    })
}

pub fn board_summary_text(board: &Board) -> String {
    let now = now_epoch_secs();
    let active = board
        .tasks
        .iter()
        .filter(|task| board_service::has_fresh_active_lease(task, now))
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

pub fn board_activity_value(board: &Board) -> Value {
    let mut events = Vec::new();
    for task in &board.tasks {
        events.push(json!({
            "kind": "task_status",
            "task_id": task.id,
            "task_title": task.title,
            "status": task.status.wire_id(),
            "at": task.updated_at,
            "assignee": task.assignee,
        }));
        if let Some(blocked) = task.blocked.as_ref() {
            events.push(json!({
                "kind": "blocked",
                "task_id": task.id,
                "task_title": task.title,
                "reason": blocked.reason,
                "category": blocked.category,
                "author": blocked.author,
                "at": blocked.blocked_at,
            }));
        }
        if let Some(paused) = task.paused.as_ref() {
            events.push(json!({
                "kind": "paused",
                "task_id": task.id,
                "task_title": task.title,
                "reason": paused.reason,
                "author": paused.author,
                "at": paused.paused_at,
            }));
        }
        for note in &task.notes {
            events.push(json!({
                "kind": "note",
                "task_id": task.id,
                "task_title": task.title,
                "author": note.author,
                "text": note.text,
                "at": note.created_at,
            }));
        }
        for entry in &task.knowledge {
            events.push(json!({
                "kind": "knowledge",
                "task_id": task.id,
                "task_title": task.title,
                "title": entry.title,
                "source": entry.source,
                "category": entry.category,
                "author": entry.author,
                "at": entry.created_at,
            }));
        }
        if let Some(started_at) = task.review.last_started_at {
            events.push(json!({
                "kind": "review_started",
                "task_id": task.id,
                "task_title": task.title,
                "reviewer": task.review.last_reviewer,
                "attempts": task.review.attempts,
                "error": task.review.last_error,
                "at": started_at,
            }));
        }
    }
    events.sort_by(|left, right| {
        right["at"]
            .as_u64()
            .unwrap_or_default()
            .cmp(&left["at"].as_u64().unwrap_or_default())
    });
    json!({
        "ok": true,
        "action": "board_activity",
        "events": events,
    })
}

pub fn mcp_context_value(project_root: &Path) -> Value {
    json!({
        "project_root": project_root.display().to_string(),
        "board_path": board_store::board_path(project_root).display().to_string(),
        "process_cwd": std::env::current_dir().ok().map(|path| path.display().to_string()),
    })
}

fn with_context(mut value: Value, project_root: &Path) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("context".to_string(), mcp_context_value(project_root));
    }
    value
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

pub fn task_handoff_markdown(task: &Task) -> String {
    let mut text = format!(
        "# Handoff: {}\n\n- id: `{}`\n- status: `{}`\n",
        task.title,
        task.id,
        task.status.wire_id()
    );
    if let Some(assignee) = task.assignee.as_deref() {
        text.push_str(&format!("- assignee: `{assignee}`\n"));
    }
    if !task.description.trim().is_empty() {
        text.push_str("\n## Scope\n\n");
        text.push_str(task.description.trim());
        text.push('\n');
    }
    let handoff_notes: Vec<_> = task
        .notes
        .iter()
        .filter(|note| {
            let text = note.text.to_ascii_lowercase();
            text.contains("handoff")
                || text.contains("changed files")
                || text.contains("review verdict")
                || text.contains("risks")
                || text.contains("tests")
        })
        .collect();
    if !handoff_notes.is_empty() {
        text.push_str("\n## Handoff and review notes\n\n");
        for note in handoff_notes {
            let author = note.author.as_deref().unwrap_or("unknown");
            text.push_str(&format!(
                "### {} by {}\n\n{}\n\n",
                note.created_at, author, note.text
            ));
        }
    } else if !task.notes.is_empty() {
        text.push_str("\n## Latest notes\n\n");
        for note in task.notes.iter().rev().take(3) {
            let author = note.author.as_deref().unwrap_or("unknown");
            text.push_str(&format!(
                "- [{}] {}: {}\n",
                note.created_at, author, note.text
            ));
        }
    }
    if !task.knowledge.is_empty() {
        text.push_str("\n## Knowledge to preserve\n\n");
        for entry in &task.knowledge {
            text.push_str(&format!("- **{}**: {}\n", entry.title, entry.content));
        }
    }
    if let Some(blocked) = task.blocked.as_ref() {
        text.push_str("\n## Current blocker\n\n");
        text.push_str(&blocked.reason);
        text.push('\n');
    }
    text
}

pub fn workflow_guide_markdown() -> String {
    [
        "# TerminalTiler Kanban MCP workflow",
        "",
        "0. Verify the `context.project_root` (or call `diagnose_mcp`) before mutating tasks; stop if it is not the project/worktree you intend to edit.",
        "1. Call `get_my_work` with your `assignee` before claiming anything new; resume owned active, stale, paused, or in-review tasks first.",
        "2. If there is nothing to resume, call `start_next_work` to atomically claim the first available unblocked To Do task. It returns `reason: no_available_task` and `task: null` when nothing is claimable.",
        "3. For a specific user-provided task id, call `start_work` (or legacy `claim_task`) with your `assignee` before editing.",
        "4. Send `heartbeat_task`, `add_task_note`, and `add_task_knowledge` while working.",
        "5. Call `ready_for_review` with `author`, `summary`, and optional `changed_files`, `tests`, `risks`, and `next_steps` when implementation is ready.",
        "6. Reviewers call `submit_review` with `verdict` and optional `severity`, `findings`, and `recommendation`; only call `complete_task` when explicitly closing the task.",
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

fn spawn_claimed_review(
    project_root: &Path,
    selection: Option<&review_dispatch::ReviewSelection>,
    message: &mut String,
) -> (Option<Value>, Option<Value>) {
    let Some(selection) = selection else {
        return (None, None);
    };
    match review_dispatch::spawn_headless_review(project_root, selection) {
        Ok(run) => {
            message.push_str(&format!(
                " Started {} headless review (pid {}, log {}).",
                selection.reviewer.label(),
                run.pid,
                run.log_path.display()
            ));
            (
                Some(json!({
                    "reviewer": selection.reviewer.assignee_id(),
                    "pid": run.pid,
                    "log_path": run.log_path.display().to_string()
                })),
                None,
            )
        }
        Err(error) => {
            message.push_str(&format!(
                " Could not start headless review for {}: {error}",
                selection.reviewer.label()
            ));
            let _ = review_dispatch::record_review_spawn_failure(
                project_root,
                &selection.task.id,
                selection.reviewer,
                &error,
            );
            (
                None,
                Some(json!({
                    "reviewer": selection.reviewer.assignee_id(),
                    "message": error,
                })),
            )
        }
    }
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

fn optional_string_list(arguments: &Value, key: &str) -> Result<Vec<String>, ToolCallError> {
    let Some(value) = arguments.get(key) else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    if let Some(text) = value.as_str() {
        return Ok(split_text_list(text));
    }
    if let Some(values) = value.as_array() {
        let mut items = Vec::new();
        for value in values {
            let Some(text) = value.as_str() else {
                return Err(text_error(format!(
                    "parameter '{key}' must be a string or an array of strings"
                )));
            };
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                items.push(trimmed.to_string());
            }
        }
        return Ok(items);
    }
    Err(text_error(format!(
        "parameter '{key}' must be a string or an array of strings"
    )))
}

fn split_text_list(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.trim().trim_start_matches("- ").trim())
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect()
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

fn format_handoff_note(
    summary: &str,
    changed_files: &[String],
    tests: &[String],
    risks: &[String],
    next_steps: &[String],
) -> String {
    let has_extra = !changed_files.is_empty()
        || !tests.is_empty()
        || !risks.is_empty()
        || !next_steps.is_empty();
    if !has_extra {
        return summary.to_string();
    }
    let mut lines = vec!["Handoff summary:".to_string(), summary.trim().to_string()];
    append_markdown_list(&mut lines, "Changed files", changed_files);
    append_markdown_list(&mut lines, "Tests", tests);
    append_markdown_list(&mut lines, "Risks", risks);
    append_markdown_list(&mut lines, "Next steps", next_steps);
    lines.join("\n")
}

fn format_review_summary(
    summary: &str,
    severity: Option<&str>,
    findings: &[String],
    recommendation: Option<&str>,
) -> String {
    let mut lines = Vec::new();
    if let Some(severity) = severity.filter(|value| !value.trim().is_empty()) {
        lines.push(format!("Severity: {}", severity.trim()));
    }
    if !summary.trim().is_empty() {
        lines.push(summary.trim().to_string());
    }
    append_markdown_list(&mut lines, "Findings", findings);
    if let Some(recommendation) = recommendation.filter(|value| !value.trim().is_empty()) {
        lines.push(format!("Recommendation: {}", recommendation.trim()));
    }
    lines.join("\n")
}

fn append_markdown_list(lines: &mut Vec<String>, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    lines.push(format!("{title}:"));
    lines.extend(items.iter().map(|item| format!("- {item}")));
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

fn board_error_with_context(
    error: board_service::BoardError,
    project_root: &Path,
) -> ToolCallError {
    structured_error_with_context(board_error(error), project_root)
}

fn structured_error_with_context(mut error: ToolCallError, project_root: &Path) -> ToolCallError {
    if let Some(Value::Object(object)) = &mut error.structured {
        object
            .entry("context".to_string())
            .or_insert_with(|| mcp_context_value(project_root));
    }
    error
}

fn review_dispatch_error(error: review_dispatch::ReviewDispatchError) -> ToolCallError {
    match error {
        review_dispatch::ReviewDispatchError::Board(error) => board_error(error),
        review_dispatch::ReviewDispatchError::Storage(error) => text_error(error),
    }
}

fn review_dispatch_error_with_context(
    error: review_dispatch::ReviewDispatchError,
    project_root: &Path,
) -> ToolCallError {
    match error {
        review_dispatch::ReviewDispatchError::Board(error) => {
            board_error_with_context(error, project_root)
        }
        review_dispatch::ReviewDispatchError::Storage(error) => text_error(error),
    }
}

fn task_structured(action: &str, task: &Task, warnings: Vec<String>, project_root: &Path) -> Value {
    let now = now_epoch_secs();
    with_context(
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
        }),
        project_root,
    )
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
        assert!(summary_required.contains(&"queues"));
        assert!(!summary_required.contains(&"task_id"));
        assert!(!summary_required.contains(&"warnings"));
        assert_eq!(
            tool_definition(&tools, "get_board_summary")["title"],
            "Get Board Summary"
        );

        let my_work_schema = &tool_definition(&tools, "get_my_work")["outputSchema"];
        let my_work_required = required_fields(my_work_schema);
        assert!(my_work_required.contains(&"assignee"));
        assert!(my_work_required.contains(&"groups"));
        assert!(my_work_required.contains(&"counts"));

        let diagnostics_schema = &tool_definition(&tools, "diagnose_mcp")["outputSchema"];
        let diagnostics_required = required_fields(diagnostics_schema);
        assert!(diagnostics_required.contains(&"diagnostics"));
        assert!(!diagnostics_required.contains(&"task_id"));
        assert!(!diagnostics_required.contains(&"warnings"));

        let lifecycle_schema = &tool_definition(&tools, "start_work")["outputSchema"];
        let lifecycle_required = required_fields(lifecycle_schema);
        assert!(lifecycle_required.contains(&"task_id"));
        assert!(lifecycle_required.contains(&"warnings"));
        assert!(lifecycle_schema["properties"]["review_error"].is_object());

        let next_schema = &tool_definition(&tools, "start_next_work")["outputSchema"];
        let next_required = required_fields(next_schema);
        assert!(next_required.contains(&"task_id"));
        assert!(next_schema["properties"]["reason"].is_object());
    }
}
