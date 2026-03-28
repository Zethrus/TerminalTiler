use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use crate::model::layout::LayoutNode;
use crate::terminal::session::TerminalSession;
use crate::ui::tile_view;

pub struct LayoutView {
    pub widget: gtk::Widget,
    pub sessions: Vec<TerminalSession>,
}

pub fn build(node: &LayoutNode, workspace_root: &Path) -> LayoutView {
    match node {
        LayoutNode::Tile(tile) => {
            let tile = tile_view::build(tile, workspace_root);
            LayoutView {
                widget: tile.widget,
                sessions: vec![tile.session],
            }
        }
        LayoutNode::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let paned = gtk::Paned::builder()
                .orientation(axis.to_orientation())
                .wide_handle(true)
                .shrink_start_child(true)
                .shrink_end_child(true)
                .build();

            let first_child = build(first, workspace_root);
            let second_child = build(second, workspace_root);
            paned.set_start_child(Some(&first_child.widget));
            paned.set_end_child(Some(&second_child.widget));

            let ratio = *ratio;
            let applied = Rc::new(Cell::new(false));
            paned.connect_map(move |paned| {
                if applied.get() {
                    return;
                }
                let applied = applied.clone();
                let paned = paned.clone();
                glib::idle_add_local_once(move || {
                    if applied.get() {
                        return;
                    }
                    applied.set(true);
                    let total = match paned.orientation() {
                        gtk::Orientation::Horizontal => paned.allocated_width(),
                        _ => paned.allocated_height(),
                    };
                    if total > 1 {
                        paned.set_position((ratio * total as f32) as i32);
                    }
                });
            });

            paned.add_css_class("split-pane");
            let mut sessions = first_child.sessions;
            sessions.extend(second_child.sessions);

            LayoutView {
                widget: paned.upcast(),
                sessions,
            }
        }
    }
}
