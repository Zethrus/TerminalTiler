use std::cell::Cell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use crate::model::layout::LayoutNode;

type RatioChangedHandler = Rc<dyn Fn(Vec<bool>, f32)>;

pub struct LayoutShell {
    pub widget: gtk::Widget,
    pub slots: Vec<gtk::Box>,
}

pub fn build(node: &LayoutNode, on_ratio_changed: Option<RatioChangedHandler>) -> LayoutShell {
    build_with_path(node, &[], on_ratio_changed)
}

fn build_with_path(
    node: &LayoutNode,
    split_path: &[bool],
    on_ratio_changed: Option<RatioChangedHandler>,
) -> LayoutShell {
    match node {
        LayoutNode::Tile(_) => {
            let slot = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(0)
                .hexpand(true)
                .vexpand(true)
                .build();
            make_shrinkable(&slot);

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
                .resize_start_child(true)
                .resize_end_child(true)
                .shrink_start_child(true)
                .shrink_end_child(true)
                .build();
            make_shrinkable(&paned);

            let mut first_path = split_path.to_vec();
            first_path.push(false);
            let first_child = build_with_path(first, &first_path, on_ratio_changed.clone());
            let mut second_path = split_path.to_vec();
            second_path.push(true);
            let second_child = build_with_path(second, &second_path, on_ratio_changed.clone());
            paned.set_start_child(Some(&first_child.widget));
            paned.set_end_child(Some(&second_child.widget));

            let ratio = *ratio;
            let applied = Rc::new(Cell::new(false));
            let suppress_position_notify = Rc::new(Cell::new(false));
            let suppress_saved_ratio_notify = suppress_position_notify.clone();
            paned.connect_map(move |paned| {
                if applied.get() {
                    return;
                }
                apply_saved_ratio_when_allocated(
                    paned,
                    ratio,
                    applied.clone(),
                    suppress_saved_ratio_notify.clone(),
                );
            });

            if let Some(on_ratio_changed) = on_ratio_changed {
                let path = split_path.to_vec();
                let suppress_position_notify = suppress_position_notify.clone();
                paned.connect_position_notify(move |paned| {
                    if suppress_position_notify.get() {
                        return;
                    }
                    let total = match paned.orientation() {
                        gtk::Orientation::Horizontal => paned.allocated_width(),
                        _ => paned.allocated_height(),
                    };
                    if total > 1 {
                        on_ratio_changed(path.clone(), paned.position() as f32 / total as f32);
                    }
                });
            }

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

fn apply_saved_ratio_when_allocated(
    paned: &gtk::Paned,
    ratio: f32,
    applied: Rc<Cell<bool>>,
    suppress_position_notify: Rc<Cell<bool>>,
) {
    paned.add_tick_callback(move |paned, _| {
        if applied.get() {
            return glib::ControlFlow::Break;
        }

        let total = match paned.orientation() {
            gtk::Orientation::Horizontal => paned.allocated_width(),
            _ => paned.allocated_height(),
        };

        if total > 1 {
            applied.set(true);
            suppress_position_notify.set(true);
            paned.set_position((ratio * total as f32) as i32);

            let suppress_position_notify = suppress_position_notify.clone();
            glib::idle_add_local_once(move || {
                suppress_position_notify.set(false);
            });
            return glib::ControlFlow::Break;
        }

        glib::ControlFlow::Continue
    });
}

fn make_shrinkable(widget: &impl IsA<gtk::Widget>) {
    widget.set_size_request(0, 0);
    widget.set_overflow(gtk::Overflow::Hidden);
}
