use std::collections::HashMap;
use std::path::Path;

use crate::model::layout::{TileKind, TileSpec};
use crate::storage::session_store::{SavedSession, SavedTerminalHistory};

pub type RestoreStartupOverrideMap = HashMap<String, String>;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub type RestoreStartupOverridesByTab = Vec<RestoreStartupOverrideMap>;

const CODEX_LAST_RESUME_COMMAND: &str = "codex resume --last --no-alt-screen";
const CLAUDE_CONTINUE_COMMAND: &str = "claude --continue";

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum AgentCli {
    Codex,
    Claude,
    Omx,
    Hermes,
    OpenClaw,
}

#[derive(Clone, Debug)]
struct ParsedStartup {
    direct_tokens: Vec<String>,
    wrapper_prefix: Option<Vec<String>>,
}

impl ParsedStartup {
    fn direct_command(tokens: Vec<String>) -> Self {
        Self {
            direct_tokens: tokens,
            wrapper_prefix: None,
        }
    }

    fn wrapped_command(tokens: &[String], command_index: usize, inner_tokens: Vec<String>) -> Self {
        Self {
            direct_tokens: inner_tokens,
            wrapper_prefix: Some(tokens[..command_index].to_vec()),
        }
    }

    fn wrap_resume_command(&self, resume_command: &str) -> String {
        if let Some(wrapper_prefix) = &self.wrapper_prefix {
            format!(
                "{} {}",
                shell_join_owned_tokens(wrapper_prefix),
                shell_quote(resume_command)
            )
        } else {
            resume_command.to_string()
        }
    }
}

#[derive(Clone, Debug)]
struct ResumeResolution {
    agent: AgentCli,
    command: String,
    exact: bool,
}

/// Backwards-compatible single-command helper used by older tests and simple
/// callers. It keeps the historical single-pane fallback, but full workspace
/// restore should prefer `restore_startup_overrides_for_saved_session` so that
/// duplicate panes can be checked before using most-recent resume commands.
#[cfg_attr(not(test), allow(dead_code))]
pub fn restore_startup_override(command: &str) -> Option<String> {
    let parsed = parse_startup_command(command)?;
    let resolution = restore_resolution_from_parts(&parsed, None, &[], false)?;
    Some(parsed.wrap_resume_command(&resolution.command))
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn restore_startup_overrides_for_saved_session(
    session: &SavedSession,
) -> RestoreStartupOverridesByTab {
    session
        .tabs
        .iter()
        .map(|tab| {
            restore_startup_overrides_for_saved_tab(
                tab.preset.layout.tile_specs().iter(),
                &tab.workspace_root,
                &tab.terminal_history,
            )
        })
        .collect()
}

pub fn restore_startup_overrides_for_saved_tab<'a>(
    tiles: impl IntoIterator<Item = &'a TileSpec>,
    _workspace_root: &Path,
    terminal_history: &[SavedTerminalHistory],
) -> RestoreStartupOverrideMap {
    let tiles = tiles.into_iter().collect::<Vec<_>>();
    let duplicate_counts = agent_counts(&tiles);
    let history_by_tile = histories_by_tile(terminal_history);

    tiles
        .into_iter()
        .filter(|tile| tile.tile_kind == TileKind::Terminal)
        .filter_map(|tile| {
            let command = tile.startup_command.as_deref()?;
            let parsed = parse_startup_command(command)?;
            let agent = agent_from_tokens(&parsed.direct_tokens)?;
            let is_duplicate = duplicate_counts.get(&agent).copied().unwrap_or(0) > 1;
            let history = history_by_tile.get(tile.id.as_str()).copied();
            let saved_resume_command =
                history.and_then(|history| history.resume_command.as_deref());
            let lines = history
                .map(|history| history.lines.as_slice())
                .unwrap_or(&[]);
            restore_resolution_from_parts(&parsed, saved_resume_command, lines, is_duplicate).map(
                |resolution| {
                    (
                        tile.id.clone(),
                        parsed.wrap_resume_command(&resolution.command),
                    )
                },
            )
        })
        .collect()
}

pub fn initial_startup_overrides_for_tiles<'a>(
    tiles: impl IntoIterator<Item = &'a TileSpec>,
    workspace_root: &Path,
) -> RestoreStartupOverrideMap {
    tiles
        .into_iter()
        .filter(|tile| tile.tile_kind == TileKind::Terminal)
        .filter_map(|tile| {
            let command = tile.startup_command.as_deref()?;
            let parsed = parse_startup_command(command)?;
            initial_identity_resolution(&parsed, tile, workspace_root)
                .map(|command| (tile.id.clone(), parsed.wrap_resume_command(&command)))
        })
        .collect()
}

/// Legacy tile-only helper. It can only detect duplicates from the tile set and
/// cannot use saved identity metadata, so it intentionally avoids ambiguous
/// most-recent fallbacks when multiple panes of the same agent exist.
pub fn restore_startup_overrides_for_tiles<'a>(
    tiles: impl IntoIterator<Item = &'a TileSpec>,
) -> RestoreStartupOverrideMap {
    let tiles = tiles.into_iter().collect::<Vec<_>>();
    let duplicate_counts = agent_counts(&tiles);

    tiles
        .into_iter()
        .filter(|tile| tile.tile_kind == TileKind::Terminal)
        .filter_map(|tile| {
            let command = tile.startup_command.as_deref()?;
            let parsed = parse_startup_command(command)?;
            let agent = agent_from_tokens(&parsed.direct_tokens)?;
            let is_duplicate = duplicate_counts.get(&agent).copied().unwrap_or(0) > 1;
            restore_resolution_from_parts(&parsed, None, &[], is_duplicate).map(|resolution| {
                (
                    tile.id.clone(),
                    parsed.wrap_resume_command(&resolution.command),
                )
            })
        })
        .collect()
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn restore_startup_overrides_for_tab_tile_sets(
    tab_tile_sets: impl IntoIterator<Item = Vec<TileSpec>>,
) -> RestoreStartupOverridesByTab {
    tab_tile_sets
        .into_iter()
        .map(|tiles| restore_startup_overrides_for_tiles(tiles.iter()))
        .collect()
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn restore_startup_override_for_tab_tile<'a>(
    overrides_by_tab: &'a [RestoreStartupOverrideMap],
    tab_index: usize,
    tile_id: &str,
) -> Option<&'a str> {
    overrides_by_tab
        .get(tab_index)?
        .get(tile_id)
        .map(String::as_str)
}

pub fn saved_resume_command_for_tile(
    tile: &TileSpec,
    _workspace_root: &Path,
    terminal_history_lines: &[String],
) -> Option<String> {
    let command = tile.startup_command.as_deref()?;
    let parsed = parse_startup_command(command)?;
    restore_resolution_from_parts(&parsed, None, terminal_history_lines, true)
        .filter(|resolution| resolution.exact)
        .map(|resolution| resolution.command)
}

fn restore_resolution_from_parts(
    parsed: &ParsedStartup,
    saved_resume_command: Option<&str>,
    terminal_history_lines: &[String],
    duplicate_agent_pane: bool,
) -> Option<ResumeResolution> {
    let agent = agent_from_tokens(&parsed.direct_tokens)?;
    if let Some(resolution) = saved_resume_command
        .filter(|command| !command.trim().is_empty())
        .and_then(|command| exact_saved_resume_command(command, agent))
    {
        return Some(resolution);
    }

    if let Some(resolution) =
        resume_from_history(terminal_history_lines, agent, &parsed.direct_tokens)
    {
        return Some(resolution);
    }

    if let Some(resolution) = exact_resume_from_startup(&parsed.direct_tokens) {
        return Some(resolution);
    }

    if duplicate_agent_pane {
        return None;
    }

    most_recent_fallback(&parsed.direct_tokens).map(|command| ResumeResolution {
        agent,
        command,
        exact: false,
    })
}

fn exact_saved_resume_command(command: &str, expected_agent: AgentCli) -> Option<ResumeResolution> {
    let tokens = split_shell_words(command.trim())?;
    exact_resume_from_startup(&tokens).filter(|resolution| resolution.agent == expected_agent)
}

fn initial_identity_resolution(
    parsed: &ParsedStartup,
    tile: &TileSpec,
    workspace_root: &Path,
) -> Option<String> {
    startup_identity_launch_command(parsed, tile, workspace_root)
}

fn startup_identity_launch_command(
    parsed: &ParsedStartup,
    tile: &TileSpec,
    workspace_root: &Path,
) -> Option<String> {
    let tokens = &parsed.direct_tokens;
    match agent_from_tokens(tokens)? {
        AgentCli::Claude if is_bare_agent(tokens, AgentCli::Claude) => {
            let name = stable_claude_name(tile, workspace_root);
            Some(shell_join_tokens(&["claude", "--name", name.as_str()]))
        }
        AgentCli::OpenClaw
            if openclaw_session_key(tokens).is_none() && is_openclaw_tui_command(tokens) =>
        {
            Some(openclaw_launch_command_with_session(
                tokens,
                &stable_openclaw_key(tile, workspace_root),
            ))
        }
        _ => None,
    }
}

fn exact_resume_from_startup(tokens: &[String]) -> Option<ResumeResolution> {
    match agent_from_tokens(tokens)? {
        AgentCli::Codex => {
            if tokens.get(1).map(String::as_str) == Some("resume") {
                first_positional_after(tokens, 2)?;
                return Some(ResumeResolution {
                    agent: AgentCli::Codex,
                    command: codex_resume_command(tokens),
                    exact: true,
                });
            }
            None
        }
        AgentCli::Claude => option_value(tokens, &["--resume", "-r"])
            .or_else(|| option_value(tokens, &["--session-id"]))
            .map(|_| ResumeResolution {
                agent: AgentCli::Claude,
                command: shell_join_owned_tokens(tokens),
                exact: true,
            })
            .or_else(|| {
                claude_session_name(tokens).map(|name| ResumeResolution {
                    agent: AgentCli::Claude,
                    command: claude_resume_command_for_named_session(tokens, name.as_str()),
                    exact: true,
                })
            }),
        AgentCli::Omx => {
            let command_index = first_omx_positional_index(tokens)?;
            if tokens.get(command_index).map(String::as_str) == Some("resume") {
                let target = first_positional_after(tokens, command_index + 1)?;
                if !looks_like_uuid(target) {
                    return None;
                }
                return Some(ResumeResolution {
                    agent: AgentCli::Omx,
                    command: shell_join_owned_tokens(tokens),
                    exact: true,
                });
            }
            None
        }
        AgentCli::Hermes => option_value(tokens, &["--resume", "-r"])
            .or_else(|| option_value(tokens, &["--continue", "-c"]))
            .map(|_| ResumeResolution {
                agent: AgentCli::Hermes,
                command: shell_join_owned_tokens(tokens),
                exact: true,
            }),
        AgentCli::OpenClaw => openclaw_session_key(tokens).map(|key| ResumeResolution {
            agent: AgentCli::OpenClaw,
            command: openclaw_launch_command_with_session(tokens, key),
            exact: true,
        }),
    }
}

fn most_recent_fallback(tokens: &[String]) -> Option<String> {
    if is_bare_agent(tokens, AgentCli::Codex) {
        return Some(CODEX_LAST_RESUME_COMMAND.into());
    }
    if is_bare_agent(tokens, AgentCli::Claude) {
        return Some(CLAUDE_CONTINUE_COMMAND.into());
    }
    if is_bare_agent(tokens, AgentCli::Hermes) {
        return Some("hermes --continue".into());
    }
    None
}

fn resume_from_history(
    lines: &[String],
    agent: AgentCli,
    startup_tokens: &[String],
) -> Option<ResumeResolution> {
    if agent == AgentCli::Omx && !is_plain_interactive_omx(startup_tokens) {
        return None;
    }

    lines.iter().rev().find_map(|line| {
        if let Some(resolution) =
            resume_command_in_line(line).filter(|resolution| resolution.agent == agent)
        {
            return Some(resolution);
        }

        match agent {
            AgentCli::Codex => codex_session_id_line(line),
            AgentCli::Claude => claude_session_id_line(line),
            AgentCli::Hermes => hermes_session_line(line),
            AgentCli::Omx => codex_resume_uuid_in_line(line).map(|id| ResumeResolution {
                agent: AgentCli::Omx,
                command: omx_resume_command(startup_tokens, id.as_str()),
                exact: true,
            }),
            AgentCli::OpenClaw => None,
        }
    })
}

fn codex_resume_uuid_in_line(line: &str) -> Option<String> {
    let index = line.find("codex resume")?;
    let tokens = split_shell_words(line[index..].trim())?;
    if agent_from_tokens(&tokens) != Some(AgentCli::Codex)
        || tokens.get(1).map(String::as_str) != Some("resume")
    {
        return None;
    }
    let target = first_positional_after(&tokens, 2)?;
    looks_like_uuid(target).then(|| target.to_string())
}

fn is_plain_interactive_omx(tokens: &[String]) -> bool {
    agent_from_tokens(tokens) == Some(AgentCli::Omx) && first_omx_positional_index(tokens).is_none()
}

fn omx_resume_command(tokens: &[String], id: &str) -> String {
    let mut output = Vec::with_capacity(tokens.len() + 2);
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index].as_str();
        if option_takes_variadic_values(token) {
            index += 1;
            while let Some(value) = tokens.get(index).filter(|value| !value.starts_with('-')) {
                output.push(format!("--image={value}"));
                index += 1;
            }
            continue;
        }

        // A bare worktree flag consumes the next positional token as its
        // optional branch name. Use its explicit empty form before appending
        // `resume`. Variadic image values are likewise attached above so they
        // cannot swallow the appended subcommand.
        if is_bare_worktree_option(token)
            && !tokens
                .get(index + 1)
                .is_some_and(|next| is_omx_worktree_name(next))
        {
            output.push(format!("{token}="));
        } else {
            output.push(tokens[index].clone());
        }
        index += 1;
    }
    output.push("resume".to_string());
    output.push(id.to_string());
    shell_join_owned_tokens(&output)
}

fn first_omx_positional_index(tokens: &[String]) -> Option<usize> {
    let mut index = 1;
    while index < tokens.len() {
        let token = tokens[index].as_str();
        if token == "--" {
            return (index + 1 < tokens.len()).then_some(index + 1);
        }
        if is_bare_worktree_option(token) {
            // OMX accepts an optional, space-separated worktree branch name;
            // unlike ordinary value options, another flag means "detached".
            index += if tokens
                .get(index + 1)
                .is_some_and(|next| is_omx_worktree_name(next))
            {
                2
            } else {
                1
            };
        } else if option_takes_variadic_values(token) {
            index += 1;
            while tokens
                .get(index)
                .is_some_and(|value| !value.starts_with('-'))
            {
                index += 1;
            }
        } else if token.starts_with('-') {
            index += if option_takes_value(token) && index + 1 < tokens.len() {
                2
            } else {
                1
            };
        } else {
            return Some(index);
        }
    }
    None
}

fn is_bare_worktree_option(token: &str) -> bool {
    matches!(token, "-w" | "--worktree")
}

fn is_omx_worktree_name(token: &str) -> bool {
    !token.is_empty() && !token.starts_with('-') && !token.contains(':')
}

fn resume_command_in_line(line: &str) -> Option<ResumeResolution> {
    for marker in [
        "codex resume",
        "claude --resume",
        "claude -r",
        "omx resume",
        "hermes --resume",
        "hermes -r",
    ] {
        let Some(index) = line.find(marker) else {
            continue;
        };
        let command = line[index..].trim();
        let tokens = split_shell_words(command).or_else(|| {
            command
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
                .into()
        })?;
        if let Some(resolution) = exact_resume_from_startup(&tokens) {
            return Some(resolution);
        }
    }
    None
}

fn hermes_session_line(line: &str) -> Option<ResumeResolution> {
    let trimmed = line.trim();
    let id = trimmed.strip_prefix("Session:")?.trim();
    valid_resume_target(id).then(|| ResumeResolution {
        agent: AgentCli::Hermes,
        command: shell_join_tokens(&["hermes", "--resume", id]),
        exact: true,
    })
}

fn claude_session_id_line(line: &str) -> Option<ResumeResolution> {
    let trimmed = line.trim();
    let id = trimmed
        .strip_prefix("Session ID:")
        .or_else(|| trimmed.strip_prefix("session_id:"))?
        .trim();
    valid_resume_target(id).then(|| ResumeResolution {
        agent: AgentCli::Claude,
        command: shell_join_tokens(&["claude", "--resume", id]),
        exact: true,
    })
}

fn codex_session_id_line(line: &str) -> Option<ResumeResolution> {
    let trimmed = line.trim();
    let id = trimmed
        .strip_prefix("Session ID:")
        .or_else(|| trimmed.strip_prefix("session_id:"))?
        .trim();
    (valid_resume_target(id) && looks_like_uuid(id)).then(|| ResumeResolution {
        agent: AgentCli::Codex,
        command: shell_join_tokens(&["codex", "resume", "--no-alt-screen", id]),
        exact: true,
    })
}

fn parse_startup_command(command: &str) -> Option<ParsedStartup> {
    let tokens = split_shell_words(command)?;
    if tokens.is_empty() {
        return None;
    }

    if is_supported_shell_wrapper(tokens.first()?) {
        let command_index = tokens
            .iter()
            .position(|token| shell_flag_runs_command(token))?
            + 1;
        let inner_command = tokens.get(command_index)?;
        if tokens.len() == command_index + 1 {
            let inner_tokens = split_shell_words(inner_command)?;
            return Some(ParsedStartup::wrapped_command(
                &tokens,
                command_index,
                inner_tokens,
            ));
        }
    }

    Some(ParsedStartup::direct_command(tokens))
}

fn agent_counts(tiles: &[&TileSpec]) -> HashMap<AgentCli, usize> {
    let mut counts = HashMap::new();
    for tile in tiles
        .iter()
        .filter(|tile| tile.tile_kind == TileKind::Terminal)
    {
        let Some(command) = tile.startup_command.as_deref() else {
            continue;
        };
        let Some(parsed) = parse_startup_command(command) else {
            continue;
        };
        let Some(agent) = agent_from_tokens(&parsed.direct_tokens) else {
            continue;
        };
        *counts.entry(agent).or_insert(0) += 1;
    }
    counts
}

fn histories_by_tile(histories: &[SavedTerminalHistory]) -> HashMap<&str, &SavedTerminalHistory> {
    histories
        .iter()
        .map(|history| (history.tile_id.as_str(), history))
        .collect()
}

fn agent_from_tokens(tokens: &[String]) -> Option<AgentCli> {
    match command_basename(tokens.first()?.as_str()) {
        "codex" => Some(AgentCli::Codex),
        "claude" => Some(AgentCli::Claude),
        "omx" => Some(AgentCli::Omx),
        "hermes" => Some(AgentCli::Hermes),
        "openclaw" => Some(AgentCli::OpenClaw),
        _ => None,
    }
}

fn is_bare_agent(tokens: &[String], agent: AgentCli) -> bool {
    agent_from_tokens(tokens) == Some(agent) && tokens.len() == 1
}

fn first_positional_after(tokens: &[String], start_index: usize) -> Option<&str> {
    first_positional_index_after(tokens, start_index)
        .and_then(|index| tokens.get(index))
        .map(String::as_str)
}

fn first_positional_index_after(tokens: &[String], start_index: usize) -> Option<usize> {
    let mut index = start_index;
    while index < tokens.len() {
        let token = tokens[index].as_str();
        if token == "--" {
            return (index + 1 < tokens.len()).then_some(index + 1);
        }
        if token.starts_with('-') {
            index += if option_takes_value(token) && index + 1 < tokens.len() {
                2
            } else {
                1
            };
        } else {
            return Some(index);
        }
    }
    None
}

fn option_value<'a>(tokens: &'a [String], names: &[&str]) -> Option<&'a str> {
    let mut index = 1;
    while index < tokens.len() {
        let token = tokens[index].as_str();
        if names.contains(&token) {
            return tokens.get(index + 1).map(String::as_str);
        }
        if let Some((name, value)) = token.split_once('=')
            && names.contains(&name)
        {
            return Some(value);
        }
        index += 1;
    }
    None
}

fn option_takes_value(option: &str) -> bool {
    matches!(
        option,
        "-a" | "--ask-for-approval"
            | "-C"
            | "--cd"
            | "-c"
            | "--config"
            | "-m"
            | "--model"
            | "-p"
            | "--profile"
            | "-s"
            | "--sandbox"
            | "--add-dir"
            | "--disable"
            | "--enable"
            | "--local-provider"
            | "--remote"
            | "--remote-auth-token-env"
            | "--codex-home"
            | "--resume"
            | "-r"
            | "--session-id"
            | "--name"
            | "--session"
            | "--custom"
    )
}

fn option_takes_variadic_values(option: &str) -> bool {
    matches!(option, "-i" | "--image")
}

fn claude_session_name(tokens: &[String]) -> Option<String> {
    option_value(tokens, &["--name"]).map(str::to_string)
}

fn codex_resume_command(tokens: &[String]) -> String {
    let mut output = tokens.to_vec();
    if !output.iter().any(|token| token == "--no-alt-screen") {
        output.insert(2, "--no-alt-screen".to_string());
    }
    shell_join_owned_tokens(&output)
}

fn claude_resume_command_for_named_session(tokens: &[String], name: &str) -> String {
    let mut output = tokens.to_vec();
    replace_option_with_value(&mut output, &["--name"], "--resume", name);
    shell_join_owned_tokens(&output)
}

fn replace_option_with_value(
    tokens: &mut Vec<String>,
    names: &[&str],
    replacement: &str,
    value: &str,
) {
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index].as_str();
        if names.contains(&token) {
            tokens[index] = replacement.to_string();
            if let Some(next) = tokens.get_mut(index + 1) {
                *next = value.to_string();
            } else {
                tokens.push(value.to_string());
            }
            return;
        }
        if let Some((name, _)) = token.split_once('=')
            && names.contains(&name)
        {
            tokens[index] = format!("{replacement}={value}");
            return;
        }
        index += 1;
    }
    tokens.push(replacement.to_string());
    tokens.push(value.to_string());
}

fn openclaw_session_key(tokens: &[String]) -> Option<&str> {
    option_value(tokens, &["--session"])
}

fn is_openclaw_tui_command(tokens: &[String]) -> bool {
    if agent_from_tokens(tokens) != Some(AgentCli::OpenClaw) {
        return false;
    }
    matches!(
        tokens.get(1).map(String::as_str),
        None | Some("tui" | "chat" | "terminal")
    )
}

fn openclaw_launch_command_with_session(tokens: &[String], key: &str) -> String {
    let mut output = tokens.to_vec();
    if let Some(index) = output.iter().position(|token| token == "--session") {
        if let Some(value) = output.get_mut(index + 1) {
            *value = key.to_string();
        }
    } else if let Some(index) = output
        .iter()
        .position(|token| token.starts_with("--session="))
    {
        output[index] = format!("--session={key}");
    } else {
        output.push("--session".to_string());
        output.push(key.to_string());
    }
    shell_join_owned_tokens(&output)
}

fn stable_claude_name(tile: &TileSpec, workspace_root: &Path) -> String {
    format!(
        "TerminalTiler {} {}",
        short_human_label(tile),
        stable_tile_hash(tile, workspace_root)
    )
}

fn stable_openclaw_key(tile: &TileSpec, workspace_root: &Path) -> String {
    format!("terminaltiler-{}", stable_tile_hash(tile, workspace_root))
}

fn short_human_label(tile: &TileSpec) -> String {
    let label = tile.title.trim();
    if label.is_empty() {
        tile.id.as_str()
    } else {
        label
    }
    .chars()
    .map(|ch| {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            ch
        } else {
            '-'
        }
    })
    .collect::<String>()
    .trim_matches('-')
    .chars()
    .take(24)
    .collect::<String>()
}

fn stable_tile_hash(tile: &TileSpec, workspace_root: &Path) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for part in [
        workspace_root.display().to_string(),
        tile.id.clone(),
        tile.title.clone(),
        tile.agent_label.clone(),
    ] {
        for byte in part.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn valid_resume_target(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value
            .chars()
            .any(|ch| matches!(ch, ';' | '&' | '|' | '<' | '>'))
}

fn looks_like_uuid(value: &str) -> bool {
    value.len() == 36
        && value.chars().enumerate().all(|(index, ch)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                ch == '-'
            } else {
                ch.is_ascii_hexdigit()
            }
        })
}

fn is_supported_shell_wrapper(command: &str) -> bool {
    matches!(command_basename(command), "bash" | "sh" | "zsh")
}

fn shell_flag_runs_command(flag: &str) -> bool {
    flag.starts_with('-') && flag.contains('c')
}

fn command_basename(command: &str) -> &str {
    command.rsplit(['/', '\\']).next().unwrap_or(command).trim()
}

fn shell_join_tokens(tokens: &[&str]) -> String {
    tokens
        .iter()
        .map(|token| shell_word(token))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_join_owned_tokens(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|token| shell_word(token))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_word(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':' | '='))
    {
        value.to_string()
    } else {
        shell_quote(value)
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn split_shell_words(command: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut quote: Option<char> = None;
    let mut saw_word = false;

    while let Some(ch) = chars.next() {
        match quote {
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                } else {
                    current.push(ch);
                    saw_word = true;
                }
            }
            Some('"') => match ch {
                '"' => quote = None,
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                        saw_word = true;
                    }
                }
                _ => {
                    current.push(ch);
                    saw_word = true;
                }
            },
            Some(_) => unreachable!(),
            None => match ch {
                '\'' | '"' => {
                    quote = Some(ch);
                    saw_word = true;
                }
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                        saw_word = true;
                    }
                }
                ch if ch.is_whitespace() => {
                    if saw_word {
                        words.push(std::mem::take(&mut current));
                        saw_word = false;
                    }
                }
                ';' | '&' | '|' | '<' | '>' => return None,
                _ => {
                    current.push(ch);
                    saw_word = true;
                }
            },
        }
    }

    if quote.is_some() {
        return None;
    }
    if saw_word {
        words.push(current);
    }
    Some(words)
}

#[cfg(test)]
mod tests {
    use super::{
        CLAUDE_CONTINUE_COMMAND, CODEX_LAST_RESUME_COMMAND, initial_startup_overrides_for_tiles,
        restore_startup_override, restore_startup_override_for_tab_tile,
        restore_startup_overrides_for_saved_tab, restore_startup_overrides_for_tab_tile_sets,
        restore_startup_overrides_for_tiles, saved_resume_command_for_tile,
    };
    use crate::model::layout::{TileKind, WorkingDirectory, tile};
    use crate::storage::session_store::SavedTerminalHistory;
    use std::path::Path;

    fn terminal_tile(id: &str, command: Option<&str>) -> crate::model::layout::TileSpec {
        tile(
            id,
            id,
            "Agent",
            "accent-cyan",
            WorkingDirectory::WorkspaceRoot,
            command,
        )
        .tile_specs()
        .remove(0)
    }

    #[test]
    fn rewrites_direct_single_codex_and_claude_to_native_resume_commands() {
        assert_eq!(
            restore_startup_override("codex").as_deref(),
            Some("codex resume --last --no-alt-screen")
        );
        assert_eq!(
            restore_startup_override("claude").as_deref(),
            Some("claude --continue")
        );
    }

    #[test]
    fn leaves_existing_most_recent_resume_commands_unchanged() {
        assert_eq!(restore_startup_override("codex resume --last"), None);
        assert_eq!(restore_startup_override("claude --continue"), None);
    }

    #[test]
    fn exact_resume_commands_are_normalized_to_precise_resume_forms() {
        assert_eq!(
            restore_startup_override("codex resume 123e4567-e89b-12d3-a456-426614174000")
                .as_deref(),
            Some("codex resume --no-alt-screen 123e4567-e89b-12d3-a456-426614174000")
        );
        assert_eq!(
            restore_startup_override("claude --resume my-session").as_deref(),
            Some("claude --resume my-session")
        );
        assert_eq!(
            restore_startup_override("hermes --resume 20260225_143052_a1b2c3").as_deref(),
            Some("hermes --resume 20260225_143052_a1b2c3")
        );
        assert_eq!(
            restore_startup_override("omx resume --project 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4")
                .as_deref(),
            Some("omx resume --project 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4")
        );
        assert_eq!(
            restore_startup_override(
                "omx --madmax --high resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4"
            )
            .as_deref(),
            Some("omx --madmax --high resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4")
        );
        assert_eq!(
            restore_startup_override(
                "omx --madmax resume --project 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4"
            )
            .as_deref(),
            Some("omx --madmax resume --project 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4")
        );
    }

    #[test]
    fn explicit_resume_commands_preserve_user_options() {
        assert_eq!(
            restore_startup_override(
                "codex resume --cd /repo --config profile=foo 123e4567-e89b-12d3-a456-426614174000"
            )
            .as_deref(),
            Some(
                "codex resume --no-alt-screen --cd /repo --config profile=foo 123e4567-e89b-12d3-a456-426614174000"
            )
        );
        assert_eq!(
            restore_startup_override("claude --resume my-session --model sonnet").as_deref(),
            Some("claude --resume my-session --model sonnet")
        );
        assert_eq!(
            restore_startup_override("hermes --resume 20260225_143052_a1b2c3 --profile prod")
                .as_deref(),
            Some("hermes --resume 20260225_143052_a1b2c3 --profile prod")
        );
        assert_eq!(
            restore_startup_override(
                "omx resume --project --codex-home /tmp 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4"
            )
            .as_deref(),
            Some("omx resume --project --codex-home /tmp 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4")
        );
    }

    #[test]
    fn leaves_agent_commands_with_non_resume_args_unchanged() {
        assert_eq!(restore_startup_override("codex exec \"summarize\""), None);
        assert_eq!(restore_startup_override("codex --model gpt-5.4"), None);
        assert_eq!(restore_startup_override("claude --model sonnet"), None);
        assert_eq!(restore_startup_override("omx ralph"), None);
        assert_eq!(
            restore_startup_override("omx --madmax team 3:executor"),
            None
        );
        assert_eq!(restore_startup_override("omx --high exec echo"), None);
        assert_eq!(
            restore_startup_override("bash -lc 'codex exec \"summarize\"'"),
            None
        );
        assert_eq!(
            restore_startup_override("bash -lc 'claude --model sonnet'"),
            None
        );
    }

    #[test]
    fn rewrites_simple_shell_wrappers() {
        assert_eq!(
            restore_startup_override("bash -ic 'claude'").as_deref(),
            Some("bash -ic 'claude --continue'")
        );
        assert_eq!(
            restore_startup_override("/bin/bash -lc \"codex\"").as_deref(),
            Some("/bin/bash -lc 'codex resume --last --no-alt-screen'")
        );
        assert_eq!(
            restore_startup_override(
                "bash --init-file '/tmp/a b' -ic 'omx --madmax --high resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4'"
            )
            .as_deref(),
            Some(
                "bash --init-file '/tmp/a b' -ic 'omx --madmax --high resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4'"
            )
        );
    }

    #[test]
    fn leaves_non_agent_and_complex_wrapper_commands_unchanged() {
        assert_eq!(restore_startup_override("npm test"), None);
        assert_eq!(restore_startup_override("bash -lc 'omx ralph'"), None);
        assert_eq!(restore_startup_override("bash -lc 'cd app && codex'"), None);
    }

    #[test]
    fn saved_tab_restore_avoids_most_recent_for_duplicate_agent_panes() {
        let tiles = vec![
            terminal_tile("codex-a", Some("codex")),
            terminal_tile("codex-b", Some("codex")),
            terminal_tile("claude-a", Some("claude")),
        ];

        let overrides = restore_startup_overrides_for_saved_tab(&tiles, Path::new("/repo"), &[]);

        assert_eq!(overrides.get("codex-a"), None);
        assert_eq!(overrides.get("codex-b"), None);
        assert_eq!(
            overrides.get("claude-a").map(String::as_str),
            Some(CLAUDE_CONTINUE_COMMAND)
        );
    }

    #[test]
    fn saved_resume_command_wins_even_with_duplicate_agent_panes() {
        let tiles = vec![
            terminal_tile("codex-a", Some("codex")),
            terminal_tile("codex-b", Some("codex")),
        ];
        let history = vec![SavedTerminalHistory {
            tile_id: "codex-b".into(),
            lines: Vec::new(),
            resume_command: Some("codex resume --no-alt-screen exact-session".into()),
        }];

        let overrides =
            restore_startup_overrides_for_saved_tab(&tiles, Path::new("/repo"), &history);

        assert_eq!(overrides.get("codex-a"), None);
        assert_eq!(
            overrides.get("codex-b").map(String::as_str),
            Some("codex resume --no-alt-screen exact-session")
        );
    }

    #[test]
    fn terminal_history_resume_hints_are_used_for_hermes() {
        let tiles = vec![terminal_tile("hermes", Some("hermes"))];
        let history = vec![SavedTerminalHistory {
            tile_id: "hermes".into(),
            lines: vec![
                "Resume this session with:".into(),
                "  hermes --resume 20260225_143052_a1b2c3".into(),
            ],
            resume_command: None,
        }];

        let overrides =
            restore_startup_overrides_for_saved_tab(&tiles, Path::new("/repo"), &history);

        assert_eq!(
            overrides.get("hermes").map(String::as_str),
            Some("hermes --resume 20260225_143052_a1b2c3")
        );
    }

    #[test]
    fn terminal_history_session_id_is_interpreted_for_matching_agent() {
        let tiles = vec![terminal_tile("codex", Some("codex"))];
        let history = vec![SavedTerminalHistory {
            tile_id: "codex".into(),
            lines: vec!["Session ID: 123e4567-e89b-12d3-a456-426614174000".into()],
            resume_command: None,
        }];

        let overrides =
            restore_startup_overrides_for_saved_tab(&tiles, Path::new("/repo"), &history);

        assert_eq!(
            overrides.get("codex").map(String::as_str),
            Some("codex resume --no-alt-screen 123e4567-e89b-12d3-a456-426614174000")
        );
    }

    #[test]
    fn omx_wraps_exact_codex_history_resume_with_existing_flags() {
        let tiles = vec![terminal_tile("omx", Some("omx --madmax --high"))];
        let history = vec![SavedTerminalHistory {
            tile_id: "omx".into(),
            lines: vec![
                "To continue this session, run:".into(),
                "  codex resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4".into(),
            ],
            resume_command: None,
        }];

        let overrides =
            restore_startup_overrides_for_saved_tab(&tiles, Path::new("/repo"), &history);

        assert_eq!(
            overrides.get("omx").map(String::as_str),
            Some("omx --madmax --high resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4")
        );
    }

    #[test]
    fn omx_history_resume_preserves_shell_wrapper() {
        let tiles = vec![terminal_tile(
            "omx",
            Some("bash --init-file '/tmp/a b' -ic 'omx --madmax --high'"),
        )];
        let history = vec![SavedTerminalHistory {
            tile_id: "omx".into(),
            lines: vec!["codex resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4".into()],
            resume_command: None,
        }];

        let overrides =
            restore_startup_overrides_for_saved_tab(&tiles, Path::new("/repo"), &history);

        assert_eq!(
            overrides.get("omx").map(String::as_str),
            Some(
                "bash --init-file '/tmp/a b' -ic 'omx --madmax --high resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4'"
            )
        );
    }

    #[test]
    fn omx_history_resume_preserves_value_flags_and_worktree_modes() {
        let id = "019f7fe2-a4b8-7012-8b6b-e45e0b55dff4";
        for (startup, expected) in [
            (
                "omx --notify-temp --custom ops",
                "omx --notify-temp --custom ops resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4",
            ),
            (
                "omx --worktree feature/session-resume --high",
                "omx --worktree feature/session-resume --high resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4",
            ),
            (
                "omx --worktree --high",
                "omx --worktree= --high resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4",
            ),
            (
                "omx --sandbox workspace-write --ask-for-approval on-request --add-dir ../shared",
                "omx --sandbox workspace-write --ask-for-approval on-request --add-dir ../shared resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4",
            ),
            (
                "omx --image first.png second.png --high",
                "omx --image=first.png --image=second.png --high resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4",
            ),
        ] {
            let tile = terminal_tile("omx", Some(startup));
            let lines = vec![format!("codex resume {id}")];
            assert_eq!(
                saved_resume_command_for_tile(&tile, Path::new("/repo"), &lines).as_deref(),
                Some(expected),
                "startup: {startup}"
            );
        }
    }

    #[test]
    fn omx_subcommands_are_not_replaced_by_nested_codex_history() {
        for command in [
            "omx ralph",
            "omx --madmax team 3:executor",
            "omx --high exec echo",
        ] {
            let tiles = vec![terminal_tile("omx", Some(command))];
            let history = vec![SavedTerminalHistory {
                tile_id: "omx".into(),
                lines: vec!["codex resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4".into()],
                resume_command: None,
            }];

            let overrides =
                restore_startup_overrides_for_saved_tab(&tiles, Path::new("/repo"), &history);
            assert_eq!(overrides.get("omx"), None, "command: {command}");
        }
    }

    #[test]
    fn omx_history_resume_rejects_malformed_or_injection_like_ids() {
        for target in [
            "not-a-session",
            "019f7fe2a4b870128b6be45e0b55dff4ffff",
            "019f7fe2-a4b8-7012-8b6b-e45e0b55dff4;touch-pwned",
            "$(touch-pwned)",
        ] {
            let tile = terminal_tile("omx", Some("omx --madmax --high"));
            let lines = vec![format!("codex resume '{target}'")];
            assert_eq!(
                saved_resume_command_for_tile(&tile, Path::new("/repo"), &lines),
                None,
                "target: {target}"
            );
        }
    }

    #[test]
    fn omx_restore_rejects_malformed_or_cross_agent_saved_commands() {
        let tile = terminal_tile("omx", Some("omx --madmax --high"));
        for command in [
            "omx --madmax --high resume not-a-uuid",
            "codex resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4",
            "omx --madmax --high exec echo",
        ] {
            let history = vec![SavedTerminalHistory {
                tile_id: "omx".into(),
                lines: Vec::new(),
                resume_command: Some(command.into()),
            }];

            let overrides = restore_startup_overrides_for_saved_tab(
                std::slice::from_ref(&tile),
                Path::new("/repo"),
                &history,
            );
            assert_eq!(overrides.get("omx"), None, "command: {command}");
        }

        assert_eq!(restore_startup_override("omx resume not-a-uuid"), None);
    }

    #[test]
    fn duplicate_omx_panes_only_resume_the_tile_with_an_exact_id() {
        let tiles = vec![
            terminal_tile("omx-a", Some("omx --madmax --high")),
            terminal_tile("omx-b", Some("omx --madmax --high")),
        ];
        let history = vec![SavedTerminalHistory {
            tile_id: "omx-b".into(),
            lines: vec!["codex resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4".into()],
            resume_command: None,
        }];

        let overrides =
            restore_startup_overrides_for_saved_tab(&tiles, Path::new("/repo"), &history);

        assert_eq!(overrides.get("omx-a"), None);
        assert_eq!(
            overrides.get("omx-b").map(String::as_str),
            Some("omx --madmax --high resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4")
        );
    }

    #[test]
    fn captured_omx_resume_command_round_trips_through_saved_history() {
        let tile = terminal_tile("omx", Some("omx --madmax --high"));
        let lines = vec!["codex resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4".into()];
        let resume_command = saved_resume_command_for_tile(&tile, Path::new("/repo"), &lines);
        assert_eq!(
            resume_command.as_deref(),
            Some("omx --madmax --high resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4")
        );

        let history = vec![SavedTerminalHistory {
            tile_id: "omx".into(),
            lines: Vec::new(),
            resume_command,
        }];
        let overrides = restore_startup_overrides_for_saved_tab(
            std::slice::from_ref(&tile),
            Path::new("/repo"),
            &history,
        );
        assert_eq!(
            overrides.get("omx").map(String::as_str),
            Some("omx --madmax --high resume 019f7fe2-a4b8-7012-8b6b-e45e0b55dff4")
        );
    }

    #[test]
    fn initial_openclaw_startup_gets_stable_session_key_without_using_shared_main() {
        let tiles = [
            terminal_tile("open-a", Some("openclaw chat")),
            terminal_tile("open-b", Some("openclaw terminal")),
        ];

        let overrides = initial_startup_overrides_for_tiles(tiles.iter(), Path::new("/repo"));
        let first = overrides.get("open-a").expect("first key");
        let second = overrides.get("open-b").expect("second key");

        assert!(first.starts_with("openclaw chat --session terminaltiler-"));
        assert!(second.starts_with("openclaw terminal --session terminaltiler-"));
        assert_ne!(first, second);
    }

    #[test]
    fn saved_tab_restore_does_not_synthesize_openclaw_session_key() {
        let tiles = vec![terminal_tile("open", Some("openclaw tui --local"))];

        let overrides = restore_startup_overrides_for_saved_tab(&tiles, Path::new("/repo"), &[]);

        assert_eq!(overrides.get("open"), None);
    }

    #[test]
    fn initial_claude_startup_gets_stable_name_without_persisting_synthetic_resume() {
        let tiles = vec![terminal_tile("claude-a", Some("claude"))];

        let overrides = restore_startup_overrides_for_saved_tab(&tiles, Path::new("/repo"), &[]);

        // Restore still prefers Claude's documented most-recent shortcut for a
        // single legacy pane; normal startup naming is covered by saved command
        // capture below.
        assert_eq!(
            overrides.get("claude-a").map(String::as_str),
            Some("claude --continue")
        );

        let initial_overrides =
            initial_startup_overrides_for_tiles(tiles.iter(), Path::new("/repo"));
        let initial_command = initial_overrides
            .get("claude-a")
            .expect("stable initial launch command");
        assert!(initial_command.starts_with("claude --name 'TerminalTiler claude-a "));
        assert_eq!(
            saved_resume_command_for_tile(&tiles[0], Path::new("/repo"), &[]),
            None
        );
    }

    #[test]
    fn bare_openclaw_startup_does_not_persist_synthetic_session_key() {
        let tile = terminal_tile("open", Some("openclaw tui --local"));

        assert_eq!(
            saved_resume_command_for_tile(&tile, Path::new("/repo"), &[]),
            None
        );
    }

    #[test]
    fn capture_resume_command_preserves_explicit_openclaw_session() {
        let tile = terminal_tile("open", Some("openclaw tui --local --session project-auth"));

        assert_eq!(
            saved_resume_command_for_tile(&tile, Path::new("/repo"), &[]).as_deref(),
            Some("openclaw tui --local --session project-auth")
        );
    }

    #[test]
    fn builds_overrides_only_for_terminal_agent_tiles() {
        let layout = tile(
            "tile-1",
            "Primary",
            "Shell",
            "accent-cyan",
            WorkingDirectory::WorkspaceRoot,
            Some("codex"),
        );
        let mut tiles = layout.tile_specs();
        tiles.push(crate::model::layout::TileSpec {
            id: "web".into(),
            title: "Web".into(),
            agent_label: "Web".into(),
            accent_class: "accent-cyan".into(),
            working_directory: WorkingDirectory::WorkspaceRoot,
            startup_command: Some("claude".into()),
            connection_target: Default::default(),
            pane_groups: Vec::new(),
            reconnect_policy: Default::default(),
            applied_role_id: None,
            output_helpers: Vec::new(),
            tile_kind: TileKind::WebView,
            url: None,
            auto_refresh_seconds: None,
        });

        let overrides = restore_startup_overrides_for_tiles(&tiles);
        assert_eq!(overrides.len(), 1);
        assert_eq!(
            overrides.get("tile-1").map(String::as_str),
            Some("codex resume --last --no-alt-screen")
        );
    }

    #[test]
    fn tab_scoped_lookup_keeps_duplicate_tile_ids_distinct() {
        let overrides_by_tab = restore_startup_overrides_for_tab_tile_sets([
            tile(
                "primary",
                "Primary",
                "Primary",
                "accent-cyan",
                WorkingDirectory::WorkspaceRoot,
                Some("codex"),
            )
            .tile_specs(),
            tile(
                "primary",
                "Primary",
                "Primary",
                "accent-purple",
                WorkingDirectory::WorkspaceRoot,
                Some("claude"),
            )
            .tile_specs(),
        ]);

        assert_eq!(
            restore_startup_override_for_tab_tile(&overrides_by_tab, 0, "primary"),
            Some(CODEX_LAST_RESUME_COMMAND)
        );
        assert_eq!(
            restore_startup_override_for_tab_tile(&overrides_by_tab, 1, "primary"),
            Some(CLAUDE_CONTINUE_COMMAND)
        );
        assert_eq!(
            restore_startup_override_for_tab_tile(&overrides_by_tab, 2, "primary"),
            None
        );
    }
}
