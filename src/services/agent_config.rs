//! One-click wiring of the bundled `terminaltiler-mcp` server into AI agent configs.
//!
//! The MCP binary ships next to the app, so "connecting" an agent just means registering
//! that binary in the agent's MCP config. Both writers merge idempotently so existing
//! servers are preserved.
//!
//! - Claude Code: project-scoped `<project_root>/.mcp.json`.
//! - Codex: `~/.codex/config.toml`.

use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

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
///
/// Under an AppImage the executable lives on an ephemeral mount that vanishes when the app
/// exits, so writing that path into an agent config would dangle. The MCP binary is
/// GTK-free and self-contained, so in that case we copy it to a stable per-user location
/// and hand that path out instead.
pub fn mcp_binary_path() -> PathBuf {
    let file = mcp_binary_file_name();

    if is_appimage_runtime()
        && let Some(stable) = ensure_stable_mcp_binary()
    {
        return stable;
    }

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

/// Whether the app is running from an AppImage (its runtime sets `APPIMAGE`).
fn is_appimage_runtime() -> bool {
    std::env::var_os("APPIMAGE").is_some()
}

/// Copy the bundled MCP binary (sibling of the running executable) into a stable per-user
/// directory so the path survives app restarts and AppImage remounts. Returns the stable
/// path, or `None` if the source or destination can't be resolved.
fn ensure_stable_mcp_binary() -> Option<PathBuf> {
    let file = mcp_binary_file_name();
    let source = std::env::current_exe().ok()?.parent()?.join(file);
    if !source.exists() {
        return None;
    }
    let dest_dir = crate::app_paths::data_dir()?.join("bin");
    install_stable_binary(&source, &dest_dir).ok()
}

/// Install `source` into `dest_dir` (keeping its file name), copying only when the
/// destination is missing or stale. The copy is atomic (temp file + rename) and the result
/// is marked executable. Idempotent.
fn install_stable_binary(source: &Path, dest_dir: &Path) -> io::Result<PathBuf> {
    let file_name = source
        .file_name()
        .ok_or_else(|| io::Error::other("source binary has no file name"))?;
    let dest = dest_dir.join(file_name);

    if needs_refresh(source, &dest) {
        std::fs::create_dir_all(dest_dir)?;
        let temp = dest_dir.join(format!(
            ".{}.tmp-{}",
            file_name.to_string_lossy(),
            std::process::id()
        ));
        std::fs::copy(source, &temp)?;
        set_executable(&temp)?;
        std::fs::rename(&temp, &dest)?;
    }
    Ok(dest)
}

/// Whether `dest` must be (re)written from `source`: missing or different contents.
fn needs_refresh(source: &Path, dest: &Path) -> bool {
    let Ok(source_meta) = std::fs::metadata(source) else {
        return true; // Let the copy attempt surface the real source error.
    };
    let Ok(dest_meta) = std::fs::metadata(dest) else {
        return true; // Destination missing.
    };
    if source_meta.len() != dest_meta.len() {
        return true;
    }
    match files_equal(source, dest) {
        Ok(equal) => !equal,
        Err(_) => true,
    }
}

/// Compare two same-sized binaries without relying on mtimes.
fn files_equal(left: &Path, right: &Path) -> io::Result<bool> {
    const BUFFER_SIZE: usize = 8 * 1024;

    let mut left = std::fs::File::open(left)?;
    let mut right = std::fs::File::open(right)?;
    let mut left_buf = [0_u8; BUFFER_SIZE];
    let mut right_buf = [0_u8; BUFFER_SIZE];

    loop {
        let left_len = left.read(&mut left_buf)?;
        let right_len = right.read(&mut right_buf)?;
        if left_len != right_len {
            return Ok(false);
        }
        if left_len == 0 {
            return Ok(true);
        }
        if left_buf[..left_len] != right_buf[..right_len] {
            return Ok(false);
        }
    }
}

#[cfg(unix)]
fn set_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> io::Result<()> {
    Ok(())
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

/// Snapshot of MCP setup health for UI panels and the `diagnose_mcp` tool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpDiagnostics {
    pub project_root: PathBuf,
    pub board_path: PathBuf,
    pub board_exists: bool,
    pub mcp_binary_path: PathBuf,
    pub mcp_binary_exists: bool,
    pub claude_config_path: PathBuf,
    pub claude_configured: bool,
    pub claude_detail: String,
    pub codex_config_path: Option<PathBuf>,
    pub codex_configured: bool,
    pub codex_detail: String,
}

impl McpDiagnostics {
    pub fn to_json(&self) -> Value {
        json!({
            "project_root": self.project_root.display().to_string(),
            "board_path": self.board_path.display().to_string(),
            "board_exists": self.board_exists,
            "mcp_binary_path": self.mcp_binary_path.display().to_string(),
            "mcp_binary_exists": self.mcp_binary_exists,
            "claude": {
                "config_path": self.claude_config_path.display().to_string(),
                "configured": self.claude_configured,
                "detail": self.claude_detail,
            },
            "codex": {
                "config_path": self.codex_config_path.as_ref().map(|path| path.display().to_string()),
                "configured": self.codex_configured,
                "detail": self.codex_detail,
            }
        })
    }
}

/// Inspect project-scoped Claude and user-scoped Codex MCP registration without mutating
/// either config. Missing files are reported as not configured rather than errors.
pub fn diagnose_mcp(project_root: &Path) -> McpDiagnostics {
    let mcp_binary_path = mcp_binary_path();
    let mcp_binary_exists = mcp_binary_available(&mcp_binary_path);
    let claude_config_path = project_root.join(".mcp.json");
    let (claude_configured, claude_detail) =
        inspect_claude_config(&claude_config_path, project_root);
    let codex_config_path =
        directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".codex").join("config.toml"));
    let (codex_configured, codex_detail) = codex_config_path
        .as_ref()
        .map(|path| inspect_codex_config(path, project_root))
        .unwrap_or_else(|| (false, "could not resolve home directory".to_string()));

    McpDiagnostics {
        project_root: project_root.to_path_buf(),
        board_path: crate::storage::board_store::board_path(project_root),
        board_exists: crate::storage::board_store::board_exists(project_root),
        mcp_binary_path,
        mcp_binary_exists,
        claude_config_path,
        claude_configured,
        claude_detail,
        codex_config_path,
        codex_configured,
        codex_detail,
    }
}

fn mcp_binary_available(path: &Path) -> bool {
    path.exists() || (is_bare_command(path) && path.file_name().is_some_and(command_on_path))
}

fn is_bare_command(path: &Path) -> bool {
    let mut components = path.components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn command_on_path(command: &OsStr) -> bool {
    command_on_search_path(command, std::env::var_os("PATH"))
}

fn command_on_search_path(command: &OsStr, path_var: Option<OsString>) -> bool {
    let Some(path_var) = path_var else {
        return false;
    };
    std::env::split_paths(&path_var).any(|dir| command_candidate_available(&dir.join(command)))
}

#[cfg(unix)]
fn command_candidate_available(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn command_candidate_available(path: &Path) -> bool {
    path.is_file()
}

fn inspect_claude_config(path: &Path, project_root: &Path) -> (bool, String) {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (false, "not installed for this project".to_string());
        }
        Err(error) => return (false, format!("could not read config: {error}")),
    };
    let value: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(error) => return (false, format!("invalid JSON: {error}")),
    };
    let Some(server) = value
        .get("mcpServers")
        .and_then(Value::as_object)
        .and_then(|servers| servers.get(SERVER_KEY))
    else {
        return (false, "terminaltiler server entry missing".to_string());
    };
    inspect_server_entry(
        server.get("command").and_then(Value::as_str),
        server.get("args").and_then(Value::as_array).map(|args| {
            args.iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        }),
        project_root,
    )
}

fn inspect_codex_config(path: &Path, project_root: &Path) -> (bool, String) {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (false, "not installed in Codex config".to_string());
        }
        Err(error) => return (false, format!("could not read config: {error}")),
    };
    let document: toml::Table = match toml::from_str(&raw) {
        Ok(document) => document,
        Err(error) => return (false, format!("invalid TOML: {error}")),
    };
    let Some(server) = document
        .get("mcp_servers")
        .and_then(toml::Value::as_table)
        .and_then(|servers| servers.get(SERVER_KEY))
        .and_then(toml::Value::as_table)
    else {
        return (false, "terminaltiler server entry missing".to_string());
    };
    let command = server.get("command").and_then(toml::Value::as_str);
    let args = server
        .get("args")
        .and_then(toml::Value::as_array)
        .map(|args| {
            args.iter()
                .filter_map(toml::Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        });
    inspect_server_entry(command, args, project_root)
}

fn inspect_server_entry(
    command: Option<&str>,
    args: Option<Vec<String>>,
    project_root: &Path,
) -> (bool, String) {
    let Some(command) = command.filter(|value| !value.trim().is_empty()) else {
        return (false, "server command missing".to_string());
    };
    let args = args.unwrap_or_default();
    let expected_root = project_root.to_string_lossy();
    let has_project_root = args
        .windows(2)
        .any(|pair| pair[0] == "--project-root" && pair[1] == expected_root);
    if !has_project_root {
        return (
            false,
            "server entry exists but does not target this project root".to_string(),
        );
    }
    (
        true,
        format!("configured: {command} --project-root {expected_root}"),
    )
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

    #[cfg(target_os = "linux")]
    fn set_file_mtime(path: &Path, seconds: libc::time_t) {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let path = CString::new(path.as_os_str().as_bytes()).unwrap();
        let times = [
            libc::timespec {
                tv_sec: seconds,
                tv_nsec: 0,
            },
            libc::timespec {
                tv_sec: seconds,
                tv_nsec: 0,
            },
        ];
        let result = unsafe { libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), 0) };
        assert_eq!(
            result,
            0,
            "failed to set mtime on {}",
            path.to_string_lossy()
        );
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

    #[test]
    fn install_stable_binary_copies_and_is_idempotent() {
        let dir = temp_dir();
        let source = dir.join("terminaltiler-mcp");
        fs::write(&source, b"binary-v1").unwrap();
        let dest_dir = dir.join("stable");

        let installed = install_stable_binary(&source, &dest_dir).unwrap();
        assert_eq!(installed, dest_dir.join("terminaltiler-mcp"));
        assert_eq!(fs::read(&installed).unwrap(), b"binary-v1");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&installed).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "installed binary must be executable");
        }

        // Unchanged source ⇒ no refresh, content preserved.
        assert!(!needs_refresh(&source, &installed));
        install_stable_binary(&source, &dest_dir).unwrap();
        assert_eq!(fs::read(&installed).unwrap(), b"binary-v1");
    }

    #[test]
    fn install_stable_binary_refreshes_when_source_changes() {
        let dir = temp_dir();
        let source = dir.join("terminaltiler-mcp");
        fs::write(&source, b"binary-v1").unwrap();
        let dest_dir = dir.join("stable");
        let installed = install_stable_binary(&source, &dest_dir).unwrap();

        // Different length ⇒ refresh regardless of mtime resolution.
        fs::write(&source, b"binary-v2-larger").unwrap();
        assert!(needs_refresh(&source, &installed));
        install_stable_binary(&source, &dest_dir).unwrap();
        assert_eq!(fs::read(&installed).unwrap(), b"binary-v2-larger");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn install_stable_binary_refreshes_same_size_content_with_older_source_mtime() {
        let dir = temp_dir();
        let source = dir.join("terminaltiler-mcp");
        fs::write(&source, b"binary-v1").unwrap();
        let dest_dir = dir.join("stable");
        let installed = install_stable_binary(&source, &dest_dir).unwrap();

        fs::write(&source, b"binary-v2").unwrap();
        assert_eq!(
            fs::metadata(&source).unwrap().len(),
            fs::metadata(&installed).unwrap().len()
        );
        set_file_mtime(&source, 1_700_000_000);
        set_file_mtime(&installed, 1_800_000_000);

        assert!(
            needs_refresh(&source, &installed),
            "same-size changed source must refresh even when its mtime is not newer"
        );
        install_stable_binary(&source, &dest_dir).unwrap();
        assert_eq!(fs::read(&installed).unwrap(), b"binary-v2");
    }

    #[test]
    fn bare_mcp_binary_is_resolved_through_search_path() {
        let dir = temp_dir();
        let bin_dir = dir.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let binary = bin_dir.join(mcp_binary_file_name());
        fs::write(&binary, b"fake-binary").unwrap();
        set_executable(&binary).unwrap();
        let search_path = std::env::join_paths([bin_dir]).unwrap();

        assert!(command_on_search_path(
            OsStr::new(mcp_binary_file_name()),
            Some(search_path)
        ));
        assert!(is_bare_command(Path::new(mcp_binary_file_name())));
        assert!(!is_bare_command(Path::new("/opt/terminaltiler-mcp")));
    }
}
