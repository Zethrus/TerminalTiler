use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::platform::home_dir;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "type", content = "value")]
pub enum WorkingDirectory {
    Home,
    WorkspaceRoot,
    Relative(String),
    Absolute(PathBuf),
}

impl WorkingDirectory {
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub fn resolve(&self, workspace_root: &Path) -> PathBuf {
        match self {
            Self::Home => home_dir().unwrap_or_else(|| workspace_root.to_path_buf()),
            Self::WorkspaceRoot => workspace_root.to_path_buf(),
            Self::Relative(path) => workspace_root.join(path),
            Self::Absolute(path) => path.clone(),
        }
    }

    pub fn short_label(&self) -> String {
        match self {
            Self::Home => "Home".into(),
            Self::WorkspaceRoot => "Workspace root".into(),
            Self::Relative(path) => path.clone(),
            Self::Absolute(path) => path.display().to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TileSpec {
    pub id: String,
    pub title: String,
    pub agent_label: String,
    pub accent_class: String,
    pub working_directory: WorkingDirectory,
    pub startup_command: Option<String>,
}

impl TileSpec {
    #[allow(dead_code)]
    pub fn startup_label(&self) -> &str {
        self.startup_command.as_deref().unwrap_or("Shell")
    }

    #[allow(dead_code)]
    pub fn summary_line(&self) -> String {
        format!(
            "{}  •  {}  •  {}  •  {}",
            self.title,
            self.agent_label,
            self.working_directory.short_label(),
            self.startup_label()
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum LayoutNode {
    Split {
        axis: SplitAxis,
        ratio: f32,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
    Tile(TileSpec),
}

impl LayoutNode {
    pub fn tile_count(&self) -> usize {
        match self {
            Self::Split { first, second, .. } => first.tile_count() + second.tile_count(),
            Self::Tile(_) => 1,
        }
    }

    pub fn tile_specs(&self) -> Vec<TileSpec> {
        let mut tiles = Vec::new();
        self.collect_tile_specs(&mut tiles);
        tiles
    }

    pub fn with_tile_specs(&self, tile_specs: &[TileSpec]) -> Self {
        let mut tiles = tile_specs.iter().cloned();
        let layout = self.replace_tile_specs(&mut tiles);
        debug_assert!(
            tiles.next().is_none(),
            "unused tile specs after layout rebuild"
        );
        layout
    }

    pub fn swap_tile_positions(&self, dragged_id: &str, target_id: &str) -> Option<Self> {
        if dragged_id == target_id {
            return None;
        }

        let mut specs = self.tile_specs();
        let dragged_index = specs.iter().position(|tile| tile.id == dragged_id)?;
        let target_index = specs.iter().position(|tile| tile.id == target_id)?;
        specs.swap(dragged_index, target_index);
        Some(self.with_tile_specs(&specs))
    }

    #[allow(dead_code)]
    pub fn tile_summaries(&self) -> Vec<String> {
        let mut summaries = Vec::new();
        self.collect_tile_summaries(&mut summaries);
        summaries
    }

    fn collect_tile_summaries(&self, summaries: &mut Vec<String>) {
        match self {
            Self::Split { first, second, .. } => {
                first.collect_tile_summaries(summaries);
                second.collect_tile_summaries(summaries);
            }
            Self::Tile(tile) => summaries.push(tile.summary_line()),
        }
    }

    fn collect_tile_specs(&self, tiles: &mut Vec<TileSpec>) {
        match self {
            Self::Split { first, second, .. } => {
                first.collect_tile_specs(tiles);
                second.collect_tile_specs(tiles);
            }
            Self::Tile(tile) => tiles.push(tile.clone()),
        }
    }

    fn replace_tile_specs<I>(&self, tile_specs: &mut I) -> Self
    where
        I: Iterator<Item = TileSpec>,
    {
        match self {
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => Self::Split {
                axis: *axis,
                ratio: *ratio,
                first: Box::new(first.replace_tile_specs(tile_specs)),
                second: Box::new(second.replace_tile_specs(tile_specs)),
            },
            Self::Tile(tile) => Self::Tile(tile_specs.next().unwrap_or_else(|| tile.clone())),
        }
    }
}

pub fn split(axis: SplitAxis, ratio: f32, first: LayoutNode, second: LayoutNode) -> LayoutNode {
    LayoutNode::Split {
        axis,
        ratio,
        first: Box::new(first),
        second: Box::new(second),
    }
}

pub fn tile(
    id: &str,
    title: &str,
    agent_label: &str,
    accent_class: &str,
    working_directory: WorkingDirectory,
    startup_command: Option<&str>,
) -> LayoutNode {
    LayoutNode::Tile(TileSpec {
        id: id.into(),
        title: title.into(),
        agent_label: agent_label.into(),
        accent_class: accent_class.into(),
        working_directory,
        startup_command: startup_command.map(str::to_owned),
    })
}

pub fn default_tile_spec(index: usize) -> TileSpec {
    TileSpec {
        id: format!("tile-{}", index),
        title: format!("Tile {}", index),
        agent_label: "Shell".into(),
        accent_class: "accent-cyan".into(),
        working_directory: WorkingDirectory::WorkspaceRoot,
        startup_command: None,
    }
}

pub fn generate_layout(tile_count: usize) -> LayoutNode {
    match tile_count {
        1 => LayoutNode::Tile(default_tile_spec(1)),
        2 => combine_evenly(
            SplitAxis::Horizontal,
            vec![
                LayoutNode::Tile(default_tile_spec(1)),
                LayoutNode::Tile(default_tile_spec(2)),
            ],
        ),
        4 => generate_grid_layout(2, 2, 1),
        6 => generate_grid_layout(3, 2, 1),
        8 => generate_grid_layout(4, 2, 1),
        10 => generate_grid_layout(5, 2, 1),
        12 => generate_grid_layout(4, 3, 1),
        16 => generate_grid_layout(4, 4, 1),
        _ => generate_layout_inner(tile_count, 0, 1),
    }
}

fn generate_layout_inner(count: usize, depth: usize, start_index: usize) -> LayoutNode {
    if count <= 1 {
        return LayoutNode::Tile(default_tile_spec(start_index));
    }

    let left_count = count / 2;
    let right_count = count - left_count;
    let axis = if depth.is_multiple_of(2) {
        SplitAxis::Horizontal
    } else {
        SplitAxis::Vertical
    };

    LayoutNode::Split {
        axis,
        ratio: left_count as f32 / count as f32,
        first: Box::new(generate_layout_inner(left_count, depth + 1, start_index)),
        second: Box::new(generate_layout_inner(
            right_count,
            depth + 1,
            start_index + left_count,
        )),
    }
}

fn generate_grid_layout(columns: usize, rows: usize, start_index: usize) -> LayoutNode {
    let mut row_nodes = Vec::with_capacity(rows);

    for row in 0..rows {
        let row_start = start_index + (row * columns);
        let tiles = (0..columns)
            .map(|offset| LayoutNode::Tile(default_tile_spec(row_start + offset)))
            .collect::<Vec<_>>();
        row_nodes.push(combine_evenly(SplitAxis::Horizontal, tiles));
    }

    combine_evenly(SplitAxis::Vertical, row_nodes)
}

fn combine_evenly(axis: SplitAxis, mut nodes: Vec<LayoutNode>) -> LayoutNode {
    assert!(
        !nodes.is_empty(),
        "combine_evenly requires at least one node"
    );

    if nodes.len() == 1 {
        return nodes.pop().expect("single node should exist");
    }

    let remaining = nodes.len();
    let first = nodes.remove(0);
    split(
        axis,
        1.0 / remaining as f32,
        first,
        combine_evenly(axis, nodes),
    )
}

pub struct LayoutTemplate {
    pub tile_count: usize,
    pub label: &'static str,
    pub subtitle: &'static str,
}

pub fn builtin_templates() -> Vec<LayoutTemplate> {
    vec![
        LayoutTemplate {
            tile_count: 1,
            label: "Single",
            subtitle: "One terminal (default)",
        },
        LayoutTemplate {
            tile_count: 2,
            label: "2 Sessions",
            subtitle: "Side by side",
        },
        LayoutTemplate {
            tile_count: 4,
            label: "4 Sessions",
            subtitle: "2\u{00d7}2 grid",
        },
        LayoutTemplate {
            tile_count: 6,
            label: "6 Sessions",
            subtitle: "3\u{00d7}2 grid",
        },
        LayoutTemplate {
            tile_count: 8,
            label: "8 Sessions",
            subtitle: "4\u{00d7}2 grid",
        },
        LayoutTemplate {
            tile_count: 10,
            label: "10 Sessions",
            subtitle: "5\u{00d7}2 grid",
        },
        LayoutTemplate {
            tile_count: 12,
            label: "12 Sessions",
            subtitle: "4\u{00d7}3 grid",
        },
        LayoutTemplate {
            tile_count: 14,
            label: "14 Sessions",
            subtitle: "Balanced split",
        },
        LayoutTemplate {
            tile_count: 16,
            label: "16 Sessions",
            subtitle: "4\u{00d7}4 grid",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::{LayoutNode, SplitAxis, WorkingDirectory, split, tile};

    #[test]
    fn collects_tile_specs_in_display_order() {
        let layout = split(
            SplitAxis::Horizontal,
            0.5,
            tile(
                "left",
                "Left",
                "Codex",
                "accent-cyan",
                WorkingDirectory::WorkspaceRoot,
                Some("codex"),
            ),
            split(
                SplitAxis::Vertical,
                0.5,
                tile(
                    "top-right",
                    "Top Right",
                    "Claude",
                    "accent-amber",
                    WorkingDirectory::WorkspaceRoot,
                    Some("claude"),
                ),
                tile(
                    "bottom-right",
                    "Bottom Right",
                    "Shell",
                    "accent-rose",
                    WorkingDirectory::WorkspaceRoot,
                    None,
                ),
            ),
        );

        let specs = layout.tile_specs();

        assert_eq!(specs.len(), 3);
        assert_eq!(specs[0].id, "left");
        assert_eq!(specs[1].id, "top-right");
        assert_eq!(specs[2].id, "bottom-right");
    }

    #[test]
    fn replacing_tile_specs_keeps_layout_shape() {
        let layout = split(
            SplitAxis::Horizontal,
            0.5,
            tile(
                "left",
                "Left",
                "Codex",
                "accent-cyan",
                WorkingDirectory::WorkspaceRoot,
                Some("codex"),
            ),
            tile(
                "right",
                "Right",
                "Shell",
                "accent-rose",
                WorkingDirectory::WorkspaceRoot,
                None,
            ),
        );

        let mut specs = layout.tile_specs();
        specs[0].agent_label = "Claude".into();
        specs[0].startup_command = Some("claude".into());
        specs[1].title = "Verifier".into();

        let updated = layout.with_tile_specs(&specs);

        match updated {
            LayoutNode::Split { first, second, .. } => {
                match *first {
                    LayoutNode::Tile(tile) => {
                        assert_eq!(tile.agent_label, "Claude");
                        assert_eq!(tile.startup_command.as_deref(), Some("claude"));
                    }
                    _ => panic!("expected first node to remain a tile"),
                }

                match *second {
                    LayoutNode::Tile(tile) => {
                        assert_eq!(tile.title, "Verifier");
                    }
                    _ => panic!("expected second node to remain a tile"),
                }
            }
            _ => panic!("expected split layout to remain a split"),
        }
    }

    #[test]
    fn swapping_tile_positions_keeps_layout_shape() {
        let layout = split(
            SplitAxis::Horizontal,
            0.5,
            tile(
                "left",
                "Left",
                "Codex",
                "accent-cyan",
                WorkingDirectory::WorkspaceRoot,
                Some("codex"),
            ),
            split(
                SplitAxis::Vertical,
                0.5,
                tile(
                    "top-right",
                    "Top Right",
                    "Claude",
                    "accent-amber",
                    WorkingDirectory::WorkspaceRoot,
                    Some("claude"),
                ),
                tile(
                    "bottom-right",
                    "Bottom Right",
                    "Shell",
                    "accent-rose",
                    WorkingDirectory::WorkspaceRoot,
                    None,
                ),
            ),
        );

        let swapped = layout
            .swap_tile_positions("left", "bottom-right")
            .expect("swap should succeed");
        let specs = swapped.tile_specs();

        assert_eq!(specs[0].id, "bottom-right");
        assert_eq!(specs[1].id, "top-right");
        assert_eq!(specs[2].id, "left");

        match swapped {
            LayoutNode::Split { first, second, .. } => {
                match *first {
                    LayoutNode::Tile(tile) => assert_eq!(tile.id, "bottom-right"),
                    _ => panic!("expected first node to remain a tile"),
                }

                match *second {
                    LayoutNode::Split { first, second, .. } => {
                        match *first {
                            LayoutNode::Tile(tile) => assert_eq!(tile.id, "top-right"),
                            _ => panic!("expected top-right node to remain a tile"),
                        }
                        match *second {
                            LayoutNode::Tile(tile) => assert_eq!(tile.id, "left"),
                            _ => panic!("expected bottom-right node to remain a tile"),
                        }
                    }
                    _ => panic!("expected right side to remain split"),
                }
            }
            _ => panic!("expected split layout to remain a split"),
        }
    }

    #[test]
    fn swapping_unknown_or_identical_tile_is_noop() {
        let layout = split(
            SplitAxis::Horizontal,
            0.5,
            tile(
                "left",
                "Left",
                "Codex",
                "accent-cyan",
                WorkingDirectory::WorkspaceRoot,
                Some("codex"),
            ),
            tile(
                "right",
                "Right",
                "Shell",
                "accent-rose",
                WorkingDirectory::WorkspaceRoot,
                None,
            ),
        );

        assert!(layout.swap_tile_positions("left", "left").is_none());
        assert!(layout.swap_tile_positions("left", "missing").is_none());
    }
}
