use crate::model::assets::{AgentRoleTemplate, ProjectSuggestion, WorkspaceAssets};
use crate::model::layout::{LayoutNode, TileSpec, generate_layout};

pub fn resize_layout(current_layout: &LayoutNode, tile_count: usize) -> LayoutNode {
    let next_layout = generate_layout(tile_count);
    let current_tiles = current_layout.tile_specs();
    let mut next_tiles = next_layout.tile_specs();

    for (index, tile) in next_tiles.iter_mut().enumerate() {
        if let Some(existing) = current_tiles.get(index) {
            tile.id = existing.id.clone();
            tile.title = existing.title.clone();
            tile.agent_label = existing.agent_label.clone();
            tile.accent_class = existing.accent_class.clone();
            tile.working_directory = existing.working_directory.clone();
            tile.startup_command = existing.startup_command.clone();
            tile.connection_target = existing.connection_target.clone();
            tile.pane_groups = existing.pane_groups.clone();
            tile.reconnect_policy = existing.reconnect_policy;
            tile.applied_role_id = existing.applied_role_id.clone();
            tile.output_helpers = existing.output_helpers.clone();
        }
    }

    next_layout.with_tile_specs(&next_tiles)
}

pub fn apply_role_to_tile(tile: &mut TileSpec, role: &AgentRoleTemplate) {
    tile.applied_role_id = Some(role.id.clone());
    tile.accent_class = role.accent_class.clone();
    if let Some(title) = role.default_title.as_deref() {
        tile.title = title.to_string();
    }
    if let Some(agent_label) = role.default_agent_label.as_deref() {
        tile.agent_label = agent_label.to_string();
    }
    if let Some(command) = role.default_startup_command.as_deref() {
        tile.startup_command = Some(command.to_string());
    }
    tile.output_helpers = role.default_output_helpers.clone();
    tile.reconnect_policy = role.default_reconnect_policy;
    if let Some(profile_id) = role.default_connection_profile_id.as_deref() {
        tile.connection_target =
            crate::model::assets::TileConnectionTarget::Profile(profile_id.to_string());
    }
    if !role.default_pane_groups.is_empty() {
        tile.pane_groups = role.default_pane_groups.clone();
    }
}

pub fn resolve_role<'a>(
    assets: &'a WorkspaceAssets,
    role_id: Option<&str>,
) -> Option<&'a AgentRoleTemplate> {
    let role_id = role_id?;
    assets.role_templates.iter().find(|role| role.id == role_id)
}

pub fn apply_project_suggestion(
    current_layout: &LayoutNode,
    suggestion: &ProjectSuggestion,
    assets: &WorkspaceAssets,
) -> LayoutNode {
    let mut layout = resize_layout(current_layout, suggestion.tile_count);
    let mut specs = layout.tile_specs();
    for (index, tile) in specs.iter_mut().enumerate() {
        if let Some(role_id) = suggestion.role_ids.get(index)
            && let Some(role) = resolve_role(assets, Some(role_id.as_str()))
        {
            apply_role_to_tile(tile, role);
        }
        if let Some(command) = suggestion.startup_commands.get(index).cloned().flatten() {
            tile.startup_command = Some(command);
        }
    }
    layout = layout.with_tile_specs(&specs);
    layout
}

#[cfg(test)]
mod tests {
    use super::{apply_project_suggestion, apply_role_to_tile, resize_layout};
    use crate::model::assets::{AgentRoleTemplate, ProjectSuggestion, WorkspaceAssets};
    use crate::model::layout::{ReconnectPolicy, TileSpec, WorkingDirectory, default_tile_spec};

    fn role() -> AgentRoleTemplate {
        AgentRoleTemplate {
            id: "ops".into(),
            name: "Ops".into(),
            description: String::new(),
            accent_class: "accent-rose".into(),
            default_title: Some("Ops".into()),
            default_agent_label: Some("Operator".into()),
            default_startup_command: Some("htop".into()),
            default_connection_profile_id: Some("prod".into()),
            default_pane_groups: vec!["ops".into()],
            default_reconnect_policy: ReconnectPolicy::Always,
            default_output_helpers: Vec::new(),
        }
    }

    fn tile_with_reconnect() -> TileSpec {
        let mut tile = default_tile_spec(1);
        tile.reconnect_policy = ReconnectPolicy::OnAbnormalExit;
        tile
    }

    #[test]
    fn resize_layout_preserves_reconnect_policy() {
        let layout = crate::model::layout::LayoutNode::Tile(tile_with_reconnect());
        let resized = resize_layout(&layout, 2);
        let tiles = resized.tile_specs();
        assert_eq!(tiles[0].reconnect_policy, ReconnectPolicy::OnAbnormalExit);
    }

    #[test]
    fn apply_role_copies_reconnect_policy() {
        let mut tile = TileSpec {
            id: "tile-1".into(),
            title: "Tile 1".into(),
            agent_label: "Shell".into(),
            accent_class: "accent-cyan".into(),
            working_directory: WorkingDirectory::WorkspaceRoot,
            startup_command: None,
            connection_target: Default::default(),
            pane_groups: Vec::new(),
            reconnect_policy: ReconnectPolicy::Manual,
            applied_role_id: None,
            output_helpers: Vec::new(),
        };

        apply_role_to_tile(&mut tile, &role());

        assert_eq!(tile.reconnect_policy, ReconnectPolicy::Always);
    }

    #[test]
    fn suggestion_application_uses_role_defaults() {
        let suggestion = ProjectSuggestion {
            id: "ops".into(),
            title: "Ops".into(),
            description: String::new(),
            role_ids: vec!["ops".into()],
            tile_count: 1,
            startup_commands: vec![None],
            tags: Vec::new(),
        };
        let assets = WorkspaceAssets {
            role_templates: vec![role()],
            ..WorkspaceAssets::default()
        };
        let layout = crate::model::layout::LayoutNode::Tile(default_tile_spec(1));
        let applied = apply_project_suggestion(&layout, &suggestion, &assets);
        let tiles = applied.tile_specs();
        assert_eq!(tiles[0].applied_role_id.as_deref(), Some("ops"));
        assert_eq!(tiles[0].reconnect_policy, ReconnectPolicy::Always);
    }
}
