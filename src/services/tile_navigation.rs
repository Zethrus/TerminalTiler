use std::cmp::Ordering;

use crate::model::layout::{LayoutNode, SplitAxis};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileDirection {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug)]
struct TileRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl TileRect {
    fn center_x(self) -> f32 {
        self.x + self.width / 2.0
    }

    fn center_y(self) -> f32 {
        self.y + self.height / 2.0
    }

    fn right(self) -> f32 {
        self.x + self.width
    }

    fn bottom(self) -> f32 {
        self.y + self.height
    }

    fn overlaps_x(self, other: Self) -> bool {
        self.x < other.right() && other.x < self.right()
    }

    fn overlaps_y(self, other: Self) -> bool {
        self.y < other.bottom() && other.y < self.bottom()
    }
}

#[derive(Clone, Debug)]
struct PositionedTile {
    id: String,
    rect: TileRect,
    order: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CandidateScore {
    lacks_cross_axis_overlap: bool,
    primary_distance: f32,
    cross_axis_distance: f32,
    order: usize,
}

pub fn neighbor_tile_id(
    layout: &LayoutNode,
    current_id: &str,
    direction: TileDirection,
) -> Option<String> {
    let tiles = tile_rectangles(layout);
    let current = tiles.iter().find(|tile| tile.id == current_id)?;

    tiles
        .iter()
        .filter(|candidate| candidate.id != current.id)
        .filter_map(|candidate| {
            candidate_score(current.rect, candidate.rect, direction).map(|score| {
                (
                    candidate.id.clone(),
                    CandidateScore {
                        order: candidate.order,
                        ..score
                    },
                )
            })
        })
        .min_by(|(_, left), (_, right)| compare_score(*left, *right))
        .map(|(id, _)| id)
}

fn tile_rectangles(layout: &LayoutNode) -> Vec<PositionedTile> {
    let mut tiles = Vec::new();
    collect_tile_rectangles(
        layout,
        TileRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
        &mut tiles,
    );
    tiles
}

fn collect_tile_rectangles(layout: &LayoutNode, rect: TileRect, tiles: &mut Vec<PositionedTile>) {
    match layout {
        LayoutNode::Tile(tile) => tiles.push(PositionedTile {
            id: tile.id.clone(),
            rect,
            order: tiles.len(),
        }),
        LayoutNode::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let ratio = ratio.clamp(0.0, 1.0);
            match axis {
                SplitAxis::Horizontal => {
                    let first_width = rect.width * ratio;
                    let first_rect = TileRect {
                        width: first_width,
                        ..rect
                    };
                    let second_rect = TileRect {
                        x: rect.x + first_width,
                        width: rect.width - first_width,
                        ..rect
                    };
                    collect_tile_rectangles(first, first_rect, tiles);
                    collect_tile_rectangles(second, second_rect, tiles);
                }
                SplitAxis::Vertical => {
                    let first_height = rect.height * ratio;
                    let first_rect = TileRect {
                        height: first_height,
                        ..rect
                    };
                    let second_rect = TileRect {
                        y: rect.y + first_height,
                        height: rect.height - first_height,
                        ..rect
                    };
                    collect_tile_rectangles(first, first_rect, tiles);
                    collect_tile_rectangles(second, second_rect, tiles);
                }
            }
        }
    }
}

fn candidate_score(
    current: TileRect,
    candidate: TileRect,
    direction: TileDirection,
) -> Option<CandidateScore> {
    let current_center_x = current.center_x();
    let current_center_y = current.center_y();
    let candidate_center_x = candidate.center_x();
    let candidate_center_y = candidate.center_y();

    let (primary_distance, cross_axis_distance, overlaps_cross_axis) = match direction {
        TileDirection::Up => {
            if candidate_center_y >= current_center_y {
                return None;
            }
            (
                current_center_y - candidate_center_y,
                (current_center_x - candidate_center_x).abs(),
                current.overlaps_x(candidate),
            )
        }
        TileDirection::Down => {
            if candidate_center_y <= current_center_y {
                return None;
            }
            (
                candidate_center_y - current_center_y,
                (current_center_x - candidate_center_x).abs(),
                current.overlaps_x(candidate),
            )
        }
        TileDirection::Left => {
            if candidate_center_x >= current_center_x {
                return None;
            }
            (
                current_center_x - candidate_center_x,
                (current_center_y - candidate_center_y).abs(),
                current.overlaps_y(candidate),
            )
        }
        TileDirection::Right => {
            if candidate_center_x <= current_center_x {
                return None;
            }
            (
                candidate_center_x - current_center_x,
                (current_center_y - candidate_center_y).abs(),
                current.overlaps_y(candidate),
            )
        }
    };

    Some(CandidateScore {
        lacks_cross_axis_overlap: !overlaps_cross_axis,
        primary_distance,
        cross_axis_distance,
        order: 0,
    })
}

fn compare_score(left: CandidateScore, right: CandidateScore) -> Ordering {
    left.lacks_cross_axis_overlap
        .cmp(&right.lacks_cross_axis_overlap)
        .then_with(|| compare_f32(left.primary_distance, right.primary_distance))
        .then_with(|| compare_f32(left.cross_axis_distance, right.cross_axis_distance))
        .then_with(|| left.order.cmp(&right.order))
}

fn compare_f32(left: f32, right: f32) -> Ordering {
    left.partial_cmp(&right).unwrap_or(Ordering::Equal)
}

#[cfg(test)]
mod tests {
    use super::{TileDirection, neighbor_tile_id};
    use crate::model::layout::{LayoutNode, SplitAxis, WorkingDirectory, split, tile};

    fn test_tile(id: &str) -> LayoutNode {
        tile(
            id,
            id,
            "Shell",
            "accent-cyan",
            WorkingDirectory::WorkspaceRoot,
            None,
        )
    }

    #[test]
    fn vertical_stack_bottom_up_selects_top() {
        let layout = split(
            SplitAxis::Vertical,
            0.5,
            test_tile("top"),
            test_tile("bottom"),
        );

        assert_eq!(
            neighbor_tile_id(&layout, "bottom", TileDirection::Up).as_deref(),
            Some("top")
        );
    }

    #[test]
    fn horizontal_split_left_right_selects_right() {
        let layout = split(
            SplitAxis::Horizontal,
            0.5,
            test_tile("left"),
            test_tile("right"),
        );

        assert_eq!(
            neighbor_tile_id(&layout, "left", TileDirection::Right).as_deref(),
            Some("right")
        );
    }

    #[test]
    fn mixed_split_selects_nearest_spatial_neighbor() {
        let layout = split(
            SplitAxis::Horizontal,
            0.5,
            test_tile("left"),
            split(
                SplitAxis::Vertical,
                0.5,
                test_tile("top-right"),
                test_tile("bottom-right"),
            ),
        );

        assert_eq!(
            neighbor_tile_id(&layout, "left", TileDirection::Right).as_deref(),
            Some("top-right")
        );
        assert_eq!(
            neighbor_tile_id(&layout, "bottom-right", TileDirection::Up).as_deref(),
            Some("top-right")
        );
        assert_eq!(
            neighbor_tile_id(&layout, "top-right", TileDirection::Left).as_deref(),
            Some("left")
        );
        assert_eq!(
            neighbor_tile_id(&layout, "top-right", TileDirection::Down).as_deref(),
            Some("bottom-right")
        );
    }

    #[test]
    fn edge_directions_return_none() {
        let layout = split(
            SplitAxis::Horizontal,
            0.5,
            test_tile("left"),
            test_tile("right"),
        );

        assert_eq!(neighbor_tile_id(&layout, "left", TileDirection::Left), None);
        assert_eq!(
            neighbor_tile_id(&layout, "right", TileDirection::Right),
            None
        );
    }

    #[test]
    fn ties_are_deterministic_by_layout_order() {
        let layout = split(
            SplitAxis::Horizontal,
            0.5,
            test_tile("left"),
            split(
                SplitAxis::Vertical,
                0.5,
                test_tile("top-right"),
                test_tile("bottom-right"),
            ),
        );

        assert_eq!(
            neighbor_tile_id(&layout, "left", TileDirection::Right).as_deref(),
            Some("top-right")
        );
    }
}
