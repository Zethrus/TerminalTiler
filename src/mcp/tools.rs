//! MCP tool registry for the Kanban board.
//!
//! Each tool loads the per-project board, applies one `services::board` operation, saves
//! atomically, and returns a short text result. Tools operate on the board at
//! `project_root` (the agent's working directory).

use std::path::Path;

use serde_json::{Value, json};

use crate::model::board::{Task, TaskStatus};
use crate::services::{board as board_service, review_dispatch};
use crate::storage::board_store;

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
    ]
}

/// Execute a tool by name. `Ok` carries text for the user/agent; `Err` is a tool error
/// message (surfaced to the agent as an error result, not a transport failure).
pub fn call(name: &str, arguments: &Value, project_root: &Path) -> Result<String, String> {
    match name {
        "list_tasks" => list_tasks(arguments, project_root),
        "get_task" => get_task(arguments, project_root),
        "create_task" => create_task(arguments, project_root),
        "claim_task" => claim_task(arguments, project_root),
        "update_task_status" => update_task_status(arguments, project_root),
        "complete_task" => complete_task(arguments, project_root),
        "add_task_note" => add_task_note(arguments, project_root),
        other => Err(format!("unknown tool '{other}'")),
    }
}

fn list_tasks(arguments: &Value, project_root: &Path) -> Result<String, String> {
    let board = board_store::load(project_root);
    let tasks: Vec<&Task> = match optional_str(arguments, "status") {
        Some(raw) => board_service::tasks_by_status(&board, parse_status(raw)?),
        None => board.tasks.iter().collect(),
    };
    serde_json::to_string_pretty(&tasks).map_err(|error| error.to_string())
}

fn get_task(arguments: &Value, project_root: &Path) -> Result<String, String> {
    let id = require_str(arguments, "id")?;
    let board = board_store::load(project_root);
    let task =
        board_service::get_task(&board, id).ok_or_else(|| format!("no task with id '{id}'"))?;
    serde_json::to_string_pretty(task).map_err(|error| error.to_string())
}

fn create_task(arguments: &Value, project_root: &Path) -> Result<String, String> {
    let title = require_str(arguments, "title")?;
    let description = optional_str(arguments, "description").unwrap_or_default();
    let status = match optional_str(arguments, "status") {
        Some(raw) => parse_status(raw)?,
        None => TaskStatus::Todo,
    };
    let id = board_store::update(project_root, |board| {
        board_service::create_task(board, title, description, status)
            .id
            .clone()
    })
    .map_err(|error| error.to_string())?;
    Ok(format!("Created task {id} in {}.", status.column_title()))
}

fn claim_task(arguments: &Value, project_root: &Path) -> Result<String, String> {
    let id = require_str(arguments, "id")?;
    let assignee = optional_str(arguments, "assignee").unwrap_or("agent");
    board_store::update(project_root, |board| {
        board_service::claim_task(board, id, assignee).map(|_| ())
    })
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())?;
    Ok(format!("Claimed task {id} as '{assignee}' (In Progress)."))
}

fn update_task_status(arguments: &Value, project_root: &Path) -> Result<String, String> {
    let id = require_str(arguments, "id")?;
    let status = parse_status(require_str(arguments, "status")?)?;
    let review = review_dispatch::set_status_and_claim_auto_review(project_root, id, status)?;
    let mut message = format!("Moved task {id} to {}.", status.column_title());
    if let Some(selection) = review {
        match review_dispatch::spawn_headless_review(project_root, &selection) {
            Ok(run) => message.push_str(&format!(
                " Started {} headless review (pid {}, log {}).",
                selection.reviewer.label(),
                run.pid,
                run.log_path.display()
            )),
            Err(error) => message.push_str(&format!(
                " Could not start headless review for {}: {error}",
                selection.reviewer.label()
            )),
        }
    }
    Ok(message)
}

fn complete_task(arguments: &Value, project_root: &Path) -> Result<String, String> {
    let id = require_str(arguments, "id")?;
    let note = optional_str(arguments, "note").map(str::to_string);
    board_store::update(project_root, |board| {
        board_service::complete_task(board, id, note).map(|_| ())
    })
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())?;
    Ok(format!("Completed task {id}."))
}

fn add_task_note(arguments: &Value, project_root: &Path) -> Result<String, String> {
    let id = require_str(arguments, "id")?;
    let text = require_str(arguments, "text")?;
    let author = optional_str(arguments, "author").map(str::to_string);
    board_store::update(project_root, |board| {
        board_service::add_note(board, id, text, author).map(|_| ())
    })
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())?;
    Ok(format!("Added note to task {id}."))
}

fn parse_status(raw: &str) -> Result<TaskStatus, String> {
    TaskStatus::from_wire(raw).ok_or_else(|| format!("unknown status '{raw}'"))
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": input_schema })
}

fn require_str<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("missing required string parameter '{key}'"))
}

fn optional_str<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}
