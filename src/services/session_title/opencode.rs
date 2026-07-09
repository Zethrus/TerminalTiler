//! opencode sessions: `~/.local/share/opencode/storage/`.
//!
//! `project/<id>.json` maps a project id to its `worktree` path; `session/<id>/ses_*.json`
//! holds each session's explicit `title` and `directory`. We find the project whose worktree
//! matches the cwd, then read the newest session's title.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::Value;

use super::util;
use super::{AgentKind, ResolvedTitle, SessionTitleSource};
use crate::platform::home_dir;

pub struct OpencodeSource {
    storage: Option<PathBuf>,
}

impl Default for OpencodeSource {
    fn default() -> Self {
        Self {
            storage: home_dir().map(|home| home.join(".local/share/opencode/storage")),
        }
    }
}

impl OpencodeSource {
    #[cfg(test)]
    pub fn with_storage(storage: PathBuf) -> Self {
        Self {
            storage: Some(storage),
        }
    }
}

impl SessionTitleSource for OpencodeSource {
    fn active_title(&self, cwd: &Path, max_age: Duration) -> Option<ResolvedTitle> {
        let storage = self.storage.as_ref()?;
        let project_id = project_id_for(&storage.join("project"), cwd)?;
        let session_dir = storage.join("session").join(project_id);
        let (session_file, mtime) =
            util::newest_file_in(&session_dir, |name| name.ends_with(".json"))?;
        if !util::is_recent(mtime, max_age) {
            return None;
        }
        let value: Value =
            serde_json::from_str(&std::fs::read_to_string(&session_file).ok()?).ok()?;
        let title = util::clean_title(value.get("title")?.as_str()?);
        if title.is_empty() {
            return None;
        }
        Some(ResolvedTitle {
            title,
            updated_at: mtime,
            agent: AgentKind::Opencode,
        })
    }
}

/// A cached `(worktree, project_id)` map tagged with the project directory and its mtime.
type ProjectCache = ((PathBuf, SystemTime), Vec<(String, String)>);

thread_local! {
    /// Keyed by the project directory and its mtime so the per-poll cost is a single stat
    /// until a project is added or removed.
    static PROJECT_MAP: RefCell<Option<ProjectCache>> = const { RefCell::new(None) };
}

/// The project id whose `worktree` matches `cwd`.
fn project_id_for(project_dir: &Path, cwd: &Path) -> Option<String> {
    let key = (project_dir.to_path_buf(), util::mtime(project_dir)?);
    PROJECT_MAP.with(|cell| {
        let mut cell = cell.borrow_mut();
        if cell.as_ref().map(|(k, _)| k) != Some(&key) {
            let projects = load_projects(project_dir);
            *cell = Some((key, projects));
        }
        cell.as_ref()
            .unwrap()
            .1
            .iter()
            .find(|(worktree, _)| util::cwd_matches(cwd, worktree))
            .map(|(_, id)| id.clone())
    })
}

/// Read every `project/*.json` into a `(worktree, id)` list.
fn load_projects(project_dir: &Path) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(project_dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                return None;
            }
            let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
            let worktree = value.get("worktree").and_then(Value::as_str)?.to_string();
            let id = value
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()))?;
            Some((worktree, id))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reads_explicit_session_title() {
        let tmp = tempdir();
        let cwd = tmp.join("aerosyne-core");
        fs::create_dir_all(&cwd).unwrap();
        let storage = tmp.join("storage");
        let project_dir = storage.join("project");
        fs::create_dir_all(&project_dir).unwrap();
        fs::write(
            project_dir.join("pid.json"),
            serde_json::json!({"id":"pid","worktree":cwd.to_string_lossy()}).to_string(),
        )
        .unwrap();
        let session_dir = storage.join("session").join("pid");
        fs::create_dir_all(&session_dir).unwrap();
        fs::write(
            session_dir.join("ses_1.json"),
            serde_json::json!({"id":"ses_1","directory":cwd.to_string_lossy(),
                "title":"Remove SECURE GATEWAY banner from whitelabel pages"})
            .to_string(),
        )
        .unwrap();

        let source = OpencodeSource::with_storage(storage);
        let resolved = source
            .active_title(&cwd, Duration::from_secs(3600))
            .expect("title");
        assert_eq!(
            resolved.title,
            "Remove SECURE GATEWAY banner from whitelabel pages"
        );
        assert_eq!(resolved.agent, AgentKind::Opencode);
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tt-opencode-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
