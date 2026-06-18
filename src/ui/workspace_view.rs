use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use vte4::prelude::TerminalExt;
use webkit6::prelude::WebViewExt;

use crate::logging;
use crate::model::assets::{Runbook, TemplateVariableValues, WorkspaceAssets};
use crate::model::layout::{DEFAULT_WEB_URL, LayoutNode, SplitAxis, TileKind, normalize_web_url};
use crate::model::preset::{ApplicationDensity, WorkspacePreset};
use crate::services::alerts::{AlertEventInput, AlertSeverity, AlertSourceKind, AlertStore};
use crate::services::broadcast::{
    BroadcastTarget, quick_send_detail, quick_send_payload, saved_groups_for_tiles,
    sent_status_label, target_from_selector_id,
};
use crate::services::layout_editor::{
    close_tile as close_layout_tile, split_web_tile, update_split_ratio,
};
use crate::services::output_helpers::{CompiledOutputHelpers, helper_summary_text};
use crate::services::runbooks::{ResolvedRunbook, resolve_runbook};
use crate::services::stats::StatsRecorder;
use crate::storage::session_store::SavedTerminalHistory;
use crate::terminal::session::TerminalSession;
use crate::ui::icons::name as icon_name;
use crate::ui::runbook_controls;
use crate::ui::runbook_dialog;
use crate::ui::workspace_alerts::{self, AlertRowAction, WorkspaceAlertListInput};
use crate::ui::workspace_chrome::{
    WorkspaceSummaryInput, build_workspace_alert_revealer, build_workspace_alert_sidebar_chrome,
    build_workspace_content_chrome, build_workspace_shell_chrome, build_workspace_summary_chrome,
};
use crate::ui::workspace_navigation;
use crate::ui::workspace_tile_state;
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
    shutdown_flag: Option<Rc<Cell<bool>>>,
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
    assets: RefCell<WorkspaceAssets>,
    use_dark_palette: bool,
    density: ApplicationDensity,
    zoom_steps: i32,
    max_reconnect_attempts: u32,
    restored_terminal_history: HashMap<String, Vec<String>>,
    stats: StatsRecorder,
    path_label: gtk::Label,
    url_entry: gtk::Entry,
    url_reload_button: gtk::Button,
    runbook_selector: gtk::ComboBoxText,
    runbook_button: gtk::Button,
    focused_tile_id: RefCell<Option<String>>,
    focused_web_tile_id: RefCell<Option<String>>,
}

#[derive(Clone)]
pub struct WorkspaceRuntime {
    inner: Rc<WorkspaceRuntimeInner>,
}

impl WorkspaceRuntime {
    pub fn current_assets(&self) -> WorkspaceAssets {
        self.inner.assets.borrow().clone()
    }

    pub fn update_assets(&self, assets: WorkspaceAssets) {
        *self.inner.assets.borrow_mut() = assets;
        self.sync_runbook_controls();
    }

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

    pub fn apply_terminal_history_lines(&self, _lines: u32) {
        // Saved/restored terminal history is captured on save. It must not
        // resize live in-session scrollback, because `0` means "do not save
        // history" rather than "disable normal terminal scrollback".
    }

    pub fn capture_terminal_histories(&self, max_lines: usize) -> Vec<SavedTerminalHistory> {
        if max_lines == 0 {
            return Vec::new();
        }

        self.inner
            .tiles
            .borrow()
            .iter()
            .filter_map(|tile| {
                let session = tile.session.as_ref()?;
                let lines = session.capture_terminal_history(max_lines);
                (!lines.is_empty()).then(|| SavedTerminalHistory {
                    tile_id: tile.tile.id.clone(),
                    lines,
                })
            })
            .collect()
    }

    pub fn reflow_layout(&self) {
        let layout = self.inner.layout.borrow().clone();
        {
            let tiles = self.inner.tiles.borrow();
            detach_tile_widgets(tiles.iter());
        }
        self.replace_layout_shell(&layout);
        {
            let tiles = self.inner.tiles.borrow();
            remount_tiles(&self.inner.slots.borrow(), &tiles);
        }
        self.sync_active_tile_styles();
        self.refresh_navigation_controls();
        self.inner.layout_host.queue_resize();
    }

    pub fn terminate_all(&self, reason: &str) {
        let resources = {
            let tiles = self.inner.tiles.borrow();
            tiles
                .iter()
                .map(|tile| {
                    (
                        tile.tile.id.clone(),
                        tile.session.clone(),
                        tile.web_view.clone(),
                        tile.refresh_source_id.clone(),
                        tile.shutdown_flag.clone(),
                    )
                })
                .collect::<Vec<_>>()
        };

        for (tile_id, session, web_view, refresh_source_id, shutdown_flag) in resources {
            shutdown_tile_resources(
                &tile_id,
                session.as_ref(),
                web_view.as_ref(),
                refresh_source_id.as_ref(),
                shutdown_flag.as_ref(),
                reason,
            );
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
            if target.includes(&tile.tile)
                && let Some(session) = &tile.session
                && session.send_text(text)
            {
                sent += 1;
            }
        }
        sent
    }

    #[allow(dead_code)]
    pub fn send_text_to_focused_terminal(&self, text: &str) -> bool {
        let focused_tile_id = self.inner.focused_tile_id.borrow().clone();
        let tiles = self.inner.tiles.borrow();
        let focused_terminal = focused_tile_id
            .as_deref()
            .and_then(|tile_id| tiles.iter().find(|tile| tile.tile.id == tile_id))
            .and_then(|tile| tile.session.as_ref());

        if let Some(session) = focused_terminal {
            session.send_text(text)
        } else {
            false
        }
    }

    #[allow(dead_code)]
    pub fn focused_terminal_available(&self) -> bool {
        let focused_tile_id = self.inner.focused_tile_id.borrow().clone();
        self.inner.tiles.borrow().iter().any(|tile| {
            focused_tile_id.as_deref() == Some(tile.tile.id.as_str()) && tile.session.is_some()
        })
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
            shutdown_tile_resources(
                &removed_tile.tile.id,
                removed_tile.session.as_ref(),
                removed_tile.web_view.as_ref(),
                removed_tile.refresh_source_id.as_ref(),
                removed_tile.shutdown_flag.as_ref(),
                "tile closed",
            );
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
        let normalized_url = normalize_web_url(url);

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
        let normalized_url = normalize_web_url(url);

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

    fn sync_runbook_controls(&self) {
        let assets = self.current_assets();
        let selected_id = self
            .inner
            .runbook_selector
            .active_id()
            .map(|value| value.to_string())
            .unwrap_or_default();

        runbook_controls::sync_runbook_selector(
            &self.inner.runbook_selector,
            &self.inner.runbook_button,
            &assets.runbooks,
            Some(&selected_id),
        );
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
                        .unwrap_or_else(|| DEFAULT_WEB_URL.into()),
                    tile.tile.auto_refresh_seconds,
                )
            })
    }

    pub fn add_web_tile(&self) -> Option<String> {
        let initial_url = if self.has_web_tiles() {
            self.inner.url_entry.text().to_string()
        } else {
            String::new()
        };
        let target_tile_id = self.inner.focused_tile_id.borrow().clone().or_else(|| {
            self.inner
                .tiles
                .borrow()
                .first()
                .map(|tile| tile.tile.id.clone())
        })?;

        let current_layout = self.inner.layout.borrow().clone();
        let (next_layout, new_tile_id) = split_web_tile(
            &current_layout,
            &target_tile_id,
            SplitAxis::Horizontal,
            &initial_url,
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
        self.set_focused_tile(Some(new_tile_id.clone()), true);
        self.focus_tile(&new_tile_id);
        (self.inner.on_layout_changed)(next_layout);

        Some(new_tile_id)
    }

    fn set_tiles(&self, mut tiles: Vec<WorkspaceTile>) {
        self.bind_tile_handlers(&mut tiles);
        remount_tiles(&self.inner.slots.borrow(), &tiles);
        *self.inner.tiles.borrow_mut() = tiles;
        self.sync_tile_close_buttons();
        self.sync_active_tile_styles();
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
        let assets = self.current_assets();

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
                    &assets,
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
                    shutdown_flag: Some(web.shutdown_flag),
                    close_button: web.close_button,
                    handlers_bound: false,
                }
            }
            TileKind::Terminal => {
                let snippet_provider = {
                    let runtime = self.clone();
                    Rc::new(move || runtime.current_assets().snippets)
                };
                let tile_view = tile_view::build(
                    tile,
                    &self.inner.workspace_root,
                    &assets,
                    self.inner.use_dark_palette,
                    self.inner.density,
                    self.inner.zoom_steps,
                    self.inner
                        .restored_terminal_history
                        .get(&tile.id)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]),
                    snippet_provider,
                    on_swap,
                    on_close,
                    can_close,
                    self.inner.stats.clone(),
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
                    shutdown_flag: None,
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
                let shutdown_flag = tile.shutdown_flag.clone();
                web_view.connect_uri_notify(move |wv| {
                    if shutdown_flag.as_ref().is_some_and(|flag| flag.get()) {
                        return;
                    }
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
        let focused_web_tile = self.inner.focused_web_tile_id.borrow().clone();
        let current_url = focused_web_tile
            .as_deref()
            .and_then(|tile_id| self.web_tile_uri(tile_id));
        workspace_navigation::sync_web_navigation_controls(
            &self.inner.path_label,
            &self.inner.url_entry,
            &self.inner.url_reload_button,
            has_web_tiles,
            current_url.as_deref(),
            focused_web_tile.is_some(),
        );
    }

    fn set_focused_tile(&self, tile_id: Option<String>, is_web: bool) {
        *self.inner.focused_tile_id.borrow_mut() = tile_id.clone();
        *self.inner.focused_web_tile_id.borrow_mut() = if is_web { tile_id } else { None };
        self.sync_active_tile_styles();
        self.refresh_navigation_controls();
    }

    fn sync_active_tile_styles(&self) {
        let focused_tile_id = self.inner.focused_tile_id.borrow().clone();
        for tile in self.inner.tiles.borrow().iter() {
            workspace_tile_state::set_tile_active_class(
                &tile.widget,
                focused_tile_id.as_deref() == Some(tile.tile.id.as_str()),
            );
        }
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
        let next_layout = {
            let mut layout = self.inner.layout.borrow_mut();
            let Some(tile) = layout.tile_spec_mut_by_id(tile_id) else {
                return false;
            };
            update(tile);
            layout.clone()
        };
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
        layout_shell.widget.set_size_request(0, 0);
        layout_shell.widget.set_overflow(gtk::Overflow::Hidden);
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

fn shutdown_tile_resources(
    tile_id: &str,
    session: Option<&TerminalSession>,
    web_view: Option<&webkit6::WebView>,
    refresh_source_id: Option<&Rc<RefCell<Option<glib::SourceId>>>>,
    shutdown_flag: Option<&Rc<Cell<bool>>>,
    reason: &str,
) {
    clear_web_refresh_timer(refresh_source_id);

    if let Some(session) = session {
        session.terminate(reason);
    }

    let Some(web_view) = web_view else {
        return;
    };

    if let Some(shutdown_flag) = shutdown_flag {
        shutdown_flag.set(true);
    }

    logging::info(format!(
        "shutting down web tile {} reason='{}'",
        tile_id, reason
    ));
    web_view.stop_loading();
    web_view.load_uri("about:blank");
}

pub struct WorkspaceView {
    pub widget: gtk::Widget,
    pub runtime: WorkspaceRuntime,
}

#[allow(clippy::too_many_arguments)]
pub fn build_with_layout_change_handler(
    preset: &WorkspacePreset,
    workspace_root: &Path,
    assets: &WorkspaceAssets,
    use_dark_palette: bool,
    zoom_steps: i32,
    max_reconnect_attempts: u32,
    _terminal_history_lines: u32,
    restored_terminal_history: Vec<SavedTerminalHistory>,
    stats: StatsRecorder,
    on_layout_changed: Rc<dyn Fn(LayoutNode)>,
) -> WorkspaceView {
    let layout_state = Rc::new(RefCell::new(preset.layout.clone()));

    let shell = build_workspace_shell_chrome();

    let summary = build_workspace_summary_chrome(WorkspaceSummaryInput {
        name: &preset.name,
        path: workspace_root.display().to_string(),
        pane_groups: saved_groups_for_tiles(&preset.layout.tile_specs()),
        controls_sensitive: true,
    });
    let url_entry = summary.url_entry.clone();
    let url_reload_button = summary.url_reload_button.clone();
    let runbook_selector = summary.runbook_selector.clone();
    let runbook_button = summary.runbook_button.clone();
    let add_web_tile_button = summary.add_web_tile_button.clone();
    let broadcast_state = summary.broadcast_state.clone();
    let broadcast_selector = summary.broadcast_selector.clone();
    let broadcast_entry = summary.broadcast_entry.clone();
    let broadcast_button = summary.broadcast_button.clone();
    let alert_button = summary.alert_button.clone();
    let path_label = summary.path_label.clone();
    let layout_host = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .hexpand(true)
        .vexpand(true)
        .build();
    layout_host.set_size_request(0, 0);
    layout_host.set_overflow(gtk::Overflow::Hidden);

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
            assets: RefCell::new(assets.clone()),
            use_dark_palette,
            density: preset.density,
            zoom_steps,
            max_reconnect_attempts,
            restored_terminal_history: restored_terminal_history
                .into_iter()
                .map(|history| (history.tile_id, history.lines))
                .collect(),
            stats,
            path_label: path_label.clone(),
            url_entry: url_entry.clone(),
            url_reload_button: url_reload_button.clone(),
            runbook_selector: runbook_selector.clone(),
            runbook_button: runbook_button.clone(),
            focused_tile_id: RefCell::new(None),
            focused_web_tile_id: RefCell::new(None),
        }),
    };
    runtime.rebuild_from_layout();
    runtime.sync_runbook_controls();

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

    {
        let runtime = runtime.clone();
        url_reload_button.connect_clicked(move |_| {
            if let Some(tile_id) = runtime.current_focused_web_tile() {
                runtime.reload_web_tile(&tile_id);
            }
        });
    }

    {
        let runtime = runtime.clone();
        add_web_tile_button.connect_clicked(move |_| {
            let _ = runtime.add_web_tile();
        });
    }

    let broadcast_target = Rc::new(RefCell::new(BroadcastTarget::Off));

    {
        let broadcast_target = broadcast_target.clone();
        let broadcast_state = broadcast_state.clone();
        broadcast_selector.connect_changed(move |combo| {
            let next_target = target_from_selector_id(combo.active_id().as_deref());
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
            let Some(payload) = quick_send_payload(&broadcast_entry.text()) else {
                return;
            };
            let sent = runtime.send_text_to_target(&target, &payload);
            broadcast_state.set_text(&sent_status_label(&target.label(), sent));
            alert_store.push(AlertEventInput {
                source: AlertSourceKind::Runbook,
                severity: AlertSeverity::Info,
                title: "Quick send executed".into(),
                detail: quick_send_detail(sent),
                pane_id: None,
                allows_reconnect: false,
            });
        });
    }

    {
        let runtime = runtime.clone();
        let alert_store = alert_store.clone();
        let runbook_selector = runbook_selector.clone();
        let broadcast_state = broadcast_state.clone();
        runbook_button.connect_clicked(move |button| {
            let Some(runbook_id) = runbook_selector.active_id() else {
                return;
            };
            if runbook_id.is_empty() {
                return;
            }
            let assets = runtime.current_assets();
            let Some(runbook) = assets
                .runbooks
                .iter()
                .find(|runbook| runbook.id == runbook_id)
            else {
                return;
            };
            present_runbook_dialog(button, runbook, &runtime, &alert_store, &broadcast_state);
        });
    }

    let alert_sidebar = build_workspace_alert_sidebar_chrome(true);
    let mark_all_read_button = alert_sidebar.mark_all_read_button.clone();
    let alert_list = alert_sidebar.alert_list.clone();
    let alert_revealer = build_workspace_alert_revealer(&alert_sidebar.widget);
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

    shell.append(&summary.widget);
    shell.append(&build_workspace_content_chrome(
        &layout_host,
        &alert_revealer,
    ));

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
    let runtime = runtime.clone();
    workspace_alerts::bind_alert_list(WorkspaceAlertListInput {
        alert_store: alert_store.clone(),
        alert_button: alert_button.clone(),
        alert_list: alert_list.clone(),
        action_provider: Some(Rc::new(move |alert, alert_store| {
            let Some(pane_id) = alert.pane_id.clone() else {
                return Vec::new();
            };

            let mut actions = Vec::new();
            actions.push(AlertRowAction {
                label: "Jump",
                icon_name: icon_name::OPEN,
                on_activate: Rc::new({
                    let runtime = runtime.clone();
                    let alert_store = alert_store.clone();
                    let pane_id = pane_id.clone();
                    let alert_id = alert.id;
                    move || {
                        runtime.focus_tile(&pane_id);
                        alert_store.mark_read(alert_id);
                    }
                }),
            });

            if alert.allows_reconnect {
                actions.push(AlertRowAction {
                    label: "Reconnect",
                    icon_name: icon_name::RECOVER,
                    on_activate: Rc::new({
                        let runtime = runtime.clone();
                        let alert_store = alert_store.clone();
                        let pane_id = pane_id.clone();
                        let alert_id = alert.id;
                        move || {
                            let _ = runtime.reconnect_tile(&pane_id);
                            alert_store.mark_read(alert_id);
                        }
                    }),
                });
            }

            actions
        })),
    });
}

fn install_tile_alert_hooks(
    session: &TerminalSession,
    tile: &crate::model::layout::TileSpec,
    alert_store: &AlertStore,
    max_reconnect_attempts: u32,
) {
    let terminal = session.widget();
    let output_helpers = CompiledOutputHelpers::new(&tile.output_helpers);
    let last_helper_signature = Rc::new(RefCell::new(String::new()));
    {
        let session = session.clone();
        let alert_store = alert_store.clone();
        let tile = tile.clone();
        let last_helper_signature = last_helper_signature.clone();
        terminal.connect_contents_changed(move |_| {
            let recent = session.recent_output(48);
            let matches = output_helpers.scan(&recent);
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
                session.set_auto_reconnect_pending(true);
                let session = session.clone();
                let alert_store = alert_store.clone();
                let tile = tile.clone();
                gtk::glib::timeout_add_seconds_local_once(delay, move || {
                    if !session.auto_reconnect_pending() {
                        return;
                    }
                    session.set_auto_reconnect_pending(false);
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
    let runtime = runtime.clone();
    let runbook_for_dialog = runbook.clone();
    let runbook_for_execute = runbook_for_dialog.clone();
    let alert_store = alert_store.clone();
    let broadcast_state = broadcast_state.clone();
    runbook_dialog::present(
        button,
        &runbook_for_dialog,
        Rc::new(move |variables| {
            execute_runbook(
                &runbook_for_execute,
                &variables,
                &runtime,
                &alert_store,
                &broadcast_state,
            );
        }),
    );
}

fn execute_runbook(
    runbook: &Runbook,
    variables: &TemplateVariableValues,
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
            let error = error.to_string();
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
