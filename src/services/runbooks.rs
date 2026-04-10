use std::collections::{BTreeSet, HashMap};

use regex::Regex;

use crate::model::assets::{Runbook, RunbookConfirmPolicy, RunbookTarget};
use crate::model::layout::TileSpec;
use crate::services::broadcast::BroadcastTarget;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedRunbook {
    pub target: BroadcastTarget,
    pub target_label: String,
    pub matching_tile_ids: BTreeSet<String>,
    pub commands: Vec<String>,
    pub confirmation_required: bool,
}

pub fn resolve_runbook(
    runbook: &Runbook,
    variables: &HashMap<String, String>,
    tiles: &[TileSpec],
) -> Result<ResolvedRunbook, String> {
    let matching_tile_ids: BTreeSet<String> = match &runbook.target {
        RunbookTarget::AllPanes => tiles.iter().map(|tile| tile.id.clone()).collect(),
        RunbookTarget::PaneGroup(group) => tiles
            .iter()
            .filter(|tile| {
                tile.pane_groups
                    .iter()
                    .any(|pane_group| pane_group == group)
            })
            .map(|tile| tile.id.clone())
            .collect(),
        RunbookTarget::Role(role_id) => tiles
            .iter()
            .filter(|tile| tile.applied_role_id.as_deref() == Some(role_id.as_str()))
            .map(|tile| tile.id.clone())
            .collect(),
        RunbookTarget::ConnectionProfile(profile_id) => tiles
            .iter()
            .filter(|tile| {
                matches!(
                    &tile.connection_target,
                    crate::model::assets::TileConnectionTarget::Profile(tile_profile_id)
                        if tile_profile_id == profile_id
                )
            })
            .map(|tile| tile.id.clone())
            .collect(),
    };

    if matching_tile_ids.is_empty() {
        return Err(format!(
            "Runbook '{}' does not match any panes for {}.",
            runbook.name,
            runbook.target.label()
        ));
    }

    let commands = runbook
        .steps
        .iter()
        .map(|step| {
            let rendered = render_variables(&step.command, variables)?;
            Ok(if step.append_newline && !rendered.ends_with('\n') {
                format!("{rendered}\n")
            } else {
                rendered
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let target = match &runbook.target {
        RunbookTarget::AllPanes => BroadcastTarget::AllPanes,
        _ => BroadcastTarget::AdHocSelection(matching_tile_ids.clone()),
    };
    let confirmation_required = match runbook.confirm_policy {
        RunbookConfirmPolicy::Always => true,
        RunbookConfirmPolicy::Never => false,
        RunbookConfirmPolicy::MultiPaneOrRemote => {
            matching_tile_ids.len() > 1
                || tiles.iter().any(|tile| {
                    matching_tile_ids.contains(&tile.id)
                        && !matches!(
                            tile.connection_target,
                            crate::model::assets::TileConnectionTarget::Local
                        )
                })
        }
    };

    Ok(ResolvedRunbook {
        target,
        target_label: runbook.target.label(),
        matching_tile_ids,
        commands,
        confirmation_required,
    })
}

fn render_variables(command: &str, variables: &HashMap<String, String>) -> Result<String, String> {
    let variable_pattern =
        Regex::new(r"\{\{\s*([a-zA-Z0-9_-]+)\s*\}\}").map_err(|error| error.to_string())?;
    let mut rendered = String::new();
    let mut last_end = 0;
    for captures in variable_pattern.captures_iter(command) {
        let Some(variable_match) = captures.get(0) else {
            continue;
        };
        let Some(key_match) = captures.get(1) else {
            continue;
        };
        rendered.push_str(&command[last_end..variable_match.start()]);
        let key = key_match.as_str();
        let value = variables
            .get(key)
            .ok_or_else(|| format!("Missing runbook variable '{key}'."))?;
        rendered.push_str(value);
        last_end = variable_match.end();
    }
    rendered.push_str(&command[last_end..]);
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::resolve_runbook;
    use crate::model::assets::{Runbook, RunbookConfirmPolicy, RunbookStep, RunbookTarget};
    use crate::model::layout::{ReconnectPolicy, TileSpec, WorkingDirectory};

    fn tile(id: &str) -> TileSpec {
        TileSpec {
            id: id.into(),
            title: id.into(),
            agent_label: "Ops".into(),
            accent_class: "accent-cyan".into(),
            working_directory: WorkingDirectory::WorkspaceRoot,
            startup_command: None,
            connection_target: crate::model::assets::TileConnectionTarget::Local,
            pane_groups: vec!["ops".into()],
            reconnect_policy: ReconnectPolicy::Manual,
            applied_role_id: Some("ops".into()),
            output_helpers: Vec::new(),
            tile_kind: Default::default(),
            url: None,
            auto_refresh_seconds: None,
        }
    }

    #[test]
    fn resolves_runbook_commands_and_targets() {
        let runbook = Runbook {
            id: "restart".into(),
            name: "Restart".into(),
            description: String::new(),
            tags: Vec::new(),
            target: RunbookTarget::PaneGroup("ops".into()),
            variables: Vec::new(),
            steps: vec![RunbookStep {
                id: "step-1".into(),
                label: "Restart".into(),
                command: "sudo systemctl restart {{service}}".into(),
                append_newline: true,
            }],
            confirm_policy: RunbookConfirmPolicy::Always,
        };
        let variables = HashMap::from([(String::from("service"), String::from("nginx"))]);

        let resolved = resolve_runbook(&runbook, &variables, &[tile("pane-1")]).unwrap();

        assert_eq!(resolved.matching_tile_ids.len(), 1);
        assert!(resolved.confirmation_required);
        assert_eq!(
            resolved.commands,
            vec![String::from("sudo systemctl restart nginx\n")]
        );
    }
}
