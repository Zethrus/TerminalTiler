use std::collections::HashMap;

use crate::model::layout::{TileKind, TileSpec};

pub type RestoreStartupOverrideMap = HashMap<String, String>;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub type RestoreStartupOverridesByTab = Vec<RestoreStartupOverrideMap>;

const CODEX_RESUME_COMMAND: &str = "codex resume --last --no-alt-screen";
const CLAUDE_RESUME_COMMAND: &str = "claude --continue";

pub fn restore_startup_override(command: &str) -> Option<String> {
    let tokens = split_shell_words(command)?;
    if tokens.is_empty() {
        return None;
    }

    direct_agent_resume_command(&tokens).or_else(|| shell_wrapper_resume_command(&tokens))
}

pub fn restore_startup_overrides_for_tiles<'a>(
    tiles: impl IntoIterator<Item = &'a TileSpec>,
) -> RestoreStartupOverrideMap {
    tiles
        .into_iter()
        .filter(|tile| tile.tile_kind == TileKind::Terminal)
        .filter_map(|tile| {
            let command = tile.startup_command.as_deref()?;
            restore_startup_override(command)
                .map(|override_command| (tile.id.clone(), override_command))
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

fn direct_agent_resume_command(tokens: &[String]) -> Option<String> {
    let program = command_basename(tokens.first()?.as_str());
    match (program, tokens.len()) {
        ("codex", 1) => Some(CODEX_RESUME_COMMAND.into()),
        ("claude", 1) => Some(CLAUDE_RESUME_COMMAND.into()),
        _ => None,
    }
}

fn shell_wrapper_resume_command(tokens: &[String]) -> Option<String> {
    let shell = tokens.first()?;
    if !is_supported_shell_wrapper(shell) {
        return None;
    }

    let command_index = tokens
        .iter()
        .position(|token| shell_flag_runs_command(token))?
        + 1;
    let inner_command = tokens.get(command_index)?;
    if tokens.len() != command_index + 1 {
        return None;
    }

    let inner_tokens = split_shell_words(inner_command)?;
    let resume_command = direct_agent_resume_command(&inner_tokens)?;
    let mut wrapper = tokens[..command_index].to_vec();
    wrapper.push(shell_quote(&resume_command));
    Some(wrapper.join(" "))
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
        CLAUDE_RESUME_COMMAND, CODEX_RESUME_COMMAND, restore_startup_override,
        restore_startup_override_for_tab_tile, restore_startup_overrides_for_tab_tile_sets,
        restore_startup_overrides_for_tiles,
    };
    use crate::model::layout::{TileKind, WorkingDirectory, tile};

    #[test]
    fn rewrites_direct_codex_and_claude_to_native_resume_commands() {
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
    fn leaves_existing_resume_commands_unchanged() {
        assert_eq!(restore_startup_override("codex resume --last"), None);
        assert_eq!(restore_startup_override("claude --continue"), None);
    }

    #[test]
    fn leaves_agent_commands_with_args_unchanged() {
        assert_eq!(restore_startup_override("codex exec \"summarize\""), None);
        assert_eq!(restore_startup_override("codex --model gpt-5.4"), None);
        assert_eq!(restore_startup_override("claude --model sonnet"), None);
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
    }

    #[test]
    fn leaves_non_agent_and_omx_wrapper_commands_unchanged() {
        assert_eq!(restore_startup_override("npm test"), None);
        assert_eq!(restore_startup_override("omx ralph"), None);
        assert_eq!(restore_startup_override("bash -lc 'omx ralph'"), None);
        assert_eq!(restore_startup_override("bash -lc 'cd app && codex'"), None);
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
            Some(CODEX_RESUME_COMMAND)
        );
        assert_eq!(
            restore_startup_override_for_tab_tile(&overrides_by_tab, 1, "primary"),
            Some(CLAUDE_RESUME_COMMAND)
        );
        assert_eq!(
            restore_startup_override_for_tab_tile(&overrides_by_tab, 2, "primary"),
            None
        );
    }
}
