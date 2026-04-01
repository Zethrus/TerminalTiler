use std::collections::BTreeSet;

use crate::model::layout::TileSpec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BroadcastTarget {
    Off,
    AllPanes,
    SavedGroup(String),
    AdHocSelection(BTreeSet<String>),
}

impl Default for BroadcastTarget {
    fn default() -> Self {
        Self::Off
    }
}

impl BroadcastTarget {
    pub fn label(&self) -> String {
        match self {
            Self::Off => "Broadcast Off".into(),
            Self::AllPanes => "Broadcast All Panes".into(),
            Self::SavedGroup(group) => format!("Group: {group}"),
            Self::AdHocSelection(ids) => format!("Ad Hoc ({})", ids.len()),
        }
    }

    pub fn includes(&self, tile: &TileSpec) -> bool {
        match self {
            Self::Off => false,
            Self::AllPanes => true,
            Self::SavedGroup(group) => tile.pane_groups.iter().any(|item| item == group),
            Self::AdHocSelection(ids) => ids.contains(&tile.id),
        }
    }
}

pub fn saved_groups_for_tiles(tiles: &[TileSpec]) -> Vec<String> {
    let mut groups = BTreeSet::new();
    for tile in tiles {
        for group in &tile.pane_groups {
            let group = group.trim();
            if !group.is_empty() {
                groups.insert(group.to_string());
            }
        }
    }
    groups.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{BroadcastTarget, saved_groups_for_tiles};
    use crate::model::layout::{ReconnectPolicy, TileSpec, WorkingDirectory};

    fn tile(id: &str, groups: &[&str]) -> TileSpec {
        TileSpec {
            id: id.into(),
            title: id.into(),
            agent_label: "Shell".into(),
            accent_class: "accent-cyan".into(),
            working_directory: WorkingDirectory::WorkspaceRoot,
            startup_command: None,
            connection_target: Default::default(),
            pane_groups: groups.iter().map(|value| value.to_string()).collect(),
            reconnect_policy: ReconnectPolicy::Manual,
            applied_role_id: None,
            output_helpers: Vec::new(),
        }
    }

    #[test]
    fn saved_groups_are_unique_and_sorted() {
        let groups = saved_groups_for_tiles(&[
            tile("one", &["ops", "prod"]),
            tile("two", &["ops", "review"]),
        ]);
        assert_eq!(groups, vec!["ops", "prod", "review"]);
    }

    #[test]
    fn broadcast_target_matches_expected_tiles() {
        let tile = tile("one", &["ops"]);
        assert!(BroadcastTarget::AllPanes.includes(&tile));
        assert!(BroadcastTarget::SavedGroup("ops".into()).includes(&tile));
        assert!(!BroadcastTarget::SavedGroup("review".into()).includes(&tile));
        assert!(BroadcastTarget::AdHocSelection(BTreeSet::from(["one".into()])).includes(&tile));
    }
}
