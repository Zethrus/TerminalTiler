use uuid::Uuid;

use crate::model::layout::{LayoutNode, SplitAxis, TileSpec};

pub fn split_tile(
    layout: &LayoutNode,
    target_tile_id: &str,
    axis: SplitAxis,
    clone_existing: bool,
) -> Option<LayoutNode> {
    mutate_layout(layout, target_tile_id, &mut |tile| {
        let new_tile = if clone_existing {
            let mut cloned = tile.clone();
            cloned.id = format!("tile-{}", Uuid::new_v4().simple());
            cloned.title = format!("{} Copy", cloned.title);
            cloned
        } else {
            let mut created = tile.clone();
            created.id = format!("tile-{}", Uuid::new_v4().simple());
            created.title = "New Tile".into();
            created.agent_label = "Shell".into();
            created.startup_command = None;
            created.applied_role_id = None;
            created.pane_groups.clear();
            created.output_helpers.clear();
            created
        };
        LayoutNode::Split {
            axis,
            ratio: 0.5,
            first: Box::new(LayoutNode::Tile(tile.clone())),
            second: Box::new(LayoutNode::Tile(new_tile)),
        }
    })
}

pub fn close_tile(layout: &LayoutNode, target_tile_id: &str) -> Option<LayoutNode> {
    if layout.tile_count() <= 1 {
        return None;
    }
    remove_tile(layout, target_tile_id)
}

pub fn update_split_ratio(
    layout: &LayoutNode,
    split_path: &[bool],
    ratio: f32,
) -> Option<LayoutNode> {
    update_ratio_inner(layout, split_path, ratio.clamp(0.1, 0.9))
}

fn mutate_layout(
    layout: &LayoutNode,
    target_tile_id: &str,
    transform: &mut dyn FnMut(&TileSpec) -> LayoutNode,
) -> Option<LayoutNode> {
    match layout {
        LayoutNode::Tile(tile) if tile.id == target_tile_id => Some(transform(tile)),
        LayoutNode::Tile(_) => None,
        LayoutNode::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            if let Some(next_first) = mutate_layout(first, target_tile_id, transform) {
                return Some(LayoutNode::Split {
                    axis: *axis,
                    ratio: *ratio,
                    first: Box::new(next_first),
                    second: second.clone(),
                });
            }
            mutate_layout(second, target_tile_id, transform).map(|next_second| LayoutNode::Split {
                axis: *axis,
                ratio: *ratio,
                first: first.clone(),
                second: Box::new(next_second),
            })
        }
    }
}

fn remove_tile(layout: &LayoutNode, target_tile_id: &str) -> Option<LayoutNode> {
    match layout {
        LayoutNode::Tile(tile) => {
            if tile.id == target_tile_id {
                None
            } else {
                Some(layout.clone())
            }
        }
        LayoutNode::Split {
            axis,
            ratio,
            first,
            second,
        } => match (
            remove_tile(first, target_tile_id),
            remove_tile(second, target_tile_id),
        ) {
            (Some(next_first), Some(next_second)) => Some(LayoutNode::Split {
                axis: *axis,
                ratio: *ratio,
                first: Box::new(next_first),
                second: Box::new(next_second),
            }),
            (Some(next_first), None) => Some(next_first),
            (None, Some(next_second)) => Some(next_second),
            (None, None) => None,
        },
    }
}

fn update_ratio_inner(layout: &LayoutNode, split_path: &[bool], ratio: f32) -> Option<LayoutNode> {
    match layout {
        LayoutNode::Tile(_) => None,
        LayoutNode::Split {
            axis,
            ratio: current_ratio,
            first,
            second,
        } => {
            if split_path.is_empty() {
                return Some(LayoutNode::Split {
                    axis: *axis,
                    ratio,
                    first: first.clone(),
                    second: second.clone(),
                });
            }

            let (head, tail) = split_path.split_first()?;
            if !head {
                update_ratio_inner(first, tail, ratio).map(|next_first| LayoutNode::Split {
                    axis: *axis,
                    ratio: *current_ratio,
                    first: Box::new(next_first),
                    second: second.clone(),
                })
            } else {
                update_ratio_inner(second, tail, ratio).map(|next_second| LayoutNode::Split {
                    axis: *axis,
                    ratio: *current_ratio,
                    first: first.clone(),
                    second: Box::new(next_second),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{close_tile, split_tile, update_split_ratio};
    use crate::model::layout::{LayoutNode, SplitAxis, default_tile_spec};

    fn single_tile_layout() -> LayoutNode {
        LayoutNode::Tile(default_tile_spec(1))
    }

    #[test]
    fn split_tile_creates_new_tile() {
        let layout = single_tile_layout();
        let next = split_tile(&layout, "tile-1", SplitAxis::Horizontal, false).unwrap();
        assert_eq!(next.tile_count(), 2);
    }

    #[test]
    fn close_tile_refuses_to_remove_last_tile() {
        let layout = single_tile_layout();
        assert!(close_tile(&layout, "tile-1").is_none());
    }

    #[test]
    fn close_tile_collapses_parent_split() {
        let layout = LayoutNode::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.5,
            first: Box::new(LayoutNode::Tile(default_tile_spec(1))),
            second: Box::new(LayoutNode::Tile(default_tile_spec(2))),
        };
        let next = close_tile(&layout, "tile-1").unwrap();
        let LayoutNode::Tile(tile) = next else {
            panic!("expected collapsed tile");
        };
        assert_eq!(tile.id, "tile-2");
    }

    #[test]
    fn update_split_ratio_changes_requested_node() {
        let layout = LayoutNode::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.5,
            first: Box::new(LayoutNode::Tile(default_tile_spec(1))),
            second: Box::new(LayoutNode::Tile(default_tile_spec(2))),
        };
        let next = update_split_ratio(&layout, &[], 0.7).unwrap();
        let LayoutNode::Split { ratio, .. } = next else {
            panic!("expected split");
        };
        assert!((ratio - 0.7).abs() < f32::EPSILON);
    }
}
