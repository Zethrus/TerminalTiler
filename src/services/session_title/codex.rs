//! Codex sessions: `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`.
//!
//! Codex records no session title. The first line is a `session_meta` entry carrying the
//! `cwd` and (in multi-agent mode) an `agent_nickname` that names the session; we use the
//! nickname as the title, falling back to the first user message. Only recent rollout files
//! under the newest day directories are considered, so an idle sessions tree is cheap to scan.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use super::util;
use super::{AgentKind, ResolvedTitle, SessionTitleSource};
use crate::platform::home_dir;

/// How many of the newest day-directories to scan. An active session updates a file created
/// on the day it started; two days covers sessions that span midnight.
const DAY_DIRS_SCANNED: usize = 2;

pub struct CodexSource {
    root: Option<PathBuf>,
}

impl Default for CodexSource {
    fn default() -> Self {
        Self {
            root: home_dir().map(|home| home.join(".codex").join("sessions")),
        }
    }
}

impl CodexSource {
    #[cfg(test)]
    pub fn with_root(root: PathBuf) -> Self {
        Self { root: Some(root) }
    }
}

impl SessionTitleSource for CodexSource {
    fn active_title(&self, cwd: &Path, max_age: Duration) -> Option<ResolvedTitle> {
        let root = self.root.as_ref()?;
        let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
        for day_dir in newest_day_dirs(root, DAY_DIRS_SCANNED) {
            for (file, mtime) in recent_rollouts(&day_dir, max_age) {
                if best.as_ref().is_none_or(|(_, m)| mtime > *m) && session_cwd_matches(&file, cwd)
                {
                    best = Some((file, mtime));
                }
            }
        }
        let (file, mtime) = best?;
        let title = read_title(&file)?;
        Some(ResolvedTitle {
            title,
            updated_at: mtime,
            agent: AgentKind::Codex,
        })
    }
}

/// The newest `count` `YYYY/MM/DD` directories under `root`, ordered newest-first.
fn newest_day_dirs(root: &Path, count: usize) -> Vec<PathBuf> {
    let mut days: Vec<PathBuf> = Vec::new();
    for year in numeric_subdirs(root) {
        for month in numeric_subdirs(&year) {
            days.extend(numeric_subdirs(&month));
        }
    }
    // Directory names are zero-padded numerics under the full path, so lexical order on the
    // path equals chronological order.
    days.sort();
    days.into_iter().rev().take(count).collect()
}

/// Immediate subdirectories whose names are purely numeric, sorted ascending.
fn numeric_subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter(|e| e.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit()))
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

/// Recent `rollout-*.jsonl` files in a day directory, paired with their mtimes.
fn recent_rollouts(day_dir: &Path, max_age: Duration) -> Vec<(PathBuf, std::time::SystemTime)> {
    std::fs::read_dir(day_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !(name.starts_with("rollout-") && name.ends_with(".jsonl")) {
                return None;
            }
            let mtime = entry.metadata().ok()?.modified().ok()?;
            util::is_recent(mtime, max_age).then_some((entry.path(), mtime))
        })
        .collect()
}

/// The `session_meta` payload (first line) of a rollout file.
fn session_meta(file: &Path) -> Option<Value> {
    let handle = File::open(file).ok()?;
    let mut first = String::new();
    BufReader::new(handle).read_line(&mut first).ok()?;
    let value: Value = serde_json::from_str(first.trim()).ok()?;
    value.get("payload").cloned()
}

fn session_cwd_matches(file: &Path, cwd: &Path) -> bool {
    session_meta(file)
        .and_then(|payload| payload.get("cwd").and_then(Value::as_str).map(str::to_string))
        .is_some_and(|recorded| util::cwd_matches(cwd, &recorded))
}

/// Title = agent nickname if present, else the first user message text.
fn read_title(file: &Path) -> Option<String> {
    if let Some(nickname) = session_meta(file)
        .and_then(|p| p.get("agent_nickname").and_then(Value::as_str).map(str::to_string))
    {
        let cleaned = util::clean_title(&nickname);
        if !cleaned.is_empty() {
            return Some(cleaned);
        }
    }
    first_user_message(file)
        .map(|text| util::clean_title(&text))
        .filter(|title| util::is_meaningful_prompt(title))
}

/// The first user message text, from either `event_msg`/`user_message` or a
/// `response_item` message with `role:"user"`. Streams and stops at the first hit.
fn first_user_message(file: &Path) -> Option<String> {
    let handle = File::open(file).ok()?;
    for line in BufReader::new(handle).lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let payload = value.get("payload").unwrap_or(&value);
        match payload.get("type").and_then(Value::as_str) {
            Some("user_message") => {
                if let Some(text) = payload.get("message").and_then(Value::as_str) {
                    return Some(text.to_string());
                }
            }
            Some("message") if payload.get("role").and_then(Value::as_str) == Some("user") => {
                if let Some(text) = payload
                    .get("content")
                    .and_then(Value::as_array)
                    .and_then(|parts| parts.iter().find_map(|p| p.get("text").and_then(Value::as_str)))
                {
                    return Some(text.to_string());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn uses_agent_nickname_and_matches_cwd() {
        let tmp = tempdir();
        let cwd = tmp.join("proj");
        fs::create_dir_all(&cwd).unwrap();
        let day = tmp.join("sessions/2026/07/09");
        fs::create_dir_all(&day).unwrap();
        fs::write(
            day.join("rollout-2026-07-09T12-00-00-abc.jsonl"),
            format!(
                "{}\n",
                serde_json::json!({"type":"session_meta","payload":
                    {"cwd":cwd.to_string_lossy(),"agent_nickname":"Hubble"}}),
            ),
        )
        .unwrap();

        let source = CodexSource::with_root(tmp.join("sessions"));
        let resolved = source
            .active_title(&cwd, Duration::from_secs(3600))
            .expect("title");
        assert_eq!(resolved.title, "Hubble");
        assert_eq!(resolved.agent, AgentKind::Codex);
    }

    #[test]
    fn ignores_sessions_for_other_cwd() {
        let tmp = tempdir();
        let cwd = tmp.join("proj");
        fs::create_dir_all(&cwd).unwrap();
        let day = tmp.join("sessions/2026/07/09");
        fs::create_dir_all(&day).unwrap();
        fs::write(
            day.join("rollout-2026-07-09T12-00-00-abc.jsonl"),
            format!(
                "{}\n",
                serde_json::json!({"type":"session_meta","payload":
                    {"cwd":"/somewhere/else","agent_nickname":"Ghost"}}),
            ),
        )
        .unwrap();

        let source = CodexSource::with_root(tmp.join("sessions"));
        assert!(source.active_title(&cwd, Duration::from_secs(3600)).is_none());
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tt-codex-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
