use std::collections::BTreeSet;

use crate::model::layout::TileSpec;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum BroadcastTarget {
    #[default]
    Off,
    AllPanes,
    SavedGroup(String),
    AdHocSelection(BTreeSet<String>),
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

#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell"),
    test
))]
pub fn target_from_selector_id(selector_id: Option<&str>) -> BroadcastTarget {
    match selector_id {
        Some("all") => BroadcastTarget::AllPanes,
        Some(value) if value.starts_with("group:") => {
            BroadcastTarget::SavedGroup(value.trim_start_matches("group:").to_string())
        }
        _ => BroadcastTarget::Off,
    }
}

#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell"),
    test
))]
pub fn quick_send_payload(command: &str) -> Option<String> {
    let command = command.trim();
    if command.is_empty() {
        None
    } else {
        Some(format!("{command}\n"))
    }
}

#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell"),
    test
))]
pub fn sent_status_label(target_label: &str, sent: usize) -> String {
    format!("{target_label}  •  sent to {sent}")
}

#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell"),
    test
))]
pub fn quick_send_detail(sent: usize) -> String {
    format!("Sent quick command to {sent} pane(s).")
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

    use super::{
        BroadcastTarget, quick_send_detail, quick_send_payload, saved_groups_for_tiles,
        sent_status_label, target_from_selector_id,
    };
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
            tile_kind: Default::default(),
            url: None,
            auto_refresh_seconds: None,
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

    #[test]
    fn broadcast_selector_ids_resolve_to_targets() {
        assert_eq!(target_from_selector_id(Some("off")), BroadcastTarget::Off);
        assert_eq!(
            target_from_selector_id(Some("all")),
            BroadcastTarget::AllPanes
        );
        assert_eq!(
            target_from_selector_id(Some("group:ops")),
            BroadcastTarget::SavedGroup("ops".into())
        );
        assert_eq!(
            target_from_selector_id(Some("unknown")),
            BroadcastTarget::Off
        );
        assert_eq!(target_from_selector_id(None), BroadcastTarget::Off);
    }

    #[test]
    fn quick_send_payload_matches_workspace_submit_contract() {
        assert_eq!(quick_send_payload(""), None);
        assert_eq!(quick_send_payload("   "), None);
        assert_eq!(quick_send_payload("ls"), Some("ls\n".into()));
        assert_eq!(
            quick_send_payload("  cargo test  "),
            Some("cargo test\n".into())
        );
    }

    #[test]
    fn broadcast_status_copy_stays_shared() {
        assert_eq!(
            sent_status_label("Broadcast All Panes", 2),
            "Broadcast All Panes  •  sent to 2"
        );
        assert_eq!(quick_send_detail(1), "Sent quick command to 1 pane(s).");
    }
}
