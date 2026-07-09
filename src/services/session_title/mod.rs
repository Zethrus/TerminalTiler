//! Resolves the live title of the agentic coding session running inside a terminal tile.
//!
//! Each supported CLI (Claude, Codex, opencode, Copilot, Grok) records its sessions on disk
//! keyed by working directory. Given a tile's live cwd we ask every source for its most
//! recently updated *active* session title and return the newest across all of them — that
//! is the agent currently running in the tile. This is intentionally process-agnostic: we
//! never inspect the foreground process, only the session stores.
//!
//! All sources are pure filesystem readers with no GTK dependency, so they are unit-testable
//! with on-disk fixtures.

use std::path::Path;
use std::time::{Duration, SystemTime};

pub mod claude;
pub mod codex;
pub mod copilot;
pub mod grok;
pub mod opencode;
mod util;

pub(crate) use util::{clean_title, percent_decode};

/// The agentic coding CLI a resolved title came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Codex,
    Opencode,
    Copilot,
    Grok,
}

impl AgentKind {
    pub fn label(self) -> &'static str {
        match self {
            AgentKind::Claude => "Claude",
            AgentKind::Codex => "Codex",
            AgentKind::Opencode => "opencode",
            AgentKind::Copilot => "Copilot",
            AgentKind::Grok => "Grok",
        }
    }
}

/// A session title resolved from an agent's on-disk store.
#[derive(Debug, Clone)]
pub struct ResolvedTitle {
    pub title: String,
    pub updated_at: SystemTime,
    pub agent: AgentKind,
}

/// A per-agent session store that can report the active session title for a working directory.
pub trait SessionTitleSource {
    /// The title of the session for `cwd` whose store was updated within `max_age`, if any.
    fn active_title(&self, cwd: &Path, max_age: Duration) -> Option<ResolvedTitle>;
}

/// Default set of sources (each rooted at the current user's home directory).
fn default_sources() -> Vec<Box<dyn SessionTitleSource>> {
    vec![
        Box::new(claude::ClaudeSource::default()),
        Box::new(codex::CodexSource::default()),
        Box::new(opencode::OpencodeSource::default()),
        Box::new(copilot::CopilotSource::default()),
        Box::new(grok::GrokSource::default()),
    ]
}

/// Resolve the most recently updated active session title across all supported agents for
/// `cwd`. Returns `None` when no agent has an active session there within `max_age`.
pub fn resolve_active_title(cwd: &Path, max_age: Duration) -> Option<ResolvedTitle> {
    resolve_from(&default_sources(), cwd, max_age)
}

/// Resolution over an explicit source list (used by tests).
pub fn resolve_from(
    sources: &[Box<dyn SessionTitleSource>],
    cwd: &Path,
    max_age: Duration,
) -> Option<ResolvedTitle> {
    sources
        .iter()
        .filter_map(|source| source.active_title(cwd, max_age))
        .filter(|resolved| !resolved.title.trim().is_empty())
        .max_by_key(|resolved| resolved.updated_at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    struct Fixed(Option<ResolvedTitle>);
    impl SessionTitleSource for Fixed {
        fn active_title(&self, _cwd: &Path, _max_age: Duration) -> Option<ResolvedTitle> {
            self.0.clone()
        }
    }

    fn at(secs: u64, agent: AgentKind, title: &str) -> ResolvedTitle {
        ResolvedTitle {
            title: title.to_string(),
            updated_at: UNIX_EPOCH + Duration::from_secs(secs),
            agent,
        }
    }

    #[test]
    fn newest_updated_source_wins() {
        let sources: Vec<Box<dyn SessionTitleSource>> = vec![
            Box::new(Fixed(Some(at(100, AgentKind::Claude, "old claude")))),
            Box::new(Fixed(Some(at(200, AgentKind::Codex, "fresh codex")))),
            Box::new(Fixed(None)),
        ];
        let resolved = resolve_from(&sources, Path::new("/x"), Duration::from_secs(60)).unwrap();
        assert_eq!(resolved.title, "fresh codex");
        assert_eq!(resolved.agent, AgentKind::Codex);
    }

    #[test]
    fn no_active_sessions_yields_none() {
        let sources: Vec<Box<dyn SessionTitleSource>> =
            vec![Box::new(Fixed(None)), Box::new(Fixed(None))];
        assert!(resolve_from(&sources, Path::new("/x"), Duration::from_secs(60)).is_none());
    }
}
