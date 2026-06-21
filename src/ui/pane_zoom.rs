//! Focused-pane maximize / restore.
//!
//! GTK4 `gtk::Paned` gives all available space to its visible child when the
//! other child is hidden (the drag handle disappears too). [`maximize`] walks
//! the `gtk::Paned` ancestor chain of a pane's slot and hides every sibling
//! subtree, expanding the pane to fill the workspace without reparenting any
//! live terminal or web view. [`restore`] reverses it.

use gtk::prelude::*;

/// Maximize the pane occupying `slot` by hiding each sibling subtree along its
/// `gtk::Paned` ancestor chain, stopping at `stop_at` (the layout host).
///
/// Returns the widgets that were hidden so the caller can restore them later.
/// Returns an empty vector when the slot has no `gtk::Paned` ancestor (i.e. the
/// workspace holds a single pane and there is nothing to maximize).
pub fn maximize(stop_at: &impl IsA<gtk::Widget>, slot: &gtk::Box) -> Vec<gtk::Widget> {
    let stop_at: &gtk::Widget = stop_at.as_ref();
    let mut hidden = Vec::new();
    let mut child: gtk::Widget = slot.clone().upcast();

    while let Some(parent) = child.parent() {
        if let Some(paned) = parent.downcast_ref::<gtk::Paned>() {
            let start = paned.start_child();
            let end = paned.end_child();
            // Hide whichever child is NOT the one we just ascended from.
            if start.as_ref() == Some(&child)
                && let Some(sibling) = end
            {
                sibling.set_visible(false);
                hidden.push(sibling);
            } else if end.as_ref() == Some(&child)
                && let Some(sibling) = start
            {
                sibling.set_visible(false);
                hidden.push(sibling);
            }
        }

        if &parent == stop_at {
            break;
        }
        child = parent;
    }

    hidden
}

/// Restore panes previously hidden by [`maximize`].
pub fn restore(hidden: &[gtk::Widget]) {
    for widget in hidden {
        widget.set_visible(true);
    }
}
