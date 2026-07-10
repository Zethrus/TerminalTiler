//! Resolves the live title of the agentic coding session running inside a terminal tile.
//!
//! Each supported CLI (Claude, Codex, opencode, Copilot, Grok) records its sessions on disk
//! keyed by working directory. The caller identifies which agent is running in a tile (see
//! [`AgentKind::from_command`]) and asks that agent's source for the active session title for
//! the tile's cwd via [`resolve_title_for`]. Titling only from the running agent's store is
//! what keeps two tiles that share a working directory from being cross-labelled.
//!
//! All sources are pure filesystem readers with no GTK dependency, so they are unit-testable
//! with on-disk fixtures.

use std::path::Path;
use std::time::Duration;

pub mod claude;
pub mod codex;
pub mod copilot;
pub mod grok;
pub mod opencode;
mod util;

pub(crate) use util::clean_title;
// `percent_decode` is only consumed by the Linux tile poller (Grok reads it via `util::` directly).
#[cfg(target_os = "linux")]
pub(crate) use util::percent_decode;

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
    // Consumed only by the Linux tile poller's tooltip.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn label(self) -> &'static str {
        match self {
            AgentKind::Claude => "Claude",
            AgentKind::Codex => "Codex",
            AgentKind::Opencode => "opencode",
            AgentKind::Copilot => "Copilot",
            AgentKind::Grok => "Grok",
        }
    }

    /// Identify the agent from a process command line or launch command. Matches on the
    /// basename tokens of each whitespace/NUL-separated argument, so `node /x/claude`,
    /// `/usr/bin/codex --flag`, and `gh copilot` all resolve. Returns `None` for non-agents
    /// (a plain shell), which is how the poller avoids titling non-agent tiles.
    pub fn from_command(command: &str) -> Option<AgentKind> {
        command
            .split(|c: char| c.is_whitespace() || c == '\0')
            .filter(|arg| !arg.is_empty())
            .find_map(|arg| {
                let token = arg
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(arg)
                    .trim_end_matches(".exe")
                    .to_ascii_lowercase();
                match token.as_str() {
                    "claude" => Some(AgentKind::Claude),
                    "codex" => Some(AgentKind::Codex),
                    "opencode" => Some(AgentKind::Opencode),
                    "copilot" => Some(AgentKind::Copilot),
                    "grok" => Some(AgentKind::Grok),
                    _ => None,
                }
            })
    }

    /// The store source for this agent.
    fn source(self) -> Box<dyn SessionTitleSource> {
        match self {
            AgentKind::Claude => Box::new(claude::ClaudeSource::default()),
            AgentKind::Codex => Box::new(codex::CodexSource::default()),
            AgentKind::Opencode => Box::new(opencode::OpencodeSource::default()),
            AgentKind::Copilot => Box::new(copilot::CopilotSource::default()),
            AgentKind::Grok => Box::new(grok::GrokSource::default()),
        }
    }
}

/// A session title resolved from an agent's on-disk store.
#[derive(Debug, Clone)]
pub struct ResolvedTitle {
    pub title: String,
    // Read only by the Linux tile poller's tooltip.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub agent: AgentKind,
}

/// A per-agent session store that can report the active session title for a working directory.
pub trait SessionTitleSource {
    /// The title of the session for `cwd` whose store was updated within `max_age`, if any.
    fn active_title(&self, cwd: &Path, max_age: Duration) -> Option<ResolvedTitle>;
}

/// Resolve the active session title for a single, already-identified agent: it only reads the
/// store of the agent actually running in the tile, so tiles that share a working directory
/// are not cross-labelled. Returns `None` when that agent has no active session for `cwd`
/// within `max_age`.
pub fn resolve_title_for(agent: AgentKind, cwd: &Path, max_age: Duration) -> Option<ResolvedTitle> {
    agent
        .source()
        .active_title(cwd, max_age)
        .filter(|resolved| !resolved.title.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_command_classifies_agents_and_wrappers() {
        assert_eq!(AgentKind::from_command("claude"), Some(AgentKind::Claude));
        assert_eq!(
            AgentKind::from_command("node /home/u/.local/bin/claude --resume"),
            Some(AgentKind::Claude)
        );
        assert_eq!(
            AgentKind::from_command("/usr/bin/codex"),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            AgentKind::from_command("gh copilot"),
            Some(AgentKind::Copilot)
        );
        assert_eq!(
            AgentKind::from_command(r"C:\Program Files\grok\grok.exe"),
            Some(AgentKind::Grok)
        );
        assert_eq!(
            AgentKind::from_command("bun x opencode"),
            Some(AgentKind::Opencode)
        );
    }

    #[test]
    fn from_command_ignores_plain_shells() {
        assert_eq!(AgentKind::from_command("/bin/bash -l"), None);
        assert_eq!(AgentKind::from_command("fish"), None);
        assert_eq!(AgentKind::from_command(""), None);
    }
}
