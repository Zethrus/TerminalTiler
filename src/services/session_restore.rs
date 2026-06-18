use crate::model::assets::RestoreLaunchMode;
use crate::storage::session_store::SavedSession;

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreStartupAction {
    StartFresh,
    ResumeAndRerun,
    ResumeAsShells,
}

pub fn session_for_restore_mode(
    saved_session: &SavedSession,
    restore_mode: RestoreLaunchMode,
) -> Option<SavedSession> {
    match restore_mode {
        RestoreLaunchMode::Prompt => None,
        RestoreLaunchMode::RerunStartupCommands => Some(saved_session.clone()),
        RestoreLaunchMode::ShellOnly => Some(shell_only_session(saved_session)),
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn session_for_startup_action(
    saved_session: &SavedSession,
    action: RestoreStartupAction,
) -> Option<SavedSession> {
    match action {
        RestoreStartupAction::StartFresh => None,
        RestoreStartupAction::ResumeAndRerun => Some(saved_session.clone()),
        RestoreStartupAction::ResumeAsShells => Some(shell_only_session(saved_session)),
    }
}

pub fn shell_only_session(saved_session: &SavedSession) -> SavedSession {
    let mut next = saved_session.clone();
    for tab in &mut next.tabs {
        let mut tile_specs = tab.preset.layout.tile_specs();
        for tile in &mut tile_specs {
            tile.startup_command = None;
        }
        tab.preset.layout = tab.preset.layout.with_tile_specs(&tile_specs);
    }
    next
}

pub fn flatten_window_sessions<'a, I>(
    sessions: I,
    active_window_id: Option<usize>,
) -> Option<SavedSession>
where
    I: IntoIterator<Item = (usize, &'a SavedSession)>,
{
    let mut tabs = Vec::new();
    let mut active_tab_index = 0usize;
    let mut current_offset = 0usize;
    let mut saw_active_window = false;
    let mut fallback_active_tab_index = None;

    for (window_id, session) in sessions {
        if session.tabs.is_empty() {
            continue;
        }

        fallback_active_tab_index.get_or_insert_with(|| {
            current_offset + session.active_tab_index.min(session.tabs.len() - 1)
        });

        if Some(window_id) == active_window_id {
            saw_active_window = true;
            active_tab_index =
                current_offset + session.active_tab_index.min(session.tabs.len() - 1);
        }

        current_offset += session.tabs.len();
        tabs.extend(session.tabs.clone());
    }

    if tabs.is_empty() {
        return None;
    }

    if !saw_active_window {
        active_tab_index = fallback_active_tab_index.unwrap_or(0);
    }

    Some(SavedSession {
        tabs,
        active_tab_index,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        RestoreStartupAction, flatten_window_sessions, session_for_restore_mode,
        session_for_startup_action,
    };
    use crate::model::assets::RestoreLaunchMode;
    use crate::model::layout::{ReconnectPolicy, WorkingDirectory, tile};
    use crate::model::preset::{ApplicationDensity, ThemeMode, WorkspacePreset};
    use crate::storage::session_store::{SavedSession, SavedTab};
    use std::path::PathBuf;

    fn sample_session() -> SavedSession {
        let mut preset = WorkspacePreset {
            id: "preset-1".into(),
            name: "Sample".into(),
            description: String::new(),
            tags: Vec::new(),
            root_label: "Workspace root".into(),
            workspace_root: None,
            theme: ThemeMode::System,
            density: ApplicationDensity::Compact,
            layout: tile(
                "tile-1",
                "Primary",
                "Shell",
                "accent-cyan",
                WorkingDirectory::WorkspaceRoot,
                Some("codex"),
            ),
        };
        let mut specs = preset.layout.tile_specs();
        specs[0].reconnect_policy = ReconnectPolicy::Always;
        preset.layout = preset.layout.with_tile_specs(&specs);
        SavedSession {
            tabs: vec![SavedTab {
                preset,
                workspace_root: PathBuf::from("."),
                custom_title: Some("Example".into()),
                terminal_zoom_steps: 2,
                terminal_history: vec![crate::storage::session_store::SavedTerminalHistory {
                    tile_id: "tile-1".into(),
                    lines: vec!["previous output".into()],
                }],
            }],
            active_tab_index: 0,
        }
    }

    #[test]
    fn shell_only_mode_clears_startup_commands_without_dropping_other_state() {
        let session = sample_session();
        let restored =
            session_for_restore_mode(&session, RestoreLaunchMode::ShellOnly).expect("session");
        let tiles = restored.tabs[0].preset.layout.tile_specs();
        assert_eq!(tiles[0].startup_command, None);
        assert_eq!(tiles[0].reconnect_policy, ReconnectPolicy::Always);
        assert_eq!(restored.tabs[0].custom_title.as_deref(), Some("Example"));
        assert_eq!(restored.tabs[0].terminal_zoom_steps, 2);
        assert_eq!(restored.tabs[0].terminal_history[0].tile_id, "tile-1");
    }

    #[test]
    fn rerun_mode_preserves_original_session() {
        let session = sample_session();
        let restored = session_for_restore_mode(&session, RestoreLaunchMode::RerunStartupCommands)
            .expect("session");
        let tiles = restored.tabs[0].preset.layout.tile_specs();
        assert_eq!(tiles[0].startup_command.as_deref(), Some("codex"));
    }

    #[test]
    fn startup_action_matches_requested_restore_behavior() {
        let session = sample_session();
        assert!(session_for_startup_action(&session, RestoreStartupAction::StartFresh).is_none());
        let rerun = session_for_startup_action(&session, RestoreStartupAction::ResumeAndRerun)
            .expect("rerun session");
        let shells = session_for_startup_action(&session, RestoreStartupAction::ResumeAsShells)
            .expect("shell session");
        assert_eq!(
            rerun.tabs[0].preset.layout.tile_specs()[0]
                .startup_command
                .as_deref(),
            Some("codex")
        );
        assert_eq!(
            shells.tabs[0].preset.layout.tile_specs()[0].startup_command,
            None
        );
    }

    #[test]
    fn flatten_window_sessions_preserves_all_tabs_and_offsets_active_index() {
        let mut first = sample_session();
        first.tabs[0].custom_title = Some("first".into());
        let mut second = sample_session();
        second.tabs[0].custom_title = Some("second-a".into());
        second.tabs.push(SavedTab {
            preset: second.tabs[0].preset.clone(),
            workspace_root: PathBuf::from("/tmp/second-b"),
            custom_title: Some("second-b".into()),
            terminal_zoom_steps: -1,
            terminal_history: Vec::new(),
        });
        second.active_tab_index = 1;

        let flattened = flatten_window_sessions([(10, &first), (20, &second)], Some(20))
            .expect("flattened session");

        assert_eq!(flattened.tabs.len(), 3);
        assert_eq!(flattened.active_tab_index, 2);
        assert_eq!(flattened.tabs[0].custom_title.as_deref(), Some("first"));
        assert_eq!(flattened.tabs[1].custom_title.as_deref(), Some("second-a"));
        assert_eq!(flattened.tabs[2].custom_title.as_deref(), Some("second-b"));
    }

    #[test]
    fn flatten_window_sessions_defaults_active_index_when_active_window_missing() {
        let session = sample_session();

        let flattened =
            flatten_window_sessions([(10, &session)], Some(99)).expect("flattened session");

        assert_eq!(flattened.tabs.len(), 1);
        assert_eq!(flattened.active_tab_index, 0);
    }

    #[test]
    fn flatten_window_sessions_ignores_empty_windows() {
        let empty = SavedSession {
            tabs: Vec::new(),
            active_tab_index: 0,
        };
        let session = sample_session();

        let flattened = flatten_window_sessions([(10, &empty), (20, &session)], Some(20))
            .expect("flattened session");

        assert_eq!(flattened.tabs.len(), 1);
        assert_eq!(flattened.active_tab_index, 0);
    }
}
