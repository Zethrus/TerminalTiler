use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use vte4::prelude::TerminalExt;
use webkit6::prelude::WebViewExt;

use crate::logging;
use crate::model::assets::{Runbook, WorkspaceAssets};
use crate::model::layout::TileKind;
use crate::model::layout::{LayoutNode, SplitAxis};
use crate::model::preset::{ApplicationDensity, WorkspacePreset};
use crate::services::alerts::{AlertEventInput, AlertSeverity, AlertSourceKind, AlertStore};
use crate::services::broadcast::{BroadcastTarget, saved_groups_for_tiles};
use crate::services::layout_editor::{
    close_tile as close_layout_tile, split_tile_with_kind, update_split_ratio,
};
use crate::services::output_helpers::{helper_summary_text, scan_output};
use crate::services::runbooks::{ResolvedRunbook, resolve_runbook};
use crate::terminal::session::TerminalSession;
use crate::ui::{layout_tree, tile_view, web_tile};

fn reconnect_delay_seconds(attempt: u32) -> u32 {
    2u32.pow(attempt.saturating_sub(1).min(5)).min(60)
}

struct WorkspaceTile {
    tile: crate::model::layout::TileSpec,
    widget: gtk::Widget,
    session: Option<TerminalSession>,
    web_view: Option<webkit6::WebView>,
    refresh_source_id: Option<Rc<RefCell<Option<glib::SourceId>>>>,
    close_button: gtk::Button,
    handlers_bound: bool,
}

struct WorkspaceRuntimeInner {
    layout: Rc<RefCell<LayoutNode>>,
    slots: RefCell<Vec<gtk::Box>>,
    tiles: RefCell<Vec<WorkspaceTile>>,
    layout_host: gtk::Box,
    on_layout_changed: Rc<dyn Fn(LayoutNode)>,
    alert_store: AlertStore,
    workspace_root: PathBuf,
    assets: WorkspaceAssets,
    use_dark_palette: bool,
    density: ApplicationDensity,
    zoom_steps: i32,
    max_reconnect_attempts: u32,
    path_label: gtk::Label,
    url_entry: gtk::Entry,
    url_reload_button: gtk::Button,
    focused_tile_id: RefCell<Option<String>>,
    focused_web_tile_id: RefCell<Option<String>>,
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
            if let Some(session) = &tile.session {
                session.apply_appearance(use_dark_palette, density, zoom_steps);
            }
        }
    }

    pub fn terminate_all(&self, reason: &str) {
        for tile in self.inner.tiles.borrow().iter() {
            if let Some(session) = &tile.session {
                session.terminate(reason);
            }
        }
    }

    pub fn has_active_processes(&self) -> bool {
        self.inner
            .tiles
            .borrow()
            .iter()
            .filter_map(|tile| tile.session.as_ref())
            .any(TerminalSession::has_active_process)
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

    pub fn tile_specs(&self) -> Vec<crate::model::layout::TileSpec> {
        self.inner
            .tiles
            .borrow()
            .iter()
            .map(|tile| tile.tile.clone())
            .collect()
    }

    pub fn alert_store(&self) -> AlertStore {
        self.inner.alert_store.clone()
    }

    pub fn send_text_to_target(&self, target: &BroadcastTarget, text: &str) -> usize {
        let mut sent = 0usize;
        for tile in self.inner.tiles.borrow().iter() {
            if target.includes(&tile.tile) {
                if let Some(session) = &tile.session {
                    session.send_text(text);
                    sent += 1;
                }
            }
        }
        sent
    }

    pub fn run_runbook(&self, resolved: &ResolvedRunbook) -> usize {
        let mut sent = 0usize;
        for command in &resolved.commands {
            sent += self.send_text_to_target(&resolved.target, command);
        }
        sent
    }

    pub fn focus_tile(&self, tile_id: &str) -> bool {
        if let Some(tile) = self
            .inner
            .tiles
            .borrow()
            .iter()
            .find(|tile| tile.tile.id == tile_id)
        {
            if let Some(session) = &tile.session {
                session.widget().grab_focus();
            } else if let Some(web_view) = &tile.web_view {
                web_view.grab_focus();
            } else {
                tile.widget.grab_focus();
            }
            true
        } else {
            false
        }
    }

    pub fn reconnect_tile(&self, tile_id: &str) -> Result<(), String> {
        let session = {
            let tiles = self.inner.tiles.borrow();
            tiles
                .iter()
                .find(|tile| tile.tile.id == tile_id)
                .and_then(|tile| tile.session.clone())
        };
        let Some(session) = session else {
            return Err(format!("Pane '{tile_id}' has no terminal session."));
        };
        session.reset_auto_reconnect_attempts();
        session.reconnect()
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
            remount_tiles(&self.inner.slots.borrow(), &tiles);
        }

        *self.inner.layout.borrow_mut() = next_layout.clone();
        (self.inner.on_layout_changed)(next_layout);
        true
    }

    pub fn close_tile(&self, tile_id: &str) -> bool {
        let current_layout = self.inner.layout.borrow().clone();
        let Some(next_layout) = close_layout_tile(&current_layout, tile_id) else {
            return false;
        };

        let ordered_specs = next_layout.tile_specs();
        let next_tile_ids = ordered_specs
            .iter()
            .map(|tile| tile.id.clone())
            .collect::<Vec<_>>();
        let mut existing_tiles = self
            .inner
            .tiles
            .borrow_mut()
            .drain(..)
            .map(|tile| (tile.tile.id.clone(), tile))
            .collect::<HashMap<_, _>>();
        detach_tile_widgets(existing_tiles.values());
        let next_tiles = ordered_specs
            .into_iter()
            .map(|spec| {
                if let Some(mut tile) = existing_tiles.remove(&spec.id) {
                    tile.tile = spec;
                    tile
                } else {
                    self.build_tile(&spec)
                }
            })
            .collect::<Vec<_>>();

        for (_, removed_tile) in existing_tiles {
            clear_web_refresh_timer(removed_tile.refresh_source_id.as_ref());
            if let Some(session) = removed_tile.session {
                session.terminate("tile closed");
            }
        }

        let next_focused_tile = self
            .inner
            .focused_tile_id
            .borrow()
            .clone()
            .filter(|focused_id| next_tile_ids.iter().any(|tile_id| tile_id == focused_id))
            .or_else(|| next_tile_ids.first().cloned());
        let next_focused_web_tile = self
            .inner
            .focused_web_tile_id
            .borrow()
            .clone()
            .filter(|focused_id| {
                next_tiles
                    .iter()
                    .any(|tile| tile.tile.id == *focused_id && tile.web_view.is_some())
            })
            .or_else(|| {
                next_focused_tile.as_ref().and_then(|focused_id| {
                    next_tiles
                        .iter()
                        .find(|tile| tile.tile.id == *focused_id && tile.web_view.is_some())
                        .map(|tile| tile.tile.id.clone())
                })
            });

        *self.inner.layout.borrow_mut() = next_layout.clone();
        *self.inner.focused_tile_id.borrow_mut() = next_focused_tile;
        *self.inner.focused_web_tile_id.borrow_mut() = next_focused_web_tile;
        self.replace_layout_shell(&next_layout);
        self.set_tiles(next_tiles);
        (self.inner.on_layout_changed)(next_layout);
        true
    }

    pub fn navigate_web_tile(&self, tile_id: &str, url: &str) -> bool {
        let normalized_url = normalize_web_tile_url(url);

        let web_view = {
            let mut tiles = self.inner.tiles.borrow_mut();
            let Some(tile) = tiles.iter_mut().find(|t| t.tile.id == tile_id) else {
                return false;
            };
            let Some(web_view) = tile.web_view.clone() else {
                return false;
            };
            tile.tile.url = Some(normalized_url.clone());
            web_view
        };

        logging::info(format!(
            "web tile {} navigating to {}",
            tile_id, normalized_url
        ));
        web_view.load_uri(&normalized_url);

        let persisted_url = normalized_url.clone();
        let _ = self.update_layout_tile(tile_id, move |tile| {
            tile.url = Some(persisted_url.clone());
        });
        if self.current_focused_web_tile().as_deref() == Some(tile_id) {
            self.inner.url_entry.set_text(&normalized_url);
        }

        true
    }

    pub fn update_web_tile_settings(
        &self,
        tile_id: &str,
        url: &str,
        auto_refresh_seconds: Option<u32>,
    ) -> bool {
        let normalized_url = normalize_web_tile_url(url);

        let (web_view, refresh_source_id, url_changed, refresh_changed) = {
            let mut tiles = self.inner.tiles.borrow_mut();
            let Some(tile) = tiles.iter_mut().find(|t| t.tile.id == tile_id) else {
                return false;
            };
            let Some(web_view) = tile.web_view.clone() else {
                return false;
            };

            let url_changed = tile.tile.url.as_deref() != Some(normalized_url.as_str());
            let refresh_changed = tile.tile.auto_refresh_seconds != auto_refresh_seconds;

            if url_changed {
                tile.tile.url = Some(normalized_url.clone());
            }
            if refresh_changed {
                tile.tile.auto_refresh_seconds = auto_refresh_seconds;
            }

            (
                web_view,
                tile.refresh_source_id.clone(),
                url_changed,
                refresh_changed,
            )
        };

        if refresh_changed && let Some(refresh_source_id) = refresh_source_id.as_ref() {
            configure_web_refresh_timer(refresh_source_id, &web_view, auto_refresh_seconds);
            logging::info(format!(
                "web tile {} auto-refresh set to {}",
                tile_id,
                auto_refresh_seconds
                    .map(|seconds| format!("{}s", seconds))
                    .unwrap_or_else(|| "disabled".into())
            ));
        }

        if url_changed {
            logging::info(format!(
                "web tile {} settings updated, navigating to {}",
                tile_id, normalized_url
            ));
            web_view.load_uri(&normalized_url);
        }

        if url_changed || refresh_changed {
            let persisted_url = normalized_url.clone();
            let _ = self.update_layout_tile(tile_id, move |tile| {
                tile.url = Some(persisted_url.clone());
                tile.auto_refresh_seconds = auto_refresh_seconds;
            });
            if self.current_focused_web_tile().as_deref() == Some(tile_id) {
                self.inner.url_entry.set_text(&normalized_url);
            }
        }

        url_changed || refresh_changed
    }

    pub fn reload_web_tile(&self, tile_id: &str) -> bool {
        let web_view = {
            let tiles = self.inner.tiles.borrow();
            tiles
                .iter()
                .find(|t| t.tile.id == tile_id)
                .and_then(|tile| tile.web_view.clone())
        };

        if let Some(web_view) = web_view {
            logging::info(format!("web tile {} reload requested", tile_id));
            web_view.reload();
            true
        } else {
            false
        }
    }

    pub fn web_tile_uri(&self, tile_id: &str) -> Option<String> {
        let tiles = self.inner.tiles.borrow();
        tiles.iter().find(|t| t.tile.id == tile_id).and_then(|t| {
            t.web_view
                .as_ref()
                .and_then(|wv| wv.uri())
                .map(|s: gtk::glib::GString| s.to_string())
                .or_else(|| t.tile.url.clone())
        })
    }

    pub fn has_web_tiles(&self) -> bool {
        self.inner
            .tiles
            .borrow()
            .iter()
            .any(|t| t.web_view.is_some())
    }

    pub fn current_focused_web_tile(&self) -> Option<String> {
        self.inner.focused_web_tile_id.borrow().clone()
    }

    fn web_tile_settings(&self, tile_id: &str) -> Option<(String, Option<u32>)> {
        let tiles = self.inner.tiles.borrow();
        tiles
            .iter()
            .find(|tile| tile.tile.id == tile_id)
            .map(|tile| {
                (
                    tile.web_view
                        .as_ref()
                        .and_then(|web_view| web_view.uri())
                        .map(|uri| uri.to_string())
                        .or_else(|| tile.tile.url.clone())
                        .unwrap_or_else(|| "about:blank".into()),
                    tile.tile.auto_refresh_seconds,
                )
            })
    }

    pub fn add_web_tile(&self) -> Option<String> {
        let target_tile_id = self.inner.focused_tile_id.borrow().clone().or_else(|| {
            self.inner
                .tiles
                .borrow()
                .first()
                .map(|tile| tile.tile.id.clone())
        })?;

        let current_layout = self.inner.layout.borrow().clone();
        let (next_layout, new_tile_id) = split_tile_with_kind(
            &current_layout,
            &target_tile_id,
            SplitAxis::Horizontal,
            false,
            TileKind::WebView,
        )?;

        let ordered_specs = next_layout.tile_specs();
        let mut existing_tiles = self
            .inner
            .tiles
            .borrow_mut()
            .drain(..)
            .map(|tile| (tile.tile.id.clone(), tile))
            .collect::<HashMap<_, _>>();
        detach_tile_widgets(existing_tiles.values());
        let next_tiles = ordered_specs
            .into_iter()
            .map(|spec| {
                if let Some(mut tile) = existing_tiles.remove(&spec.id) {
                    tile.tile = spec;
                    tile
                } else {
                    self.build_tile(&spec)
                }
            })
            .collect::<Vec<_>>();

        *self.inner.layout.borrow_mut() = next_layout.clone();
        self.replace_layout_shell(&next_layout);
        self.set_tiles(next_tiles);
        *self.inner.focused_tile_id.borrow_mut() = Some(new_tile_id.clone());
        *self.inner.focused_web_tile_id.borrow_mut() = Some(new_tile_id.clone());
        self.refresh_navigation_controls();
        self.inner.url_entry.set_text("about:blank");
        self.inner.url_entry.grab_focus();
        (self.inner.on_layout_changed)(next_layout);

        Some(new_tile_id)
    }

    fn set_tiles(&self, mut tiles: Vec<WorkspaceTile>) {
        self.bind_tile_handlers(&mut tiles);
        remount_tiles(&self.inner.slots.borrow(), &tiles);
        *self.inner.tiles.borrow_mut() = tiles;
        self.sync_tile_close_buttons();
        self.refresh_navigation_controls();
    }

    fn build_tile(&self, tile: &crate::model::layout::TileSpec) -> WorkspaceTile {
        let on_swap = {
            let runtime = self.clone();
            Rc::new(move |dragged_id: String, target_id: String| {
                let _ = runtime.swap_tiles(&dragged_id, &target_id);
            })
        };
        let on_close = {
            let runtime = self.clone();
            Rc::new(move |tile_id: String| {
                let _ = runtime.close_tile(&tile_id);
            })
        };
        let can_close = self.inner.layout.borrow().tile_count() > 1;

        match tile.tile_kind {
            TileKind::WebView => {
                let on_update_settings = {
                    let runtime = self.clone();
                    Rc::new(
                        move |tile_id: String, url: String, auto_refresh_seconds: Option<u32>| {
                            let _ = runtime.update_web_tile_settings(
                                &tile_id,
                                &url,
                                auto_refresh_seconds,
                            );
                        },
                    )
                };
                let on_reload = {
                    let runtime = self.clone();
                    Rc::new(move |tile_id: String| {
                        let _ = runtime.reload_web_tile(&tile_id);
                    })
                };
                let get_settings = {
                    let runtime = self.clone();
                    Rc::new(move |tile_id: String| runtime.web_tile_settings(&tile_id))
                };
                let web = web_tile::build(
                    tile,
                    &self.inner.assets,
                    self.inner.use_dark_palette,
                    self.inner.density,
                    on_swap,
                    on_close,
                    on_update_settings,
                    on_reload,
                    get_settings,
                    can_close,
                );
                WorkspaceTile {
                    tile: web.tile,
                    widget: web.widget,
                    session: None,
                    web_view: Some(web.web_view),
                    refresh_source_id: Some(web.refresh_source_id),
                    close_button: web.close_button,
                    handlers_bound: false,
                }
            }
            TileKind::Terminal => {
                let tile_view = tile_view::build(
                    tile,
                    &self.inner.workspace_root,
                    &self.inner.assets,
                    self.inner.use_dark_palette,
                    self.inner.density,
                    self.inner.zoom_steps,
                    on_swap,
                    on_close,
                    can_close,
                );
                install_tile_alert_hooks(
                    &tile_view.session,
                    tile,
                    &self.inner.alert_store,
                    self.inner.max_reconnect_attempts,
                );
                WorkspaceTile {
                    tile: tile_view.tile,
                    widget: tile_view.widget,
                    session: Some(tile_view.session),
                    web_view: None,
                    refresh_source_id: None,
                    close_button: tile_view.close_button,
                    handlers_bound: false,
                }
            }
        }
    }

    fn sync_tile_close_buttons(&self) {
        let can_close = self.inner.tiles.borrow().len() > 1;
        for tile in self.inner.tiles.borrow().iter() {
            tile.close_button.set_sensitive(can_close);
            tile.close_button.set_tooltip_text(Some(if can_close {
                "Close tile"
            } else {
                "Cannot close the last tile"
            }));
        }
    }

    fn bind_tile_handlers(&self, tiles: &mut [WorkspaceTile]) {
        for tile in tiles.iter_mut() {
            if tile.handlers_bound {
                continue;
            }

            let focus_target: gtk::Widget = if let Some(session) = &tile.session {
                session.widget().upcast()
            } else if let Some(web_view) = &tile.web_view {
                web_view.clone().upcast()
            } else {
                tile.widget.clone()
            };

            let runtime = self.clone();
            let tile_id = tile.tile.id.clone();
            let tile_kind = tile.tile.tile_kind;
            let controller = gtk::EventControllerFocus::new();
            controller.connect_enter(move |_| {
                runtime.set_focused_tile(Some(tile_id.clone()), tile_kind == TileKind::WebView);
            });
            focus_target.add_controller(controller);

            if let Some(web_view) = &tile.web_view {
                let runtime = self.clone();
                let tile_id = tile.tile.id.clone();
                web_view.connect_uri_notify(move |wv| {
                    if let Some(uri) = wv.uri() {
                        runtime.record_web_tile_uri(&tile_id, uri.as_str());
                    }
                });
            }

            tile.handlers_bound = true;
        }
    }

    fn refresh_navigation_controls(&self) {
        let has_web_tiles = self.has_web_tiles();
        self.inner.path_label.set_visible(!has_web_tiles);
        self.inner.url_entry.set_visible(has_web_tiles);
        self.inner.url_reload_button.set_visible(has_web_tiles);

        let focused_web_tile = self.inner.focused_web_tile_id.borrow().clone();
        let current_url = focused_web_tile
            .as_deref()
            .and_then(|tile_id| self.web_tile_uri(tile_id))
            .unwrap_or_default();
        let web_controls_enabled = focused_web_tile.is_some();
        self.inner.url_entry.set_sensitive(web_controls_enabled);
        self.inner
            .url_reload_button
            .set_sensitive(web_controls_enabled);
        if has_web_tiles {
            self.inner.url_entry.set_text(&current_url);
        }
    }

    fn set_focused_tile(&self, tile_id: Option<String>, is_web: bool) {
        *self.inner.focused_tile_id.borrow_mut() = tile_id.clone();
        *self.inner.focused_web_tile_id.borrow_mut() = if is_web { tile_id } else { None };
        self.refresh_navigation_controls();
    }

    fn record_web_tile_uri(&self, tile_id: &str, uri: &str) {
        let next_uri = uri.to_string();
        let changed = {
            let mut tiles = self.inner.tiles.borrow_mut();
            let Some(tile) = tiles.iter_mut().find(|tile| tile.tile.id == tile_id) else {
                return;
            };
            if tile.tile.url.as_deref() == Some(uri) {
                false
            } else {
                tile.tile.url = Some(next_uri.clone());
                true
            }
        };

        if changed {
            logging::info(format!("web tile {} uri updated to {}", tile_id, next_uri));
            let persisted_uri = next_uri.clone();
            let _ = self.update_layout_tile(tile_id, move |tile| {
                tile.url = Some(persisted_uri.clone());
            });
        }

        if self.current_focused_web_tile().as_deref() == Some(tile_id) {
            self.inner.url_entry.set_text(&next_uri);
        }
    }

    fn update_layout_tile<F>(&self, tile_id: &str, update: F) -> bool
    where
        F: FnOnce(&mut crate::model::layout::TileSpec),
    {
        let current_layout = self.inner.layout.borrow().clone();
        let mut tile_specs = current_layout.tile_specs();
        let Some(tile) = tile_specs.iter_mut().find(|tile| tile.id == tile_id) else {
            return false;
        };

        update(tile);
        let next_layout = current_layout.with_tile_specs(&tile_specs);
        *self.inner.layout.borrow_mut() = next_layout.clone();
        (self.inner.on_layout_changed)(next_layout);
        true
    }

    fn replace_layout_shell(&self, layout: &LayoutNode) {
        let layout_state = self.inner.layout.clone();
        let on_layout_changed = self.inner.on_layout_changed.clone();
        let layout_shell = layout_tree::build(
            layout,
            Some(Rc::new(move |split_path, ratio| {
                let current = layout_state.borrow().clone();
                if let Some(next_layout) = update_split_ratio(&current, &split_path, ratio) {
                    *layout_state.borrow_mut() = next_layout.clone();
                    on_layout_changed(next_layout);
                }
            })),
        );
        layout_shell.widget.set_hexpand(true);
        layout_shell.widget.set_vexpand(true);
        while let Some(child) = self.inner.layout_host.first_child() {
            self.inner.layout_host.remove(&child);
        }
        self.inner.layout_host.append(&layout_shell.widget);
        *self.inner.slots.borrow_mut() = layout_shell.slots;
    }

    fn rebuild_from_layout(&self) {
        let layout = self.inner.layout.borrow().clone();
        self.replace_layout_shell(&layout);
        let tiles = layout
            .tile_specs()
            .into_iter()
            .map(|tile| self.build_tile(&tile))
            .collect::<Vec<_>>();
        self.set_tiles(tiles);
    }
}

fn normalize_web_tile_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        "about:blank".into()
    } else if trimmed.contains("://") || trimmed.starts_with("about:") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    }
}

fn clear_web_refresh_timer(refresh_source_id: Option<&Rc<RefCell<Option<glib::SourceId>>>>) {
    let Some(refresh_source_id) = refresh_source_id else {
        return;
    };
    if let Some(source_id) = refresh_source_id.borrow_mut().take() {
        source_id.remove();
    }
}

fn configure_web_refresh_timer(
    refresh_source_id: &Rc<RefCell<Option<glib::SourceId>>>,
    web_view: &webkit6::WebView,
    auto_refresh_seconds: Option<u32>,
) {
    clear_web_refresh_timer(Some(refresh_source_id));
    let Some(interval) = auto_refresh_seconds.filter(|interval| *interval > 0) else {
        return;
    };

    let web_view = web_view.clone();
    let source_id = glib::timeout_add_seconds_local(interval, move || {
        web_view.reload();
        glib::ControlFlow::Continue
    });
    *refresh_source_id.borrow_mut() = Some(source_id);
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
    max_reconnect_attempts: u32,
    on_layout_changed: Rc<dyn Fn(LayoutNode)>,
) -> WorkspaceView {
    let layout_state = Rc::new(RefCell::new(preset.layout.clone()));

    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(4)
        .margin_end(4)
        .build();

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

    let url_entry = gtk::Entry::builder()
        .placeholder_text("URL")
        .width_chars(30)
        .hexpand(false)
        .css_classes(["workspace-url-entry"])
        .build();
    let url_reload_button = gtk::Button::builder()
        .label("Reload")
        .css_classes(["flat", "surface-button"])
        .build();
    let layout_host = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .hexpand(true)
        .vexpand(true)
        .build();

    let alert_store = AlertStore::default();
    let runtime = WorkspaceRuntime {
        inner: Rc::new(WorkspaceRuntimeInner {
            layout: layout_state,
            slots: RefCell::new(Vec::new()),
            tiles: RefCell::new(Vec::new()),
            layout_host: layout_host.clone(),
            on_layout_changed,
            alert_store: alert_store.clone(),
            workspace_root: workspace_root.to_path_buf(),
            assets: assets.clone(),
            use_dark_palette,
            density: preset.density,
            zoom_steps,
            max_reconnect_attempts,
            path_label: path_label.clone(),
            url_entry: url_entry.clone(),
            url_reload_button: url_reload_button.clone(),
            focused_tile_id: RefCell::new(None),
            focused_web_tile_id: RefCell::new(None),
        }),
    };
    runtime.rebuild_from_layout();

    // Navigate on Enter
    {
        let runtime = runtime.clone();
        url_entry.connect_activate(move |entry| {
            let url = entry.text().to_string();
            if url.is_empty() {
                return;
            }
            if let Some(tile_id) = runtime.current_focused_web_tile() {
                runtime.navigate_web_tile(&tile_id, &url);
            }
        });
    }

    // Reload button
    {
        let runtime = runtime.clone();
        url_reload_button.connect_clicked(move |_| {
            if let Some(tile_id) = runtime.current_focused_web_tile() {
                runtime.reload_web_tile(&tile_id);
            }
        });
    }

    let add_web_tile_button = gtk::Button::builder()
        .label("Add Web Tile")
        .css_classes(["flat", "surface-button"])
        .build();
    {
        let runtime = runtime.clone();
        add_web_tile_button.connect_clicked(move |_| {
            let _ = runtime.add_web_tile();
        });
    }

    let broadcast_target = Rc::new(RefCell::new(BroadcastTarget::Off));
    let broadcast_state = gtk::Label::builder()
        .label(BroadcastTarget::Off.label())
        .valign(gtk::Align::Center)
        .css_classes(["status-chip", "muted-chip"])
        .build();
    let broadcast_selector = gtk::ComboBoxText::new();
    broadcast_selector.add_css_class("surface-select-control");
    broadcast_selector.append(Some("off"), "Broadcast Off");
    broadcast_selector.append(Some("all"), "Broadcast All");
    for group in runtime.saved_groups() {
        let id = format!("group:{group}");
        broadcast_selector.append(Some(&id), &format!("Group: {group}"));
    }
    broadcast_selector.set_active_id(Some("off"));

    let broadcast_entry = gtk::Entry::builder()
        .placeholder_text("Quick send command")
        .width_chars(18)
        .css_classes(["workspace-broadcast-entry"])
        .build();
    let broadcast_button = gtk::Button::builder()
        .label("Send")
        .css_classes(["flat", "surface-button"])
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
        let alert_store = alert_store.clone();
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
            alert_store.push(AlertEventInput {
                source: AlertSourceKind::Runbook,
                severity: AlertSeverity::Info,
                title: "Quick send executed".into(),
                detail: format!("Sent quick command to {} pane(s).", sent),
                pane_id: None,
                allows_reconnect: false,
            });
        });
    }

    let runbook_selector = gtk::ComboBoxText::new();
    runbook_selector.add_css_class("surface-select-control");
    runbook_selector.append(Some(""), "Runbook");
    for runbook in &assets.runbooks {
        runbook_selector.append(Some(&runbook.id), &runbook.name);
    }
    runbook_selector.set_active_id(Some(""));
    let runbook_button = gtk::Button::builder()
        .label("Run")
        .css_classes(["flat", "surface-button"])
        .sensitive(!assets.runbooks.is_empty())
        .build();
    {
        let runtime = runtime.clone();
        let alert_store = alert_store.clone();
        let runbooks = assets.runbooks.clone();
        let runbook_selector = runbook_selector.clone();
        let broadcast_state = broadcast_state.clone();
        runbook_button.connect_clicked(move |button| {
            let Some(runbook_id) = runbook_selector.active_id() else {
                return;
            };
            if runbook_id.is_empty() {
                return;
            }
            let Some(runbook) = runbooks.iter().find(|runbook| runbook.id == runbook_id) else {
                return;
            };
            present_runbook_dialog(button, runbook, &runtime, &alert_store, &broadcast_state);
        });
    }

    let alert_button = gtk::Button::builder()
        .label("Alerts (0)")
        .css_classes(["flat", "surface-button"])
        .build();
    let mark_all_read_button = gtk::Button::builder()
        .label("Mark All Read")
        .css_classes(["flat", "surface-button"])
        .build();
    let alert_list = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    let alert_scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .min_content_width(320)
        .build();
    alert_scroller.set_child(Some(&alert_list));
    let alert_sidebar = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_start(12)
        .css_classes(["config-panel"])
        .build();
    alert_sidebar.append(
        &gtk::Label::builder()
            .label("Alert Center")
            .halign(gtk::Align::Start)
            .css_classes(["card-title"])
            .build(),
    );
    alert_sidebar.append(&mark_all_read_button);
    alert_sidebar.append(&alert_scroller);
    let alert_revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideLeft)
        .reveal_child(false)
        .build();
    alert_revealer.set_child(Some(&alert_sidebar));
    {
        let alert_revealer = alert_revealer.clone();
        alert_button.connect_clicked(move |_| {
            alert_revealer.set_reveal_child(!alert_revealer.reveals_child());
        });
    }
    {
        let alert_store = alert_store.clone();
        mark_all_read_button.connect_clicked(move |_| {
            alert_store.mark_all_read();
        });
    }
    bind_alert_ui(&runtime, &alert_store, &alert_button, &alert_list);

    summary.append(&name_label);
    summary.append(&alert_button);
    summary.append(&broadcast_state);
    summary.append(&broadcast_selector);
    summary.append(&broadcast_entry);
    summary.append(&broadcast_button);
    summary.append(&add_web_tile_button);
    summary.append(&url_entry);
    summary.append(&url_reload_button);
    summary.append(&runbook_selector);
    summary.append(&runbook_button);
    summary.append(&path_label);
    shell.append(&summary);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .hexpand(true)
        .vexpand(true)
        .build();
    content.append(&layout_host);
    content.append(&alert_revealer);
    shell.append(&content);

    WorkspaceView {
        widget: shell.upcast(),
        runtime,
    }
}

fn bind_alert_ui(
    runtime: &WorkspaceRuntime,
    alert_store: &AlertStore,
    alert_button: &gtk::Button,
    alert_list: &gtk::Box,
) {
    let alert_button = alert_button.clone();
    let alert_list = alert_list.clone();
    let runtime = runtime.clone();
    let alert_store_for_refresh = alert_store.clone();
    let refresh = Rc::new(move || {
        alert_button.set_label(&format!(
            "Alerts ({})",
            alert_store_for_refresh.unread_count()
        ));
        while let Some(child) = alert_list.first_child() {
            alert_list.remove(&child);
        }

        for alert in alert_store_for_refresh.snapshot().into_iter().rev() {
            let row = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(6)
                .css_classes(["tile-editor-row"])
                .build();
            let title = gtk::Label::builder()
                .label(&alert.title)
                .halign(gtk::Align::Start)
                .wrap(true)
                .css_classes(["card-title"])
                .build();
            row.append(&title);
            let detail = gtk::Label::builder()
                .label(if alert.detail.trim().is_empty() {
                    "No detail available."
                } else {
                    alert.detail.as_str()
                })
                .halign(gtk::Align::Start)
                .wrap(true)
                .css_classes(["field-hint"])
                .build();
            row.append(&detail);
            let actions = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(6)
                .build();
            if let Some(pane_id) = alert.pane_id.clone() {
                let jump_button = gtk::Button::builder()
                    .label("Jump")
                    .css_classes(["flat", "surface-button"])
                    .build();
                let runtime_for_jump = runtime.clone();
                let alert_store = alert_store_for_refresh.clone();
                let alert_id = alert.id;
                let pane_id_for_jump = pane_id.clone();
                jump_button.connect_clicked(move |_| {
                    runtime_for_jump.focus_tile(&pane_id_for_jump);
                    alert_store.mark_read(alert_id);
                });
                actions.append(&jump_button);

                if alert.allows_reconnect {
                    let reconnect_button = gtk::Button::builder()
                        .label("Reconnect")
                        .css_classes(["flat", "surface-button"])
                        .build();
                    let runtime_for_reconnect = runtime.clone();
                    let alert_store = alert_store_for_refresh.clone();
                    let alert_id = alert.id;
                    let pane_id_for_reconnect = pane_id.clone();
                    reconnect_button.connect_clicked(move |_| {
                        let _ = runtime_for_reconnect.reconnect_tile(&pane_id_for_reconnect);
                        alert_store.mark_read(alert_id);
                    });
                    actions.append(&reconnect_button);
                }
            }
            let mark_read_button = gtk::Button::builder()
                .label(if alert.unread { "Mark Read" } else { "Read" })
                .css_classes(["flat", "surface-button"])
                .sensitive(alert.unread)
                .build();
            let alert_store = alert_store_for_refresh.clone();
            let alert_id = alert.id;
            mark_read_button.connect_clicked(move |_| {
                alert_store.mark_read(alert_id);
            });
            actions.append(&mark_read_button);
            row.append(&actions);
            alert_list.append(&row);
        }
    });
    alert_store.subscribe(refresh.clone());
    refresh();
}

fn install_tile_alert_hooks(
    session: &TerminalSession,
    tile: &crate::model::layout::TileSpec,
    alert_store: &AlertStore,
    max_reconnect_attempts: u32,
) {
    let terminal = session.widget();
    let last_helper_signature = Rc::new(RefCell::new(String::new()));
    {
        let session = session.clone();
        let alert_store = alert_store.clone();
        let tile = tile.clone();
        let last_helper_signature = last_helper_signature.clone();
        terminal.connect_contents_changed(move |_| {
            let recent = session.recent_output(48);
            let matches = scan_output(&tile.output_helpers, &recent);
            let (summary, severity) = helper_summary_text(&matches);
            let signature = format!("{}::{:?}", summary, severity);
            if matches.is_empty() || *last_helper_signature.borrow() == signature {
                return;
            }
            *last_helper_signature.borrow_mut() = signature;
            let mut alert = AlertEventInput::new(
                AlertSourceKind::OutputHelper,
                match severity.unwrap_or(crate::model::assets::OutputSeverity::Info) {
                    crate::model::assets::OutputSeverity::Info => AlertSeverity::Info,
                    crate::model::assets::OutputSeverity::Warning => AlertSeverity::Warning,
                    crate::model::assets::OutputSeverity::Error => AlertSeverity::Error,
                },
                format!("{}: {}", tile.title, summary),
            );
            alert.detail = recent;
            alert.pane_id = Some(tile.id.clone());
            alert.allows_reconnect = true;
            alert_store.push(alert);
        });
    }
    {
        let session = session.clone();
        let alert_store = alert_store.clone();
        let tile = tile.clone();
        terminal.connect_child_exited(move |_, status| {
            let detail = session.recent_transcript(40);
            let mut alert = AlertEventInput::new(
                AlertSourceKind::PaneExit,
                if status == 0 {
                    AlertSeverity::Info
                } else {
                    AlertSeverity::Warning
                },
                format!("{} exited with status {}", tile.title, status),
            );
            alert.detail = detail;
            alert.pane_id = Some(tile.id.clone());
            alert_store.push(alert);
            if should_auto_reconnect(&tile, &session, status, max_reconnect_attempts) {
                let attempt = session.register_auto_reconnect_attempt();
                let delay = reconnect_delay_seconds(attempt.into());
                let session = session.clone();
                let alert_store = alert_store.clone();
                let tile = tile.clone();
                gtk::glib::timeout_add_seconds_local_once(delay, move || {
                    match session.reconnect() {
                        Ok(_) => {
                            let mut reconnect_alert = AlertEventInput::new(
                                AlertSourceKind::Reconnect,
                                AlertSeverity::Info,
                                format!("{} reconnect scheduled", tile.title),
                            );
                            reconnect_alert.detail =
                                format!("Attempt {} ran after {} second(s).", attempt, delay);
                            reconnect_alert.pane_id = Some(tile.id.clone());
                            reconnect_alert.allows_reconnect = true;
                            alert_store.push(reconnect_alert);
                        }
                        Err(error) => {
                            let mut reconnect_alert = AlertEventInput::new(
                                AlertSourceKind::Reconnect,
                                AlertSeverity::Error,
                                format!("{} reconnect failed", tile.title),
                            );
                            reconnect_alert.detail = error;
                            reconnect_alert.pane_id = Some(tile.id.clone());
                            reconnect_alert.allows_reconnect = true;
                            alert_store.push(reconnect_alert);
                        }
                    }
                });
            }
        });
    }
}

fn should_auto_reconnect(
    tile: &crate::model::layout::TileSpec,
    session: &TerminalSession,
    status: i32,
    max_attempts: u32,
) -> bool {
    if session.termination_requested()
        || u32::from(session.auto_reconnect_attempts()) >= max_attempts
    {
        return false;
    }
    match tile.reconnect_policy {
        crate::model::layout::ReconnectPolicy::Manual => false,
        crate::model::layout::ReconnectPolicy::OnAbnormalExit => status != 0,
        crate::model::layout::ReconnectPolicy::Always => true,
    }
}

fn present_runbook_dialog(
    button: &gtk::Button,
    runbook: &Runbook,
    runtime: &WorkspaceRuntime,
    alert_store: &AlertStore,
    broadcast_state: &gtk::Label,
) {
    if runbook.variables.is_empty()
        && runbook.confirm_policy == crate::model::assets::RunbookConfirmPolicy::Never
    {
        execute_runbook(
            runbook,
            &HashMap::new(),
            runtime,
            alert_store,
            broadcast_state,
        );
        return;
    }

    let Some(window) = button
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    else {
        return;
    };
    let dialog = gtk::Dialog::builder()
        .modal(true)
        .transient_for(&window)
        .title(format!("Run {}", runbook.name))
        .build();
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Run", gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Accept);
    let area = dialog.content_area();
    area.set_spacing(12);
    area.set_margin_top(16);
    area.set_margin_bottom(16);
    area.set_margin_start(16);
    area.set_margin_end(16);
    area.append(
        &gtk::Label::builder()
            .label(if runbook.description.trim().is_empty() {
                format!(
                    "Target: {}  •  Steps: {}  •  {}",
                    runbook.target.label(),
                    runbook.steps.len(),
                    runbook.confirm_policy.label()
                )
            } else {
                format!(
                    "{}\nTarget: {}  •  Steps: {}  •  {}",
                    runbook.description,
                    runbook.target.label(),
                    runbook.steps.len(),
                    runbook.confirm_policy.label()
                )
            })
            .wrap(true)
            .halign(gtk::Align::Start)
            .build(),
    );
    let entries = runbook
        .variables
        .iter()
        .map(|variable| {
            let entry = gtk::Entry::builder()
                .placeholder_text(&variable.label)
                .text(variable.default_value.clone().unwrap_or_default())
                .build();
            area.append(
                &gtk::Label::builder()
                    .label(&variable.label)
                    .halign(gtk::Align::Start)
                    .build(),
            );
            area.append(&entry);
            (variable.id.clone(), entry)
        })
        .collect::<Vec<_>>();
    let preview = runbook
        .steps
        .iter()
        .map(|step| step.command.clone())
        .collect::<Vec<_>>()
        .join("\n");
    area.append(
        &gtk::Label::builder()
            .label(format!("Preview:\n{preview}"))
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint"])
            .build(),
    );

    let runtime = runtime.clone();
    let runbook = runbook.clone();
    let alert_store = alert_store.clone();
    let broadcast_state = broadcast_state.clone();
    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            let variables = entries
                .iter()
                .map(|(id, entry)| (id.clone(), entry.text().to_string()))
                .collect::<HashMap<_, _>>();
            execute_runbook(
                &runbook,
                &variables,
                &runtime,
                &alert_store,
                &broadcast_state,
            );
        }
        dialog.close();
    });
    dialog.present();
}

fn execute_runbook(
    runbook: &Runbook,
    variables: &HashMap<String, String>,
    runtime: &WorkspaceRuntime,
    alert_store: &AlertStore,
    broadcast_state: &gtk::Label,
) {
    match resolve_runbook(runbook, variables, &runtime.tile_specs()) {
        Ok(resolved) => {
            let sent = runtime.run_runbook(&resolved);
            broadcast_state.set_text(&format!("{}  •  sent to {}", resolved.target_label, sent));
            let mut alert = AlertEventInput::new(
                AlertSourceKind::Runbook,
                AlertSeverity::Info,
                format!("Runbook '{}' executed", runbook.name),
            );
            alert.detail = format!(
                "Targeted {} pane(s) with {} step(s).",
                resolved.matching_tile_ids.len(),
                resolved.commands.len()
            );
            alert_store.push(alert);
        }
        Err(error) => {
            let mut alert = AlertEventInput::new(
                AlertSourceKind::Runbook,
                AlertSeverity::Error,
                format!("Runbook '{}' failed", runbook.name),
            );
            alert.detail = error.clone();
            alert_store.push(alert);
            broadcast_state.set_text(&error);
        }
    }
}

fn remount_tiles(slots: &[gtk::Box], tiles: &[WorkspaceTile]) {
    for slot in slots {
        while let Some(child) = slot.first_child() {
            slot.remove(&child);
        }
    }

    detach_tile_widgets(tiles.iter());

    for (slot, tile) in slots.iter().zip(tiles.iter()) {
        slot.append(&tile.widget);
    }
}

fn detach_tile_widgets<'a, I>(tiles: I)
where
    I: IntoIterator<Item = &'a WorkspaceTile>,
{
    for tile in tiles {
        let Some(parent) = tile.widget.parent() else {
            continue;
        };

        if let Ok(parent_box) = parent.clone().downcast::<gtk::Box>() {
            parent_box.remove(&tile.widget);
            continue;
        }

        if let Ok(parent_paned) = parent.downcast::<gtk::Paned>() {
            if parent_paned
                .start_child()
                .as_ref()
                .is_some_and(|child| child == &tile.widget)
            {
                parent_paned.set_start_child(Option::<&gtk::Widget>::None);
            }
            if parent_paned
                .end_child()
                .as_ref()
                .is_some_and(|child| child == &tile.widget)
            {
                parent_paned.set_end_child(Option::<&gtk::Widget>::None);
            }
        }
    }
}
