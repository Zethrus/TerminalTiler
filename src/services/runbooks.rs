use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

use crate::model::assets::{Runbook, RunbookConfirmPolicy, RunbookTarget, TemplateVariableValues};
use crate::model::layout::TileSpec;
use crate::services::broadcast::BroadcastTarget;
use crate::services::template_variables::{
    TemplateRenderError, TemplateVariableContext, render_variables,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedRunbook {
    pub target: BroadcastTarget,
    pub target_label: String,
    pub matching_tile_ids: BTreeSet<String>,
    pub commands: Vec<String>,
    pub confirmation_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunbookResolveError {
    NoMatchingPanes {
        runbook_name: String,
        target_label: String,
    },
    Template(TemplateRenderError),
}

impl fmt::Display for RunbookResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoMatchingPanes {
                runbook_name,
                target_label,
            } => write!(
                formatter,
                "Runbook '{}' does not match any panes for {}.",
                runbook_name, target_label
            ),
            Self::Template(error) => error.fmt(formatter),
        }
    }
}

impl Error for RunbookResolveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::NoMatchingPanes { .. } => None,
            Self::Template(error) => Some(error),
        }
    }
}

impl From<TemplateRenderError> for RunbookResolveError {
    fn from(error: TemplateRenderError) -> Self {
        Self::Template(error)
    }
}

pub fn resolve_runbook(
    runbook: &Runbook,
    variables: &TemplateVariableValues,
    tiles: &[TileSpec],
) -> Result<ResolvedRunbook, RunbookResolveError> {
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
        return Err(RunbookResolveError::NoMatchingPanes {
            runbook_name: runbook.name.clone(),
            target_label: runbook.target.label(),
        });
    }

    let commands = runbook
        .steps
        .iter()
        .map(|step| {
            let rendered =
                render_variables(&step.command, variables, TemplateVariableContext::Runbook)?;
            Ok(if step.append_newline && !rendered.ends_with('\n') {
                format!("{rendered}\n")
            } else {
                rendered
            })
        })
        .collect::<Result<Vec<_>, RunbookResolveError>>()?;

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
#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{RunbookResolveError, resolve_runbook};
    use crate::model::assets::{Runbook, RunbookConfirmPolicy, RunbookStep, RunbookTarget};
    use crate::model::layout::{ReconnectPolicy, TileSpec, WorkingDirectory};
    use crate::services::template_variables::{TemplateRenderError, TemplateVariableContext};

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

    #[test]
    fn reports_missing_variables_with_a_typed_error() {
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

        let error = resolve_runbook(&runbook, &HashMap::new(), &[tile("pane-1")])
            .expect_err("missing variable should fail");

        assert_eq!(
            error,
            RunbookResolveError::Template(TemplateRenderError::MissingVariable {
                context: TemplateVariableContext::Runbook,
                key: "service".into(),
            })
        );
    }
}
