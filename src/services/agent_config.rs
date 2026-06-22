//! One-click wiring of the bundled `terminaltiler-mcp` server into AI agent configs.
//!
//! The MCP binary ships next to the app, so "connecting" an agent just means registering
//! that binary in the agent's MCP config. Both writers merge idempotently so existing
//! servers are preserved.
//!
//! - Claude Code: project-scoped `<project_root>/.mcp.json`.
//! - Codex: `~/.codex/config.toml`.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::storage::fs_utils::atomic_write_private;

/// Key used for our server in both agent configs.
const SERVER_KEY: &str = "terminaltiler";

/// File name of the bundled MCP server binary.
fn mcp_binary_file_name() -> &'static str {
    if cfg!(windows) {
        "terminaltiler-mcp.exe"
    } else {
        "terminaltiler-mcp"
    }
}

/// Resolve the bundled MCP binary: prefer the sibling of the running executable (the
/// packaged layout and the cargo target dir both satisfy this), else fall back to a bare
/// name resolved via `PATH`.
pub fn mcp_binary_path() -> PathBuf {
    let file = mcp_binary_file_name();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join(file);
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from(file)
}

/// Register the MCP server with Claude Code for a project. Returns the written path.
pub fn connect_claude(project_root: &Path) -> Result<PathBuf, String> {
    let path = project_root.join(".mcp.json");
    let binary = mcp_binary_path().to_string_lossy().into_owned();
    upsert_claude(&path, &binary, project_root)?;
    Ok(path)
}

/// Register the MCP server with Codex (`~/.codex/config.toml`). Returns the written path.
pub fn connect_codex(project_root: &Path) -> Result<PathBuf, String> {
    let home = directories::BaseDirs::new()
        .ok_or_else(|| "could not resolve your home directory".to_string())?
        .home_dir()
        .to_path_buf();
    let path = home.join(".codex").join("config.toml");
    let binary = mcp_binary_path().to_string_lossy().into_owned();
    upsert_codex(&path, &binary, project_root)?;
    Ok(path)
}

fn upsert_claude(path: &Path, binary: &str, project_root: &Path) -> Result<(), String> {
    let mut root = match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str::<Value>(&raw)
            .map_err(|error| format!("existing {} is not valid JSON: {error}", path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(error) => return Err(error.to_string()),
    };

    let object = root
        .as_object_mut()
        .ok_or_else(|| format!("existing {} is not a JSON object", path.display()))?;
    let servers = object
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| "'mcpServers' in .mcp.json is not an object".to_string())?;
    servers.insert(
        SERVER_KEY.to_string(),
        json!({ "command": binary, "args": ["--project-root", project_root.to_string_lossy()] }),
    );

    let serialized = serde_json::to_string_pretty(&root).map_err(|error| error.to_string())?;
    atomic_write_private(path, &serialized).map_err(|error| error.to_string())
}

fn upsert_codex(path: &Path, binary: &str, project_root: &Path) -> Result<(), String> {
    let mut document = match std::fs::read_to_string(path) {
        Ok(raw) => toml::from_str::<toml::Table>(&raw)
            .map_err(|error| format!("existing {} is not valid TOML: {error}", path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(error) => return Err(error.to_string()),
    };

    let servers = document
        .entry("mcp_servers".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| "'mcp_servers' in config.toml is not a table".to_string())?;

    let mut entry = toml::Table::new();
    entry.insert(
        "command".to_string(),
        toml::Value::String(binary.to_string()),
    );
    entry.insert(
        "args".to_string(),
        toml::Value::Array(vec![
            toml::Value::String("--project-root".to_string()),
            toml::Value::String(project_root.to_string_lossy().into_owned()),
        ]),
    );
    servers.insert(SERVER_KEY.to_string(), toml::Value::Table(entry));

    let serialized = toml::to_string_pretty(&document).map_err(|error| error.to_string())?;
    atomic_write_private(path, &serialized).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("terminaltiler-agentcfg-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn claude_config_is_created_and_idempotent() {
        let dir = temp_dir();
        let path = dir.join(".mcp.json");

        upsert_claude(&path, "/opt/tt/terminaltiler-mcp", &dir).unwrap();
        upsert_claude(&path, "/opt/tt/terminaltiler-mcp", &dir).unwrap();

        let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let servers = value["mcpServers"].as_object().unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(
            servers["terminaltiler"]["command"],
            "/opt/tt/terminaltiler-mcp"
        );
        assert_eq!(
            servers["terminaltiler"]["args"],
            json!(["--project-root", dir.to_string_lossy()])
        );
    }

    #[test]
    fn claude_merge_preserves_other_servers() {
        let dir = temp_dir();
        let path = dir.join(".mcp.json");
        fs::write(
            &path,
            r#"{ "mcpServers": { "other": { "command": "x" } } }"#,
        )
        .unwrap();

        upsert_claude(&path, "/opt/tt/terminaltiler-mcp", &dir).unwrap();

        let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let servers = value["mcpServers"].as_object().unwrap();
        assert!(servers.contains_key("other"));
        assert!(servers.contains_key("terminaltiler"));
    }

    #[test]
    fn codex_config_is_created_and_idempotent() {
        let dir = temp_dir();
        let path = dir.join("config.toml");

        upsert_codex(&path, "/opt/tt/terminaltiler-mcp", &dir).unwrap();
        upsert_codex(&path, "/opt/tt/terminaltiler-mcp", &dir).unwrap();

        let document: toml::Table = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let servers = document["mcp_servers"].as_table().unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(
            servers["terminaltiler"]["command"].as_str(),
            Some("/opt/tt/terminaltiler-mcp")
        );
        let args = servers["terminaltiler"]["args"].as_array().unwrap();
        assert_eq!(args[0].as_str(), Some("--project-root"));
        assert_eq!(args[1].as_str(), Some(dir.to_string_lossy().as_ref()));
    }
}
