use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use gtk::prelude::*;

use crate::model::assets::WorkspaceAssets;
use crate::model::layout::LayoutNode;
use crate::model::preset::{ApplicationDensity, WorkspacePreset};
use crate::services::broadcast::{BroadcastTarget, saved_groups_for_tiles};
use crate::services::layout_editor::update_split_ratio;
use crate::terminal::session::TerminalSession;
use crate::ui::{layout_tree, tile_view};

struct WorkspaceTile {
    tile: crate::model::layout::TileSpec,
    widget: gtk::Widget,
    session: TerminalSession,
}

struct WorkspaceRuntimeInner {
    layout: Rc<RefCell<LayoutNode>>,
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

    pub fn saved_groups(&self) -> Vec<String> {
        let tiles = self
            .inner
            .tiles
            .borrow()
            .iter()
            .map(|tile| tile.tile.clone())
            .collect::<Vec<_>>();
        saved_groups_for_tiles(&tiles)
    }

    pub fn send_text_to_target(&self, target: &BroadcastTarget, text: &str) -> usize {
        let mut sent = 0usize;
        for tile in self.inner.tiles.borrow().iter() {
            if target.includes(&tile.tile) {
                tile.session.send_text(text);
                sent += 1;
            }
        }
        sent
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
    assets: &WorkspaceAssets,
    use_dark_palette: bool,
    zoom_steps: i32,
    on_layout_changed: Rc<dyn Fn(LayoutNode)>,
) -> WorkspaceView {
    let layout_state = Rc::new(RefCell::new(preset.layout.clone()));
    let on_layout_changed_for_ratio = on_layout_changed.clone();
    let layout = layout_tree::build(
        &preset.layout,
        Some(Rc::new({
            let layout_state = layout_state.clone();
            move |split_path, ratio| {
                let current = layout_state.borrow().clone();
                if let Some(next_layout) = update_split_ratio(&current, &split_path, ratio) {
                    *layout_state.borrow_mut() = next_layout.clone();
                    on_layout_changed_for_ratio(next_layout);
                }
            }
        })),
    );

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
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::Start)
        .css_classes(["workspace-summary-path"])
        .build();
    let runtime = WorkspaceRuntime {
        inner: Rc::new(WorkspaceRuntimeInner {
            layout: layout_state,
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
                assets,
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

    let broadcast_target = Rc::new(RefCell::new(BroadcastTarget::Off));
    let broadcast_state = gtk::Label::builder()
        .label(BroadcastTarget::Off.label())
        .valign(gtk::Align::Center)
        .css_classes(["status-chip", "muted-chip"])
        .build();
    let broadcast_selector = gtk::ComboBoxText::new();
    broadcast_selector.append(Some("off"), "Broadcast Off");
    broadcast_selector.append(Some("all"), "Broadcast All");
    for group in runtime.saved_groups() {
        let id = format!("group:{group}");
        broadcast_selector.append(Some(&id), &format!("Group: {group}"));
    }
    broadcast_selector.set_active_id(Some("off"));

    let broadcast_entry = gtk::Entry::builder()
        .placeholder_text("Send command")
        .width_chars(18)
        .css_classes(["workspace-broadcast-entry"])
        .build();
    let broadcast_button = gtk::Button::builder()
        .label("Send")
        .css_classes(["flat"])
        .build();

    {
        let broadcast_target = broadcast_target.clone();
        let broadcast_state = broadcast_state.clone();
        broadcast_selector.connect_changed(move |combo| {
            let next_target = match combo.active_id().as_deref() {
                Some("all") => BroadcastTarget::AllPanes,
                Some(value) if value.starts_with("group:") => {
                    BroadcastTarget::SavedGroup(value.trim_start_matches("group:").to_string())
                }
                _ => BroadcastTarget::Off,
            };
            broadcast_state.set_text(&next_target.label());
            *broadcast_target.borrow_mut() = next_target;
        });
    }

    {
        let runtime = runtime.clone();
        let broadcast_target = broadcast_target.clone();
        let broadcast_entry = broadcast_entry.clone();
        let broadcast_state = broadcast_state.clone();
        broadcast_button.connect_clicked(move |_| {
            let target = broadcast_target.borrow().clone();
            let command = broadcast_entry.text().trim().to_string();
            if command.is_empty() {
                return;
            }
            let payload = if command.ends_with('\n') {
                command
            } else {
                format!("{command}\n")
            };
            let sent = runtime.send_text_to_target(&target, &payload);
            broadcast_state.set_text(&format!("{}  •  sent to {}", target.label(), sent));
        });
    }

    summary.append(&name_label);
    summary.append(&broadcast_state);
    summary.append(&broadcast_selector);
    summary.append(&broadcast_entry);
    summary.append(&broadcast_button);
    summary.append(&path_label);
    shell.append(&summary);

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
