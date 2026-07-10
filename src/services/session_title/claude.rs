//! Claude Code sessions: `~/.claude/projects/<escaped-cwd>/<uuid>.jsonl`.
//!
//! The project directory name is the cwd with every non-`[A-Za-z0-9-]` character replaced by
//! `-`. The session title is the last `{"type":"custom-title"}` entry's `customTitle` (a
//! stable value written near the start of the session), falling back to the first user
//! message text. Claude also emits this as its OSC window title, so the poller and the
//! existing OSC handler converge on the same value.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use super::util;
use super::{AgentKind, ResolvedTitle, SessionTitleSource};
use crate::platform::home_dir;

pub struct ClaudeSource {
    root: Option<PathBuf>,
}

impl Default for ClaudeSource {
    fn default() -> Self {
        Self {
            root: home_dir().map(|home| home.join(".claude").join("projects")),
        }
    }
}

impl ClaudeSource {
    #[cfg(test)]
    pub fn with_root(root: PathBuf) -> Self {
        Self { root: Some(root) }
    }
}

impl SessionTitleSource for ClaudeSource {
    fn active_title(&self, cwd: &Path, max_age: Duration) -> Option<ResolvedTitle> {
        let root = self.root.as_ref()?;
        // The escaped directory name is deterministic, so a single `is_dir` stat locates the
        // project (or proves there is none) without scanning every project on each poll.
        let project_dir = root.join(escape_cwd(cwd));
        if !project_dir.is_dir() {
            return None;
        }
        let (session_file, mtime) =
            util::newest_file_in(&project_dir, |name| name.ends_with(".jsonl"))?;
        if !util::is_recent(mtime, max_age) {
            return None;
        }
        let title = read_title(&session_file, cwd)?;
        Some(ResolvedTitle {
            title,
            agent: AgentKind::Claude,
        })
    }
}

/// Claude's cwd → directory-name escaping.
fn escape_cwd(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Read the title from a session file, preferring the *latest* `custom-title` (it changes when
/// the user renames the session) over the first meaningful user message. `cwd` is verified
/// against the session's recorded cwd to guard against escaped-name collisions.
fn read_title(file: &Path, cwd: &Path) -> Option<String> {
    let handle = File::open(file).ok()?;
    let mut last_custom: Option<String> = None;
    let mut first_user: Option<String> = None;
    for line in BufReader::new(handle).lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(recorded) = value.get("cwd").and_then(Value::as_str)
            && !util::cwd_matches(cwd, recorded)
        {
            return None;
        }
        match value.get("type").and_then(Value::as_str) {
            Some("custom-title") => {
                if let Some(title) = value.get("customTitle").and_then(Value::as_str) {
                    let cleaned = util::clean_title(title);
                    if !cleaned.is_empty() {
                        last_custom = Some(cleaned);
                    }
                }
            }
            Some("user") if first_user.is_none() => {
                first_user = user_message_text(&value)
                    .map(|t| util::clean_title(&t))
                    .filter(|t| util::is_meaningful_prompt(t));
            }
            _ => {}
        }
    }
    last_custom.or(first_user).filter(|t| !t.is_empty())
}

/// Extract plain text from a Claude `user` entry (`message.content` as a string or an array
/// of `{type:"text", text}` parts).
fn user_message_text(value: &Value) -> Option<String> {
    let content = value.get("message").and_then(|m| m.get("content"))?;
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }
    content
        .as_array()?
        .iter()
        .find_map(|part| part.get("text").and_then(Value::as_str).map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn escapes_cwd_like_claude() {
        assert_eq!(
            escape_cwd(Path::new("/home/zethrus/Projects/TerminalTiler")),
            "-home-zethrus-Projects-TerminalTiler"
        );
    }

    #[test]
    fn reads_custom_title_over_user_message() {
        let tmp = tempdir();
        let cwd = tmp.join("work");
        fs::create_dir_all(&cwd).unwrap();
        let proj = tmp.join("projects").join(escape_cwd(&cwd));
        fs::create_dir_all(&proj).unwrap();
        let session = proj.join("s.jsonl");
        fs::write(
            &session,
            format!(
                "{}\n{}\n{}\n",
                serde_json::json!({"type":"user","cwd":cwd.to_string_lossy(),
                    "message":{"content":"first prompt"}}),
                serde_json::json!({"type":"custom-title","customTitle":"✳ My Session"}),
                serde_json::json!({"type":"assistant"}),
            ),
        )
        .unwrap();

        let source = ClaudeSource::with_root(tmp.join("projects"));
        let resolved = source
            .active_title(&cwd, Duration::from_secs(3600))
            .expect("title");
        assert_eq!(resolved.title, "My Session");
        assert_eq!(resolved.agent, AgentKind::Claude);
    }

    #[test]
    fn falls_back_to_first_user_message() {
        let tmp = tempdir();
        let cwd = tmp.join("work2");
        fs::create_dir_all(&cwd).unwrap();
        let proj = tmp.join("projects").join(escape_cwd(&cwd));
        fs::create_dir_all(&proj).unwrap();
        fs::write(
            proj.join("s.jsonl"),
            format!(
                "{}\n",
                serde_json::json!({"type":"user","cwd":cwd.to_string_lossy(),
                    "message":{"content":[{"type":"text","text":"Fix the parser"}]}}),
            ),
        )
        .unwrap();

        let source = ClaudeSource::with_root(tmp.join("projects"));
        let resolved = source
            .active_title(&cwd, Duration::from_secs(3600))
            .expect("title");
        assert_eq!(resolved.title, "Fix the parser");
    }

    #[test]
    fn uses_latest_custom_title_after_rename() {
        let tmp = tempdir();
        let cwd = tmp.join("work-rename");
        fs::create_dir_all(&cwd).unwrap();
        let proj = tmp.join("projects").join(escape_cwd(&cwd));
        fs::create_dir_all(&proj).unwrap();
        fs::write(
            proj.join("s.jsonl"),
            format!(
                "{}\n{}\n{}\n",
                serde_json::json!({"type":"custom-title","customTitle":"Old Name",
                    "cwd":cwd.to_string_lossy()}),
                serde_json::json!({"type":"user","message":{"content":"do work"}}),
                serde_json::json!({"type":"custom-title","customTitle":"Renamed Session"}),
            ),
        )
        .unwrap();

        let source = ClaudeSource::with_root(tmp.join("projects"));
        let resolved = source
            .active_title(&cwd, Duration::from_secs(3600))
            .expect("title");
        assert_eq!(resolved.title, "Renamed Session");
    }

    #[test]
    fn skips_command_caveat_user_entries() {
        let tmp = tempdir();
        let cwd = tmp.join("work3");
        fs::create_dir_all(&cwd).unwrap();
        let proj = tmp.join("projects").join(escape_cwd(&cwd));
        fs::create_dir_all(&proj).unwrap();
        fs::write(
            proj.join("s.jsonl"),
            format!(
                "{}\n{}\n",
                serde_json::json!({"type":"user","cwd":cwd.to_string_lossy(),
                    "message":{"content":"<local-command-caveat>ignore me</local-command-caveat>"}}),
                serde_json::json!({"type":"user",
                    "message":{"content":"Actually fix the cave rendering bug"}}),
            ),
        )
        .unwrap();

        let source = ClaudeSource::with_root(tmp.join("projects"));
        let resolved = source
            .active_title(&cwd, Duration::from_secs(3600))
            .expect("title");
        assert_eq!(resolved.title, "Actually fix the cave rendering bug");
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tt-claude-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
