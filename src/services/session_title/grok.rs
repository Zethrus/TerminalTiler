//! Grok sessions: `~/.grok/sessions/<percent-encoded-cwd>/<uuid>/summary.json`.
//!
//! The directory name is the percent-encoded cwd; `summary.json` carries `session_summary`
//! (the session title, sometimes empty for brand-new sessions). When the summary is empty
//! there is no meaningful title, so we report none and let the OSC/default title stand.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use super::util;
use super::{AgentKind, ResolvedTitle, SessionTitleSource};
use crate::platform::home_dir;

pub struct GrokSource {
    root: Option<PathBuf>,
}

impl Default for GrokSource {
    fn default() -> Self {
        Self {
            root: home_dir().map(|home| home.join(".grok").join("sessions")),
        }
    }
}

impl GrokSource {
    #[cfg(test)]
    pub fn with_root(root: PathBuf) -> Self {
        Self { root: Some(root) }
    }
}

impl SessionTitleSource for GrokSource {
    fn active_title(&self, cwd: &Path, max_age: Duration) -> Option<ResolvedTitle> {
        let root = self.root.as_ref()?;
        let cwd_dir = cwd_dir_for(root, cwd)?;
        // Each session is a `<uuid>/summary.json`; pick the newest by summary mtime.
        let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
        for entry in std::fs::read_dir(&cwd_dir).ok()?.flatten() {
            let summary = entry.path().join("summary.json");
            let Some(mtime) = util::mtime(&summary) else {
                continue;
            };
            if best.as_ref().is_none_or(|(_, m)| mtime > *m) {
                best = Some((summary, mtime));
            }
        }
        let (summary_file, mtime) = best?;
        if !util::is_recent(mtime, max_age) {
            return None;
        }
        let value: Value =
            serde_json::from_str(&std::fs::read_to_string(&summary_file).ok()?).ok()?;
        let title = util::clean_title(value.get("session_summary")?.as_str()?);
        if title.is_empty() {
            return None;
        }
        Some(ResolvedTitle {
            title,
            updated_at: mtime,
            agent: AgentKind::Grok,
        })
    }
}

/// The session directory whose percent-decoded name matches `cwd`.
fn cwd_dir_for(root: &Path, cwd: &Path) -> Option<PathBuf> {
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let decoded = util::percent_decode(&entry.file_name().to_string_lossy());
        if util::cwd_matches(cwd, &decoded) {
            return Some(entry.path());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reads_session_summary_by_encoded_dir() {
        let tmp = tempdir();
        let cwd = tmp.join("Wildroot");
        fs::create_dir_all(&cwd).unwrap();
        let root = tmp.join("sessions");
        let encoded = cwd.to_string_lossy().replace('/', "%2F");
        let session = root.join(encoded).join("uuid-1");
        fs::create_dir_all(&session).unwrap();
        fs::write(
            session.join("summary.json"),
            serde_json::json!({"info":{"cwd":cwd.to_string_lossy()},
                "session_summary":"Wire up the settings panel"})
            .to_string(),
        )
        .unwrap();

        let source = GrokSource::with_root(root);
        let resolved = source
            .active_title(&cwd, Duration::from_secs(3600))
            .expect("title");
        assert_eq!(resolved.title, "Wire up the settings panel");
        assert_eq!(resolved.agent, AgentKind::Grok);
    }

    #[test]
    fn empty_summary_yields_no_title() {
        let tmp = tempdir();
        let cwd = tmp.join("Wildroot");
        fs::create_dir_all(&cwd).unwrap();
        let root = tmp.join("sessions");
        let encoded = cwd.to_string_lossy().replace('/', "%2F");
        let session = root.join(encoded).join("uuid-1");
        fs::create_dir_all(&session).unwrap();
        fs::write(
            session.join("summary.json"),
            serde_json::json!({"session_summary":""}).to_string(),
        )
        .unwrap();

        let source = GrokSource::with_root(root);
        assert!(
            source
                .active_title(&cwd, Duration::from_secs(3600))
                .is_none()
        );
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tt-grok-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
