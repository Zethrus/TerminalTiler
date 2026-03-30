use std::cell::Cell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use crate::model::layout::LayoutNode;

pub struct LayoutShell {
    pub widget: gtk::Widget,
    pub slots: Vec<gtk::Box>,
}

pub fn build(node: &LayoutNode) -> LayoutShell {
    match node {
        LayoutNode::Tile(_) => {
            let slot = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(0)
                .hexpand(true)
                .vexpand(true)
                .build();

            LayoutShell {
                widget: slot.clone().upcast(),
                slots: vec![slot],
            }
        }
        LayoutNode::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let paned = gtk::Paned::builder()
                .orientation(match axis {
                    crate::model::layout::SplitAxis::Horizontal => gtk::Orientation::Horizontal,
                    crate::model::layout::SplitAxis::Vertical => gtk::Orientation::Vertical,
                })
                .wide_handle(true)
                .shrink_start_child(true)
                .shrink_end_child(true)
                .build();

            let first_child = build(first);
            let second_child = build(second);
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
            let mut slots = first_child.slots;
            slots.extend(second_child.slots);

            LayoutShell {
                widget: paned.upcast(),
                slots,
            }
        }
    }
}
