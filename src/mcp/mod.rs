//! Synchronous MCP (Model Context Protocol) server for the Kanban board.
//!
//! The stdio transport is newline-delimited JSON-RPC over stdin/stdout, so the whole
//! server is a blocking read loop — no async runtime. The AI client (Claude/Codex) spawns
//! `terminaltiler-mcp` with `--project-root <path>` so this module owns protocol and
//! project-root binding, while [`tools`] owns the board operations.

pub mod tools;

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// MCP protocol revision this server implements.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
/// Server identity reported during `initialize`.
pub const SERVER_NAME: &str = "terminaltiler";

/// Guidance returned to the agent so it knows how to work the board.
const INSTRUCTIONS: &str = "\
This server exposes a TerminalTiler Kanban board for the current project. Workflow: call \
`list_tasks` with status \"todo\" to find work; `claim_task` to move a task to In Progress \
and record yourself as the assignee; use `add_task_note` to report progress as you go; and \
when implementation is ready, call `update_task_status` to move the task to \"in_review\" \
before completion so the board can trigger review. Use `complete_task` only when the user \
explicitly asks you to mark the task Complete. Always claim a task before starting and post \
a note or status update when finished so the user can follow along.";

/// Run the stdio server loop until stdin closes. This is the binary's entire job.
pub fn run_stdio() {
    let project_root = match resolve_project_root_from_args(std::env::args_os().skip(1)) {
        Ok(project_root) => project_root,
        Err(message) => {
            eprintln!("terminaltiler-mcp: {message}");
            return;
        }
    };
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF — client closed stdin.
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Some(response) = handle_request(trimmed, &project_root)
                    && (writeln!(writer, "{response}").is_err() || writer.flush().is_err())
                {
                    break;
                }
            }
            Err(error) => {
                eprintln!("terminaltiler-mcp: failed to read stdin: {error}");
                break;
            }
        }
    }
}

/// Resolve the project root configured for this MCP server invocation.
///
/// `--project-root <path>` pins the server to that canonical project. Omitting the flag
/// preserves the legacy behavior of serving the current working directory.
pub fn resolve_project_root_from_args<I, S>(args: I) -> Result<PathBuf, String>
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString>,
{
    let mut args = args.into_iter().map(Into::into);
    let mut project_root: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        if arg == "--project-root" {
            let value = args
                .next()
                .ok_or_else(|| "--project-root requires a path".to_string())?;
            if project_root.is_some() {
                return Err("--project-root may only be provided once".to_string());
            }
            project_root = Some(PathBuf::from(value));
        } else {
            return Err(format!("unknown argument '{}'", arg.to_string_lossy()));
        }
    }

    let root = match project_root {
        Some(path) => path,
        None => std::env::current_dir()
            .map_err(|error| format!("could not resolve current directory: {error}"))?,
    };

    if !root.is_dir() {
        return Err(format!(
            "project root '{}' does not exist or is not a directory",
            root.display()
        ));
    }
    root.canonicalize().map_err(|error| {
        format!(
            "could not canonicalize project root '{}': {error}",
            root.display()
        )
    })
}

/// Handle one JSON-RPC message line. Returns the serialized response, or `None` for
/// notifications (which have no id and expect no reply).
pub fn handle_request(line: &str, project_root: &Path) -> Option<String> {
    let message: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(error) => {
            return Some(error_response(
                Value::Null,
                -32700,
                &format!("parse error: {error}"),
            ));
        }
    };

    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str);
    let params = message.get("params").cloned().unwrap_or(Value::Null);

    match method {
        Some("initialize") => Some(success_response(id_or_null(id), initialize_result())),
        Some("ping") => Some(success_response(id_or_null(id), json!({}))),
        Some("tools/list") => Some(success_response(
            id_or_null(id),
            json!({ "tools": tools::list_json() }),
        )),
        Some("tools/call") => Some(success_response(
            id_or_null(id),
            tools_call_result(&params, project_root),
        )),
        // Notifications (no id, e.g. "notifications/initialized") need no response.
        Some(_) if id.is_none() => None,
        Some(other) => Some(error_response(
            id_or_null(id),
            -32601,
            &format!("method not found: {other}"),
        )),
        None => Some(error_response(id_or_null(id), -32600, "invalid request")),
    }
}

fn tools_call_result(params: &Value, project_root: &Path) -> Value {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);

    match tools::call(name, &arguments, project_root) {
        Ok(text) => json!({
            "content": [{ "type": "text", "text": text }],
            "isError": false
        }),
        Err(message) => json!({
            "content": [{ "type": "text", "text": message }],
            "isError": true
        }),
    }
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": SERVER_NAME,
            "title": "TerminalTiler Kanban",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": INSTRUCTIONS
    })
}

fn id_or_null(id: Option<Value>) -> Value {
    id.unwrap_or(Value::Null)
}

fn success_response(id: Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    fn temp_root() -> PathBuf {
        let path = std::env::temp_dir().join(format!("terminaltiler-mcp-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn call_tool(root: &Path, name: &str, arguments: Value) -> Value {
        let request = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        })
        .to_string();
        let response = handle_request(&request, root).expect("tools/call returns a response");
        let parsed: Value = serde_json::from_str(&response).unwrap();
        parsed["result"].clone()
    }

    #[test]
    fn project_root_arg_selects_canonical_directory() {
        let root = temp_root();
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();

        let resolved = resolve_project_root_from_args([
            std::ffi::OsString::from("--project-root"),
            nested.clone().into_os_string(),
        ])
        .unwrap();

        assert_eq!(resolved, nested.canonicalize().unwrap());
    }

    #[test]
    fn missing_project_root_arg_falls_back_to_cwd() {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        assert_eq!(
            resolve_project_root_from_args(std::iter::empty::<std::ffi::OsString>()).unwrap(),
            cwd
        );
    }

    #[test]
    fn invalid_project_root_arg_returns_clear_error() {
        let missing =
            std::env::temp_dir().join(format!("terminaltiler-missing-{}", Uuid::new_v4()));
        let error = resolve_project_root_from_args([
            std::ffi::OsString::from("--project-root"),
            missing.clone().into_os_string(),
        ])
        .unwrap_err();

        assert!(error.contains("project root"));
        assert!(error.contains(&missing.display().to_string()));
    }

    #[test]
    fn project_root_arg_requires_value() {
        let error = resolve_project_root_from_args([std::ffi::OsString::from("--project-root")])
            .unwrap_err();
        assert!(error.contains("requires a path"));
    }

    #[test]
    fn initialize_advertises_protocol_and_tools_capability() {
        let root = temp_root();
        let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }).to_string();
        let response: Value =
            serde_json::from_str(&handle_request(&request, &root).unwrap()).unwrap();
        assert_eq!(response["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert!(response["result"]["capabilities"]["tools"].is_object());
        let instructions = response["result"]["instructions"].as_str().unwrap();
        assert!(instructions.contains("update_task_status"));
        assert!(instructions.contains("in_review"));
        assert!(instructions.contains("before completion"));
    }

    #[test]
    fn notifications_get_no_response() {
        let root = temp_root();
        let request =
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string();
        assert!(handle_request(&request, &root).is_none());
    }

    #[test]
    fn tools_list_exposes_all_board_tools() {
        let root = temp_root();
        let request = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }).to_string();
        let response: Value =
            serde_json::from_str(&handle_request(&request, &root).unwrap()).unwrap();
        let names: Vec<&str> = response["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        for expected in [
            "list_tasks",
            "get_task",
            "create_task",
            "claim_task",
            "update_task_status",
            "complete_task",
            "add_task_note",
        ] {
            assert!(names.contains(&expected), "missing tool {expected}");
        }
    }

    #[test]
    fn create_then_list_reflects_persisted_task() {
        let root = temp_root();

        let created = call_tool(&root, "create_task", json!({ "title": "Wire MCP" }));
        assert_eq!(created["isError"], false);

        // Board file is written to disk.
        assert!(crate::storage::board_store::board_path(&root).exists());

        let listed = call_tool(&root, "list_tasks", json!({ "status": "todo" }));
        let text = listed["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Wire MCP"));
    }

    #[test]
    fn update_status_to_in_review_marks_one_headless_review_from_configured_root() {
        crate::services::review_dispatch::set_test_disable_headless_review_spawn(true);
        let root = temp_root();
        let created = call_tool(&root, "create_task", json!({ "title": "Review via MCP" }));
        assert_eq!(created["isError"], false);
        let task_id = crate::storage::board_store::load(&root).tasks[0].id.clone();

        let first = call_tool(
            &root,
            "update_task_status",
            json!({ "id": task_id, "status": "in_review" }),
        );
        assert_eq!(first["isError"], false);
        let text = first["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Started Claude headless review"));

        let second = call_tool(
            &root,
            "update_task_status",
            json!({ "id": task_id, "status": "in_review" }),
        );
        assert_eq!(second["isError"], false);
        let text = second["content"][0]["text"].as_str().unwrap();
        assert!(!text.contains("headless review"));

        let board = crate::storage::board_store::load(&root);
        assert!(crate::storage::board_store::board_path(&root).exists());
        let task = &board.tasks[0];
        assert_eq!(task.status, crate::model::board::TaskStatus::InReview);
        assert_eq!(task.review.attempts, 1);
        assert_eq!(
            task.review.last_reviewer,
            Some(crate::model::agent_run::AgentKind::Claude)
        );
        assert!(root.join(".mcp.json").exists());
        crate::services::review_dispatch::set_test_disable_headless_review_spawn(false);
    }

    #[test]
    fn concurrent_note_tools_preserve_every_update() {
        let root = std::sync::Arc::new(temp_root());
        let created = call_tool(&root, "create_task", json!({ "title": "Concurrent" }));
        assert_eq!(created["isError"], false);
        let task_id = crate::storage::board_store::load(&root).tasks[0].id.clone();

        let writers = 10;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(writers));
        let handles = (0..writers)
            .map(|index| {
                let root = root.clone();
                let task_id = task_id.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let result = tools::call(
                        "add_task_note",
                        &json!({
                            "id": task_id,
                            "text": format!("note-{index}"),
                            "author": "test"
                        }),
                        &root,
                    )
                    .unwrap();
                    assert!(result.contains("Added note"));
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().unwrap();
        }

        let board = crate::storage::board_store::load(&root);
        let task = &board.tasks[0];
        assert_eq!(task.notes.len(), writers);
        for index in 0..writers {
            assert!(
                task.notes
                    .iter()
                    .any(|note| note.text == format!("note-{index}")),
                "missing note-{index}"
            );
        }
    }

    #[test]
    fn unknown_tool_returns_tool_error_not_transport_error() {
        let root = temp_root();
        let result = call_tool(&root, "does_not_exist", json!({}));
        assert_eq!(result["isError"], true);
    }

    #[test]
    fn malformed_json_yields_parse_error() {
        let root = temp_root();
        let response: Value =
            serde_json::from_str(&handle_request("{ not json", &root).unwrap()).unwrap();
        assert_eq!(response["error"]["code"], -32700);
    }
}
