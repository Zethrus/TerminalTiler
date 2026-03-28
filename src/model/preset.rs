use serde::{Deserialize, Serialize};

use crate::model::layout::{LayoutNode, SplitAxis, WorkingDirectory, split, tile};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowChrome {
    Standard,
    Compact,
}

impl ThemeMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

impl WindowChrome {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Compact => "Compact",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspacePreset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub root_label: String,
    pub theme: ThemeMode,
    pub chrome: WindowChrome,
    pub layout: LayoutNode,
}

impl WorkspacePreset {
    pub fn tile_count(&self) -> usize {
        self.layout.tile_count()
    }

    pub fn template_badge(&self) -> String {
        format!("{} tiles", self.tile_count())
    }

    #[allow(dead_code)]
    pub fn tile_summaries(&self) -> Vec<String> {
        self.layout.tile_summaries()
    }
}

const BUILTIN_IDS: &[&str] = &["solo-operator", "review-pair", "delivery-fleet"];

pub fn is_builtin_preset_id(id: &str) -> bool {
    BUILTIN_IDS.contains(&id)
}

pub fn builtin_presets() -> Vec<WorkspacePreset> {
    vec![solo_operator(), review_pair(), delivery_fleet()]
}

fn solo_operator() -> WorkspacePreset {
    WorkspacePreset {
        id: "solo-operator".into(),
        name: "Solo Operator".into(),
        description: "A single primary terminal with a clean command deck for focused execution."
            .into(),
        tags: vec!["focused".into(), "codex".into()],
        root_label: "Workspace root".into(),
        theme: ThemeMode::Dark,
        chrome: WindowChrome::Compact,
        layout: tile(
            "primary",
            "Primary Agent",
            "Codex",
            "accent-amber",
            WorkingDirectory::WorkspaceRoot,
            Some("codex"),
        ),
    }
}

fn review_pair() -> WorkspacePreset {
    WorkspacePreset {
        id: "review-pair".into(),
        name: "Review Pair".into(),
        description: "Implementation on the left, verification on the right, with split ownership by directory.".into(),
        tags: vec!["pairing".into(), "review".into()],
        root_label: "Project directory".into(),
        theme: ThemeMode::System,
        chrome: WindowChrome::Standard,
        layout: split(
            SplitAxis::Horizontal,
            0.56,
            tile(
                "builder",
                "Builder",
                "Codex Build",
                "accent-cyan",
                WorkingDirectory::Relative(".".into()),
                Some("codex"),
            ),
            tile(
                "reviewer",
                "Reviewer",
                "QA Watch",
                "accent-rose",
                WorkingDirectory::Relative(".".into()),
                Some("bash"),
            ),
        ),
    }
}

fn delivery_fleet() -> WorkspacePreset {
    WorkspacePreset {
        id: "delivery-fleet".into(),
        name: "Delivery Fleet".into(),
        description: "Planner, implementation, and release terminals arranged for multi-step project execution.".into(),
        tags: vec!["fleet".into(), "release".into(), "premium".into()],
        root_label: "Delivery workspace".into(),
        theme: ThemeMode::Dark,
        chrome: WindowChrome::Compact,
        layout: split(
            SplitAxis::Horizontal,
            0.36,
            tile(
                "planner",
                "Planner",
                "Route Control",
                "accent-violet",
                WorkingDirectory::WorkspaceRoot,
                Some("codex"),
            ),
            split(
                SplitAxis::Vertical,
                0.54,
                tile(
                    "implementer",
                    "Implementer",
                    "Execution Bay",
                    "accent-cyan",
                    WorkingDirectory::Relative("src".into()),
                    Some("bash"),
                ),
                tile(
                    "shipper",
                    "Release",
                    "Launch Control",
                    "accent-amber",
                    WorkingDirectory::Relative(".".into()),
                    Some("bash"),
                ),
            ),
        ),
    }
}
