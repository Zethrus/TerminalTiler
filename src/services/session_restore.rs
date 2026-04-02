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

#[cfg(test)]
mod tests {
    use super::{RestoreStartupAction, session_for_restore_mode, session_for_startup_action};
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
}
