use std::path::Path;

use crate::model::assets::{PaneStatusSnapshot, WorkspaceAssets};
use crate::model::layout::TileSpec;
use crate::services::launch_resolution::resolve_tile_launch;

pub(crate) fn initial_status_snapshot(
    tile: &TileSpec,
    workspace_root: &Path,
    assets: &WorkspaceAssets,
) -> PaneStatusSnapshot {
    let connection_label = resolve_tile_launch(tile, workspace_root, assets)
        .map(|resolved| resolved.connection_label)
        .unwrap_or_else(|_| "launch-error".into());
    PaneStatusSnapshot {
        connection_label,
        location_label: tile.working_directory.short_label(),
        shell_label: tile.agent_label.clone(),
        helper_label: String::new(),
        helper_severity: None,
    }
}
