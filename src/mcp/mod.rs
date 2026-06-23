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
pub const PROTOCOL_VERSION: &str = "2025-11-25";
/// Server identity reported during `initialize`.
pub const SERVER_NAME: &str = "terminaltiler";

/// Guidance returned to the agent so it knows how to work the board.
const INSTRUCTIONS: &str = "\
This server exposes a TerminalTiler Kanban board for the current project. Workflow: call \
`get_my_work` first to resume owned active, stale, paused, or in-review work; if you have \
no owned work to continue, call `start_next_work` to atomically claim the first available \
unblocked To Do task, or call `start_work` for an explicit task id with lifecycle-aware \
soft leases; use `heartbeat_task` plus `add_task_note` and \
`add_task_knowledge` to report progress and durable findings as you work; when \
implementation is ready, call `ready_for_review` with a handoff summary so the task moves \
to \"in_review\" and the duplicate-gated review path can run; reviewers should call \
`submit_review` with a verdict and leave completion manual. Existing tools such as \
`claim_task` and `update_task_status` remain available for compatibility, but lifecycle \
helpers are preferred. Use `complete_task` only when the user explicitly asks you to mark \
the task Complete. Before mutating tasks, inspect the structured `context.project_root` \
or `diagnose_mcp` output and verify it matches the project/worktree you intend to change.";

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
        Some("resources/list") => Some(success_response(
            id_or_null(id),
            resources_list_result(project_root),
        )),
        Some("resources/read") => match resources_read_result(&params, project_root) {
            Ok(result) => Some(success_response(id_or_null(id), result)),
            Err(message) => Some(error_response(id_or_null(id), -32602, &message)),
        },
        Some("prompts/list") => Some(success_response(id_or_null(id), prompts_list_result())),
        Some("prompts/get") => match prompts_get_result(&params, project_root) {
            Ok(result) => Some(success_response(id_or_null(id), result)),
            Err(message) => Some(error_response(id_or_null(id), -32602, &message)),
        },
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
        Ok(output) => {
            let mut result = json!({
                "content": [{ "type": "text", "text": output.text }],
                "isError": false
            });
            if let Some(structured) = output.structured {
                result["structuredContent"] = structured;
            }
            result
        }
        Err(error) => {
            let mut result = json!({
                "content": [{ "type": "text", "text": error.text }],
                "isError": true
            });
            if let Some(structured) = error.structured {
                result["structuredContent"] = structured;
            }
            result
        }
    }
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
        "serverInfo": {
            "name": SERVER_NAME,
            "title": "TerminalTiler Kanban",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": INSTRUCTIONS
    })
}

fn resources_list_result(project_root: &Path) -> Value {
    let board = crate::storage::board_store::load(project_root);
    let mut resources = vec![
        json!({
            "uri": "terminaltiler://project/context",
            "name": "Project context",
            "description": "Bound TerminalTiler project root, board path, and MCP process context.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "terminaltiler://board/summary",
            "name": "Board summary",
            "description": "Compact task and lifecycle counts for this TerminalTiler board.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "terminaltiler://board/activity",
            "name": "Board activity",
            "description": "Recent task notes, knowledge captures, blockers, reviews, and lifecycle updates derived from the board.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "terminaltiler://board/tasks",
            "name": "Task list",
            "description": "All board tasks as JSON.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "terminaltiler://workflow/guide",
            "name": "Workflow guide",
            "description": "Recommended MCP workflow and ownership guardrails.",
            "mimeType": "text/markdown"
        }),
    ];
    for task in &board.tasks {
        resources.push(json!({
            "uri": format!("terminaltiler://task/{}.json", task.id),
            "name": format!("Task JSON: {}", task.title),
            "description": "Full task JSON.",
            "mimeType": "application/json"
        }));
        resources.push(json!({
            "uri": format!("terminaltiler://task/{}.md", task.id),
            "name": format!("Task brief: {}", task.title),
            "description": "Markdown task brief.",
            "mimeType": "text/markdown"
        }));
        resources.push(json!({
            "uri": format!("terminaltiler://task/{}/handoff.md", task.id),
            "name": format!("Task handoff: {}", task.title),
            "description": "Markdown implementation/review handoff with notes and risks.",
            "mimeType": "text/markdown"
        }));
    }
    json!({ "resources": resources })
}

fn resources_read_result(params: &Value, project_root: &Path) -> Result<Value, String> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "resources/read requires a non-empty string parameter 'uri'".to_string())?;
    read_resource(uri, project_root).map(|(mime_type, text)| {
        json!({
            "contents": [{ "uri": uri, "mimeType": mime_type, "text": text }]
        })
    })
}

fn read_resource(uri: &str, project_root: &Path) -> Result<(&'static str, String), String> {
    let board = crate::storage::board_store::load(project_root);
    match uri {
        "terminaltiler://project/context" => {
            serde_json::to_string_pretty(&tools::mcp_context_value(project_root))
                .map(|text| ("application/json", text))
                .map_err(|error| error.to_string())
        }
        "terminaltiler://board/summary" => {
            let mut summary = tools::board_summary_value(&board);
            if let Some(object) = summary.as_object_mut() {
                object.insert(
                    "context".to_string(),
                    tools::mcp_context_value(project_root),
                );
            }
            serde_json::to_string_pretty(&summary)
                .map(|text| ("application/json", text))
                .map_err(|error| error.to_string())
        }
        "terminaltiler://board/activity" => {
            serde_json::to_string_pretty(&tools::board_activity_value(&board))
                .map(|text| ("application/json", text))
                .map_err(|error| error.to_string())
        }
        "terminaltiler://board/tasks" => serde_json::to_string_pretty(&board.tasks)
            .map(|text| ("application/json", text))
            .map_err(|error| error.to_string()),
        _ if uri.starts_with("terminaltiler://task/") && uri.ends_with("/handoff.md") => {
            let id = uri
                .trim_start_matches("terminaltiler://task/")
                .trim_end_matches("/handoff.md");
            let task = crate::services::board::get_task(&board, id)
                .ok_or_else(|| format!("no task with id '{id}'"))?;
            Ok(("text/markdown", tools::task_handoff_markdown(task)))
        }
        "terminaltiler://workflow/guide" => Ok(("text/markdown", tools::workflow_guide_markdown())),
        _ if uri.starts_with("terminaltiler://task/") && uri.ends_with(".json") => {
            let id = uri
                .trim_start_matches("terminaltiler://task/")
                .trim_end_matches(".json");
            let task = crate::services::board::get_task(&board, id)
                .ok_or_else(|| format!("no task with id '{id}'"))?;
            serde_json::to_string_pretty(task)
                .map(|text| ("application/json", text))
                .map_err(|error| error.to_string())
        }
        _ if uri.starts_with("terminaltiler://task/") && uri.ends_with(".md") => {
            let id = uri
                .trim_start_matches("terminaltiler://task/")
                .trim_end_matches(".md");
            let task = crate::services::board::get_task(&board, id)
                .ok_or_else(|| format!("no task with id '{id}'"))?;
            Ok(("text/markdown", tools::task_brief_markdown(task)))
        }
        _ => Err(format!("unknown resource URI '{uri}'")),
    }
}

fn prompts_list_result() -> Value {
    json!({
        "prompts": [
            {
                "name": "implement_task",
                "description": "Claim and implement a TerminalTiler Kanban task safely.",
                "arguments": [{ "name": "task_id", "description": "Task id to implement.", "required": true }]
            },
            {
                "name": "work_next_task",
                "description": "Resume owned work if present, otherwise atomically claim the next available task.",
                "arguments": [{ "name": "assignee", "description": "Assignee id to use; defaults to agent.", "required": false }]
            },
            {
                "name": "review_task",
                "description": "Review a TerminalTiler Kanban task and submit a verdict.",
                "arguments": [{ "name": "task_id", "description": "Task id to review.", "required": true }]
            },
            {
                "name": "triage_board",
                "description": "Summarize board state, blockers, and recommended next tasks.",
                "arguments": []
            }
        ]
    })
}

fn prompts_get_result(params: &Value, project_root: &Path) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "prompts/get requires a non-empty string parameter 'name'".to_string())?;
    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
    let task_id = arguments
        .get("task_id")
        .and_then(Value::as_str)
        .unwrap_or("<task-id>");
    let assignee = arguments
        .get("assignee")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("agent");
    let text = match name {
        "implement_task" => format!(
            "{}\n\nImplement task `{task_id}`: first call `get_my_work` for resume context, read `terminaltiler://task/{task_id}.md`, call `start_work` with your assignee, update notes/knowledge as you work, then call `ready_for_review` with your author id and a concise handoff summary.",
            tools::workflow_guide_markdown()
        ),
        "work_next_task" => format!(
            "{}\n\nWork the next task as `{assignee}`: call `get_my_work` with assignee `{assignee}` and resume any active/stale/paused work before starting new work. If there is nothing to resume, call `start_next_work` with assignee `{assignee}`. If it returns `reason: no_available_task`, report that no unblocked To Do task is currently claimable. Do not call `complete_task` unless the user explicitly asks.",
            tools::workflow_guide_markdown()
        ),
        "review_task" => format!(
            "Review task `{task_id}` using `terminaltiler://task/{task_id}.md`. Inspect the project root `{}`. Call `submit_review` with your reviewer author id, verdict, and severity-rated summary. Do not call `complete_task`.",
            project_root.display()
        ),
        "triage_board" => {
            "Call `get_board_summary` and `get_my_work`, inspect blocked/stale/in_review counts, then recommend the next available tasks and any ownership conflicts to resolve.".to_string()
        }
        _ => return Err(format!("unknown TerminalTiler prompt '{name}'")),
    };
    Ok(json!({
        "description": name,
        "messages": [{
            "role": "user",
            "content": { "type": "text", "text": text }
        }]
    }))
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
        // The stdio entry point canonicalizes --project-root before serving requests.
        // Mirror that here so Windows CI does not compare 8.3 Temp aliases with long paths.
        path.canonicalize().unwrap()
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

    fn output_schema_required_fields(tool_name: &str) -> Vec<String> {
        tools::list_json()
            .into_iter()
            .find(|tool| tool["name"] == tool_name)
            .unwrap_or_else(|| panic!("missing tool definition for {tool_name}"))["outputSchema"]
            ["required"]
            .as_array()
            .unwrap_or_else(|| panic!("missing outputSchema.required for {tool_name}"))
            .iter()
            .map(|field| field.as_str().unwrap().to_string())
            .collect()
    }

    fn assert_structured_has_required_fields(tool_name: &str, structured: &Value) {
        let object = structured
            .as_object()
            .unwrap_or_else(|| panic!("{tool_name} structured content must be an object"));
        for field in output_schema_required_fields(tool_name) {
            assert!(
                object.contains_key(&field),
                "{tool_name} structured content is missing required field {field}"
            );
        }
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
        assert!(response["result"]["capabilities"]["resources"].is_object());
        assert!(response["result"]["capabilities"]["prompts"].is_object());
        let instructions = response["result"]["instructions"].as_str().unwrap();
        assert!(instructions.contains("get_my_work"));
        assert!(instructions.contains("start_next_work"));
        assert!(instructions.contains("start_work"));
        assert!(instructions.contains("ready_for_review"));
        assert!(instructions.contains("submit_review"));
        assert!(instructions.contains("in_review"));
        assert!(instructions.contains("complete_task"));
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
            "get_board_summary",
            "get_my_work",
            "get_task",
            "get_task_brief",
            "diagnose_mcp",
            "create_task",
            "claim_task",
            "update_task_status",
            "complete_task",
            "add_task_note",
            "add_task_knowledge",
            "start_work",
            "start_next_work",
            "heartbeat_task",
            "pause_work",
            "release_task",
            "reassign_task",
            "block_task",
            "unblock_task",
            "ready_for_review",
            "submit_review",
        ] {
            assert!(names.contains(&expected), "missing tool {expected}");
        }
        let start_work = response["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "start_work")
            .unwrap();
        assert_eq!(start_work["title"], "Start Work");
        assert!(start_work["outputSchema"].is_object());
        assert!(
            response["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .all(|tool| tool["outputSchema"].is_object())
        );
    }

    #[test]
    fn resources_and_prompts_are_advertised_and_readable() {
        let root = temp_root();
        call_tool(&root, "create_task", json!({ "title": "Resource task" }));
        let task_id = crate::storage::board_store::load(&root).tasks[0].id.clone();

        let list_request =
            json!({ "jsonrpc": "2.0", "id": 3, "method": "resources/list" }).to_string();
        let listed: Value =
            serde_json::from_str(&handle_request(&list_request, &root).unwrap()).unwrap();
        let resources = listed["result"]["resources"].as_array().unwrap();
        assert!(
            resources
                .iter()
                .any(|resource| resource["uri"] == "terminaltiler://board/summary")
        );
        assert!(
            resources
                .iter()
                .any(|resource| resource["uri"] == "terminaltiler://project/context")
        );
        assert!(
            resources
                .iter()
                .any(|resource| resource["uri"] == "terminaltiler://board/activity")
        );
        assert!(
            resources
                .iter()
                .any(|resource| resource["uri"] == format!("terminaltiler://task/{task_id}.md"))
        );
        assert!(resources.iter().any(
            |resource| resource["uri"] == format!("terminaltiler://task/{task_id}/handoff.md")
        ));

        let read_request = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "resources/read",
            "params": { "uri": format!("terminaltiler://task/{task_id}.md") }
        })
        .to_string();
        let read: Value =
            serde_json::from_str(&handle_request(&read_request, &root).unwrap()).unwrap();
        assert!(
            read["result"]["contents"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Resource task")
        );

        let prompts_request =
            json!({ "jsonrpc": "2.0", "id": 5, "method": "prompts/list" }).to_string();
        let prompts: Value =
            serde_json::from_str(&handle_request(&prompts_request, &root).unwrap()).unwrap();
        let names: Vec<&str> = prompts["result"]["prompts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|prompt| prompt["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"implement_task"));
        assert!(names.contains(&"work_next_task"));
        assert!(names.contains(&"review_task"));
        assert!(names.contains(&"triage_board"));

        let prompt_request = json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "prompts/get",
            "params": { "name": "implement_task", "arguments": { "task_id": task_id } }
        })
        .to_string();
        let prompt: Value =
            serde_json::from_str(&handle_request(&prompt_request, &root).unwrap()).unwrap();
        assert!(
            prompt["result"]["messages"][0]["content"]["text"]
                .as_str()
                .unwrap()
                .contains("ready_for_review")
        );

        let next_prompt_request = json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "prompts/get",
            "params": { "name": "work_next_task", "arguments": { "assignee": "codex" } }
        })
        .to_string();
        let next_prompt: Value =
            serde_json::from_str(&handle_request(&next_prompt_request, &root).unwrap()).unwrap();
        let next_prompt_text = next_prompt["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap();
        assert!(next_prompt_text.contains("get_my_work"));
        assert!(next_prompt_text.contains("start_next_work"));
        assert!(next_prompt_text.contains("codex"));
    }

    #[test]
    fn unknown_resources_and_prompts_return_invalid_params() {
        let root = temp_root();
        let resource_request = json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "resources/read",
            "params": { "uri": "terminaltiler://missing" }
        })
        .to_string();
        let resource: Value =
            serde_json::from_str(&handle_request(&resource_request, &root).unwrap()).unwrap();
        assert_eq!(resource["error"]["code"], -32602);

        let prompt_request = json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "prompts/get",
            "params": { "name": "missing_prompt" }
        })
        .to_string();
        let prompt: Value =
            serde_json::from_str(&handle_request(&prompt_request, &root).unwrap()).unwrap();
        assert_eq!(prompt["error"]["code"], -32602);
    }

    #[test]
    fn create_then_list_reflects_persisted_task() {
        let root = temp_root();

        let created = call_tool(&root, "create_task", json!({ "title": "Wire MCP" }));
        assert_eq!(created["isError"], false);

        // Board file is written to disk.
        assert!(crate::storage::board_store::board_path(&root).exists());

        let listed = call_tool(&root, "list_tasks", json!({ "status": "todo" }));
        assert_eq!(
            listed["structuredContent"]["context"]["project_root"],
            root.display().to_string()
        );
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
    }

    #[test]
    fn lifecycle_tools_return_structured_content_and_conflicts() {
        let root = temp_root();
        call_tool(
            &root,
            "create_task",
            json!({ "title": "Lifecycle via MCP" }),
        );
        let task_id = crate::storage::board_store::load(&root).tasks[0].id.clone();

        let started = call_tool(
            &root,
            "start_work",
            json!({ "id": task_id, "assignee": "alice", "stale_after_secs": 60 }),
        );
        assert_eq!(started["isError"], false);
        assert_eq!(started["structuredContent"]["ok"], true);
        assert_eq!(started["structuredContent"]["task_id"], task_id);
        assert_eq!(
            started["structuredContent"]["lifecycle"]["assignee"],
            "alice"
        );

        let conflict = call_tool(
            &root,
            "start_work",
            json!({ "id": task_id, "assignee": "bob" }),
        );
        assert_eq!(conflict["isError"], true);
        assert_eq!(conflict["structuredContent"]["ok"], false);
        assert_structured_has_required_fields("start_work", &conflict["structuredContent"]);
        assert_eq!(
            conflict["structuredContent"]["context"]["project_root"]
                .as_str()
                .unwrap(),
            root.canonicalize().unwrap().to_string_lossy().as_ref()
        );
        assert_eq!(
            conflict["structuredContent"]["conflict"]["current_assignee"],
            "alice"
        );

        let heartbeat = call_tool(
            &root,
            "heartbeat_task",
            json!({ "id": task_id, "assignee": "alice", "note": "progress" }),
        );
        assert_eq!(heartbeat["isError"], false);
        assert_eq!(heartbeat["structuredContent"]["action"], "heartbeat_task");
    }

    #[test]
    fn start_next_work_claims_first_available_and_reports_no_task() {
        let root = temp_root();
        call_tool(&root, "create_task", json!({ "title": "Blocked first" }));
        call_tool(
            &root,
            "create_task",
            json!({ "title": "Fresh lease second" }),
        );
        call_tool(&root, "create_task", json!({ "title": "Available third" }));
        let ids: Vec<String> = crate::storage::board_store::load(&root)
            .tasks
            .iter()
            .map(|task| task.id.clone())
            .collect();

        let blocked = call_tool(
            &root,
            "block_task",
            json!({ "id": ids[0].clone(), "reason": "blocked" }),
        );
        assert_eq!(blocked["isError"], false);
        let leased = call_tool(
            &root,
            "start_work",
            json!({ "id": ids[1].clone(), "assignee": "alice", "stale_after_secs": 60 }),
        );
        assert_eq!(leased["isError"], false);
        crate::storage::board_store::update(&root, |board| {
            board.tasks[1].status = crate::model::board::TaskStatus::Todo;
        })
        .unwrap();

        let claimed = call_tool(&root, "start_next_work", json!({ "assignee": "bob" }));
        assert_eq!(claimed["isError"], false);
        assert_eq!(claimed["structuredContent"]["action"], "start_next_work");
        assert_eq!(claimed["structuredContent"]["task_id"], ids[2]);
        assert_eq!(claimed["structuredContent"]["task"]["assignee"], "bob");

        let my_work = call_tool(&root, "get_my_work", json!({ "assignee": "bob" }));
        assert_eq!(my_work["isError"], false);
        assert_eq!(my_work["structuredContent"]["counts"]["active"], 1);
        assert_eq!(
            my_work["structuredContent"]["groups"]["active"][0]["id"],
            ids[2]
        );

        let none = call_tool(&root, "start_next_work", json!({ "assignee": "carol" }));
        assert_eq!(none["isError"], false);
        assert_eq!(none["structuredContent"]["task"], Value::Null);
        assert_eq!(none["structuredContent"]["reason"], "no_available_task");
    }

    #[test]
    fn concurrent_start_next_work_claims_distinct_tasks() {
        let root = std::sync::Arc::new(temp_root());
        for title in ["One", "Two", "Three"] {
            call_tool(&root, "create_task", json!({ "title": title }));
        }

        let workers = 2;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(workers));
        let handles = (0..workers)
            .map(|index| {
                let root = root.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let output = tools::call(
                        "start_next_work",
                        &json!({ "assignee": format!("agent-{index}") }),
                        &root,
                    )
                    .unwrap();
                    output.structured.unwrap()["task_id"]
                        .as_str()
                        .unwrap()
                        .to_string()
                })
            })
            .collect::<Vec<_>>();

        let claimed_ids = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(claimed_ids.len(), workers);
        let board = crate::storage::board_store::load(&root);
        assert_eq!(
            board
                .tasks
                .iter()
                .filter(|task| task.status == crate::model::board::TaskStatus::InProgress)
                .count(),
            workers
        );
    }

    #[test]
    fn legacy_mcp_paths_enforce_ownership_with_force_override() {
        crate::services::review_dispatch::set_test_disable_headless_review_spawn(true);
        let root = temp_root();
        call_tool(&root, "create_task", json!({ "title": "Guarded" }));
        let task_id = crate::storage::board_store::load(&root).tasks[0].id.clone();
        call_tool(
            &root,
            "claim_task",
            json!({ "id": task_id, "assignee": "alice" }),
        );

        let competing_claim = call_tool(
            &root,
            "claim_task",
            json!({ "id": task_id, "assignee": "bob" }),
        );
        assert_eq!(competing_claim["isError"], true);
        assert_eq!(
            competing_claim["structuredContent"]["action"],
            "ownership_conflict"
        );

        let competing_review = call_tool(
            &root,
            "ready_for_review",
            json!({ "id": task_id, "summary": "done", "author": "bob" }),
        );
        assert_eq!(competing_review["isError"], true);
        assert_eq!(
            competing_review["structuredContent"]["conflict"]["current_assignee"],
            "alice"
        );
        assert_structured_has_required_fields(
            "ready_for_review",
            &competing_review["structuredContent"],
        );
        assert_eq!(
            competing_review["structuredContent"]["context"]["project_root"]
                .as_str()
                .unwrap(),
            root.canonicalize().unwrap().to_string_lossy().as_ref()
        );

        let forced = call_tool(
            &root,
            "complete_task",
            json!({ "id": task_id, "author": "bob", "force": true, "note": "forced close" }),
        );
        assert_eq!(forced["isError"], false);
        assert!(
            forced["structuredContent"]["warnings"][0]
                .as_str()
                .unwrap()
                .contains("force took over")
        );
    }

    #[test]
    fn ready_for_review_triggers_one_review_and_submit_review_stays_in_review() {
        crate::services::review_dispatch::set_test_disable_headless_review_spawn(true);
        let root = temp_root();
        call_tool(&root, "create_task", json!({ "title": "Ready helper" }));
        let task_id = crate::storage::board_store::load(&root).tasks[0].id.clone();
        call_tool(
            &root,
            "start_work",
            json!({ "id": task_id, "assignee": "codex" }),
        );

        let ready = call_tool(
            &root,
            "ready_for_review",
            json!({ "id": task_id, "summary": "Implemented", "author": "codex" }),
        );
        assert_eq!(ready["isError"], false);
        assert_eq!(ready["structuredContent"]["action"], "ready_for_review");
        assert_eq!(
            ready["structuredContent"]["handoff"]["summary"],
            "Implemented"
        );
        assert_eq!(
            ready["structuredContent"]["context"]["board_path"],
            crate::storage::board_store::board_path(&root)
                .display()
                .to_string()
        );
        assert_eq!(
            ready["structuredContent"]["task"]["status"],
            crate::model::board::TaskStatus::InReview.wire_id()
        );
        assert_eq!(
            ready["structuredContent"]["task"]["claimed_at"],
            Value::Null
        );
        assert!(ready["structuredContent"]["review_started"].is_object());

        let second = call_tool(
            &root,
            "ready_for_review",
            json!({ "id": task_id, "summary": "Still ready", "author": "codex" }),
        );
        assert_eq!(second["isError"], false);
        assert_eq!(second["structuredContent"]["review_started"], Value::Null);

        let reviewed = call_tool(
            &root,
            "submit_review",
            json!({
                "id": task_id,
                "verdict": "changes_requested",
                "summary": "Fix edge case",
                "severity": "medium",
                "findings": ["Missing edge case"],
                "recommendation": "Patch before completion",
                "author": "codex-reviewer"
            }),
        );
        assert_eq!(reviewed["isError"], false);
        assert_eq!(
            reviewed["structuredContent"]["review"]["severity"],
            "medium"
        );
        assert_eq!(
            reviewed["structuredContent"]["task"]["status"],
            crate::model::board::TaskStatus::InReview.wire_id()
        );

        let board = crate::storage::board_store::load(&root);
        let task = &board.tasks[0];
        assert_eq!(task.status, crate::model::board::TaskStatus::InReview);
        assert_eq!(task.review.attempts, 1);
        assert!(task.latest_note().unwrap().contains("changes_requested"));
    }

    #[test]
    fn review_spawn_failures_are_structured_and_visible_on_task() {
        crate::services::review_dispatch::set_test_headless_review_spawn_error(Some(
            "synthetic spawn failure",
        ));
        let root = temp_root();
        call_tool(&root, "create_task", json!({ "title": "Review failure" }));
        let task_id = crate::storage::board_store::load(&root).tasks[0].id.clone();
        call_tool(
            &root,
            "start_work",
            json!({ "id": task_id, "assignee": "codex" }),
        );

        let ready = call_tool(
            &root,
            "ready_for_review",
            json!({ "id": task_id, "summary": "Implemented", "author": "codex" }),
        );
        crate::services::review_dispatch::set_test_headless_review_spawn_error(None);

        assert_eq!(ready["isError"], false);
        assert_eq!(ready["structuredContent"]["review_started"], Value::Null);
        assert_eq!(
            ready["structuredContent"]["review_error"]["message"],
            "synthetic spawn failure"
        );
        let board = crate::storage::board_store::load(&root);
        let task = &board.tasks[0];
        assert_eq!(
            task.review.last_error.as_deref(),
            Some("synthetic spawn failure")
        );
        assert!(
            task.latest_note()
                .unwrap()
                .contains("Review launch failed for")
        );
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
                    assert!(result.text.contains("Added note"));
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
