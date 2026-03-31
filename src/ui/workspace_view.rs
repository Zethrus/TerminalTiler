use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use gtk::prelude::*;

use crate::model::layout::LayoutNode;
use crate::model::preset::{ApplicationDensity, WorkspacePreset};
use crate::terminal::session::TerminalSession;
use crate::ui::{layout_tree, tile_view};

struct WorkspaceTile {
    tile: crate::model::layout::TileSpec,
    widget: gtk::Widget,
    session: TerminalSession,
}

struct WorkspaceRuntimeInner {
    layout: RefCell<LayoutNode>,
    slots: Vec<gtk::Box>,
    tiles: RefCell<Vec<WorkspaceTile>>,
    on_layout_changed: Rc<dyn Fn(LayoutNode)>,
}

#[derive(Clone)]
pub struct WorkspaceRuntime {
    inner: Rc<WorkspaceRuntimeInner>,
}

impl WorkspaceRuntime {
    pub fn apply_appearance(
        &self,
        use_dark_palette: bool,
        density: ApplicationDensity,
        zoom_steps: i32,
    ) {
        for tile in self.inner.tiles.borrow().iter() {
            tile.session
                .apply_appearance(use_dark_palette, density, zoom_steps);
        }
    }

    pub fn terminate_all(&self, reason: &str) {
        for tile in self.inner.tiles.borrow().iter() {
            tile.session.terminate(reason);
        }
    }

    pub fn swap_tiles(&self, dragged_id: &str, target_id: &str) -> bool {
        let next_layout = {
            let layout = self.inner.layout.borrow();
            layout.swap_tile_positions(dragged_id, target_id)
        };
        let Some(next_layout) = next_layout else {
            return false;
        };

        {
            let mut tiles = self.inner.tiles.borrow_mut();
            let Some(dragged_index) = tiles.iter().position(|tile| tile.tile.id == dragged_id)
            else {
                return false;
            };
            let Some(target_index) = tiles.iter().position(|tile| tile.tile.id == target_id) else {
                return false;
            };
            tiles.swap(dragged_index, target_index);
            remount_tiles(&self.inner.slots, &tiles);
        }

        *self.inner.layout.borrow_mut() = next_layout.clone();
        (self.inner.on_layout_changed)(next_layout);
        true
    }

    fn set_tiles(&self, tiles: Vec<WorkspaceTile>) {
        remount_tiles(&self.inner.slots, &tiles);
        *self.inner.tiles.borrow_mut() = tiles;
    }
}

pub struct WorkspaceView {
    pub widget: gtk::Widget,
    pub runtime: WorkspaceRuntime,
}

pub fn build_with_layout_change_handler(
    preset: &WorkspacePreset,
    workspace_root: &Path,
    use_dark_palette: bool,
    zoom_steps: i32,
    on_layout_changed: Rc<dyn Fn(LayoutNode)>,
) -> WorkspaceView {
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(4)
        .margin_end(4)
        .build();

    // Workspace summary header
    let summary = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(["workspace-summary"])
        .build();

    let name_label = gtk::Label::builder()
        .label(&preset.name)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["workspace-summary-name"])
        .build();

    let path_label = gtk::Label::builder()
        .label(workspace_root.display().to_string())
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .ellipsize(gtk::pango::EllipsizeMode::Start)
        .css_classes(["workspace-summary-path"])
        .build();

    summary.append(&name_label);
    summary.append(&path_label);
    shell.append(&summary);

    let layout = layout_tree::build(&preset.layout);
    let runtime = WorkspaceRuntime {
        inner: Rc::new(WorkspaceRuntimeInner {
            layout: RefCell::new(preset.layout.clone()),
            slots: layout.slots,
            tiles: RefCell::new(Vec::new()),
            on_layout_changed,
        }),
    };
    let on_swap = {
        let runtime = runtime.clone();
        Rc::new(move |dragged_id: String, target_id: String| {
            let _ = runtime.swap_tiles(&dragged_id, &target_id);
        })
    };
    let tiles = preset
        .layout
        .tile_specs()
        .into_iter()
        .map(|tile| {
            let tile_view = tile_view::build(
                &tile,
                workspace_root,
                use_dark_palette,
                preset.density,
                zoom_steps,
                on_swap.clone(),
            );
            WorkspaceTile {
                tile: tile_view.tile,
                widget: tile_view.widget,
                session: tile_view.session,
            }
        })
        .collect::<Vec<_>>();
    runtime.set_tiles(tiles);

    layout.widget.set_hexpand(true);
    layout.widget.set_vexpand(true);
    shell.append(&layout.widget);

    WorkspaceView {
        widget: shell.upcast(),
        runtime,
    }
}

fn remount_tiles(slots: &[gtk::Box], tiles: &[WorkspaceTile]) {
    for slot in slots {
        while let Some(child) = slot.first_child() {
            slot.remove(&child);
        }
    }

    for (slot, tile) in slots.iter().zip(tiles.iter()) {
        slot.append(&tile.widget);
    }
}
