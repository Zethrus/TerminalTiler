use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::glib::types::StaticType;
use gtk::pango;
use gtk::prelude::*;

use crate::model::assets::{CliSnippet, Runbook, TemplateVariableValues, WorkspaceAssets};
use crate::model::layout::{DEFAULT_WEB_URL, SplitAxis, TileKind, TileSpec, normalize_web_url};
use crate::model::preset::ApplicationDensity;
use crate::services::alerts::{AlertEventInput, AlertSeverity, AlertSourceKind, AlertStore};
use crate::services::broadcast::{
    BroadcastTarget, quick_send_detail, quick_send_payload, saved_groups_for_tiles,
    sent_status_label, target_from_selector_id,
};
use crate::services::layout_editor::{
    close_tile, split_tile_with_kind, split_web_tile, update_split_ratio,
};
use crate::services::runbooks::resolve_runbook;
use crate::services::snippets::resolve_snippet;
use crate::storage::session_store::{SavedSession, SavedTab};
use crate::ui::appearance::resolved_theme_uses_dark_palette;
use crate::ui::icons::{self, name as icon_name};
use crate::ui::pane_status::initial_status_snapshot;
use crate::ui::runbook_controls;
use crate::ui::runbook_dialog;
use crate::ui::snippet_popover::{self, SnippetPopoverInput};
use crate::ui::tile_chrome::{
    TERMINAL_HEADER_BADGE_MAX_CHARS, TileHeaderInput, WEB_HEADER_BADGE_MAX_CHARS,
    append_terminal_tile_action_chrome, append_web_tile_action_chrome,
    bind_web_tile_settings_popover, build_terminal_tile_action_chrome, build_tile_frame,
    build_tile_header_chrome, build_tile_shell, build_web_tile_action_chrome, domain_from_url,
    make_shrinkable,
};
use crate::ui::tile_drag::TileDragPayload;
use crate::ui::title_chrome::{TitleTabInput, build_interactive_title_tab};
use crate::ui::workspace_alerts::{self, WorkspaceAlertListInput};
use crate::ui::workspace_chrome::{
    WorkspaceAlertSidebarChrome, WorkspaceSummaryChrome, WorkspaceSummaryInput,
    build_workspace_alert_revealer, build_workspace_alert_sidebar_chrome,
    build_workspace_content_chrome, build_workspace_shell_chrome, build_workspace_summary_chrome,
};
use crate::ui::workspace_navigation;
use crate::ui::workspace_tile_state;

#[derive(Clone)]
pub struct TileRuntimeSurface {
    pub widget: gtk::Widget,
    pub command_sender: Option<Rc<dyn Fn(&str) -> bool>>,
    pub dropped_paths_sender: Option<DroppedPathsSender>,
    pub appearance_applier: Option<Rc<dyn Fn(bool, ApplicationDensity, i32)>>,
    pub url_applier: Option<Rc<dyn Fn(&str)>>,
    pub web_settings_applier: Option<Rc<dyn Fn(&str, Option<u32>)>>,
    pub shutdown: Option<Rc<dyn Fn(&str)>>,
    pub active_process_checker: Option<Rc<dyn Fn() -> bool>>,
    pub terminal_history_provider: Option<Rc<dyn Fn(usize) -> Vec<String>>>,
    pub recovery_binder: Option<TileRuntimeRecoveryBinder>,
}

#[derive(Clone)]
pub struct TileRuntimeRecoveryBinder {
    pub bind: Rc<dyn Fn(&gtk::Box, &gtk::Label, &gtk::Button, &gtk::Label) -> Option<Rc<dyn Fn()>>>,
}

pub type DroppedPathsSender = Rc<dyn Fn(&[PathBuf], Option<&dyn Fn()>) -> bool>;

impl TileRuntimeSurface {
    pub fn widget(widget: gtk::Widget) -> Self {
        Self {
            widget,
            command_sender: None,
            dropped_paths_sender: None,
            appearance_applier: None,
            url_applier: None,
            web_settings_applier: None,
            shutdown: None,
            active_process_checker: None,
            terminal_history_provider: None,
            recovery_binder: None,
        }
    }
}

pub type TileRuntimeFactory =
    Rc<dyn Fn(usize, &TileSpec, &SavedTab, &WorkspaceAssets) -> TileRuntimeSurface>;
pub type SessionChangeHandler = Rc<dyn Fn(SavedSession, &'static str)>;

pub struct DetachedPreviewTab {
    tab: SavedTab,
    runtime_surfaces: HashMap<String, TileRuntimeSurface>,
}

const MIN_TERMINAL_FONT_POINTS: i32 = 7;
const MAX_TERMINAL_FONT_POINTS: i32 = 20;

/// Build a GTK workspace shell that mirrors the Linux workspace chrome without
/// binding to a platform-specific terminal/web runtime.
///
/// Windows uses this as the visual parity surface while its ConPTY/WebView2
/// adapters are being moved behind the shared GTK layout. The widget therefore
/// intentionally reuses Linux CSS classes (`workspace-summary`, `app-tab-*`,
/// `terminal-card`, `terminal-header`, `terminal-frame`, `terminal-surface`,
/// `web-tile-frame`) and the shared `layout_tree` split renderer instead of
/// opening the legacy Win32 workspace host.
pub fn build_session_preview(session: &SavedSession) -> gtk::Widget {
    SessionPreview::new(session, true).widget()
}

#[derive(Clone)]
pub struct SessionPreview {
    shell: gtk::Box,
    session: Rc<RefCell<SavedSession>>,
    assets: Rc<WorkspaceAssets>,
    active_index: Rc<Cell<usize>>,
    show_inline_tab_strip: bool,
    runtime_factory: Option<TileRuntimeFactory>,
    runtime_surfaces: Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    on_session_changed: Option<SessionChangeHandler>,
    alert_store: AlertStore,
}

#[derive(Clone)]
struct PreviewRenderContext {
    shell: gtk::Box,
    session: Rc<RefCell<SavedSession>>,
    assets: Rc<WorkspaceAssets>,
    active_index: Rc<Cell<usize>>,
    show_inline_tab_strip: bool,
    runtime_factory: Option<TileRuntimeFactory>,
    runtime_surfaces: Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    on_session_changed: Option<SessionChangeHandler>,
    alert_store: AlertStore,
}

impl PreviewRenderContext {
    fn rerender(&self) {
        render_session_preview(
            &self.shell,
            &self.session,
            &self.assets,
            &self.active_index,
            self.show_inline_tab_strip,
            self.runtime_factory.clone(),
            self.runtime_surfaces.clone(),
            self.on_session_changed.clone(),
            self.alert_store.clone(),
        );
    }

    fn prune_and_rerender(&self) {
        prune_runtime_surfaces(
            &self.runtime_surfaces,
            &self.session.borrow(),
            "workspace preview layout changed",
        );
        self.rerender();
    }

    fn notify_session_changed(&self, reason: &'static str) {
        if let Some(on_session_changed) = &self.on_session_changed {
            on_session_changed(self.session.borrow().clone(), reason);
        }
    }
}

impl SessionPreview {
    pub fn new(session: &SavedSession, show_inline_tab_strip: bool) -> Self {
        Self::with_assets(session, show_inline_tab_strip, WorkspaceAssets::default())
    }

    pub fn with_assets(
        session: &SavedSession,
        show_inline_tab_strip: bool,
        assets: WorkspaceAssets,
    ) -> Self {
        Self::with_runtime_assets(session, show_inline_tab_strip, assets, None)
    }

    pub fn with_runtime_assets(
        session: &SavedSession,
        show_inline_tab_strip: bool,
        assets: WorkspaceAssets,
        runtime_factory: Option<TileRuntimeFactory>,
    ) -> Self {
        Self::with_runtime_assets_and_change_handler(
            session,
            show_inline_tab_strip,
            assets,
            runtime_factory,
            None,
        )
    }

    pub fn with_runtime_assets_and_change_handler(
        session: &SavedSession,
        show_inline_tab_strip: bool,
        assets: WorkspaceAssets,
        runtime_factory: Option<TileRuntimeFactory>,
        on_session_changed: Option<SessionChangeHandler>,
    ) -> Self {
        Self::with_runtime_parts(
            session,
            show_inline_tab_strip,
            assets,
            runtime_factory,
            on_session_changed,
            Rc::new(RefCell::new(HashMap::new())),
        )
    }

    fn with_runtime_parts(
        session: &SavedSession,
        show_inline_tab_strip: bool,
        assets: WorkspaceAssets,
        runtime_factory: Option<TileRuntimeFactory>,
        on_session_changed: Option<SessionChangeHandler>,
        runtime_surfaces: Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    ) -> Self {
        let session = Rc::new(RefCell::new(session.clone()));
        let assets = Rc::new(assets);
        let initial_active_index = {
            let session = session.borrow();
            session
                .active_tab_index
                .min(session.tabs.len().saturating_sub(1))
        };
        let active_index = Rc::new(Cell::new(initial_active_index));

        let shell = build_workspace_shell_chrome();

        let preview = Self {
            shell,
            session,
            assets,
            active_index,
            show_inline_tab_strip,
            runtime_factory,
            runtime_surfaces,
            on_session_changed,
            alert_store: AlertStore::default(),
        };
        preview.render();
        preview
    }

    pub fn widget(&self) -> gtk::Widget {
        self.shell.clone().upcast()
    }

    pub fn select_tab(&self, next_index: usize) {
        let next_index = {
            let session = self.session.borrow();
            next_index.min(session.tabs.len().saturating_sub(1))
        };
        self.active_index.set(next_index);
        self.session.borrow_mut().active_tab_index = next_index;
        self.notify_session_changed("active workspace preview tab changed");
        self.render();
    }

    pub fn active_index(&self) -> usize {
        self.active_index.get()
    }

    pub fn tab_title(&self, index: usize) -> Option<String> {
        self.session.borrow().tabs.get(index).map(|tab| {
            tab.custom_title
                .as_deref()
                .unwrap_or(tab.preset.name.as_str())
                .to_string()
        })
    }

    pub fn snapshot(&self) -> SavedSession {
        self.session.borrow().clone()
    }

    pub fn snapshot_with_terminal_history(&self, line_limit: usize) -> SavedSession {
        let mut session = self.session.borrow().clone();
        if line_limit == 0 {
            for tab in &mut session.tabs {
                tab.terminal_history.clear();
            }
            return session;
        }

        let surfaces = self.runtime_surfaces.borrow();
        for (index, tab) in session.tabs.iter_mut().enumerate() {
            let histories = tab
                .preset
                .layout
                .tile_specs()
                .into_iter()
                .filter_map(|tile| {
                    let provider = surfaces
                        .get(&runtime_surface_key(index, tab, &tile))?
                        .terminal_history_provider
                        .as_ref()?;
                    let lines = provider(line_limit);
                    (!lines.is_empty()).then(|| {
                        crate::storage::session_store::SavedTerminalHistory {
                            tile_id: tile.id,
                            lines,
                        }
                    })
                })
                .collect::<Vec<_>>();
            tab.terminal_history = histories;
        }
        session
    }

    pub fn runbooks(&self) -> Vec<Runbook> {
        self.assets.runbooks.clone()
    }

    pub fn push_tab(&self, tab: SavedTab) -> usize {
        let next_index = {
            let mut session = self.session.borrow_mut();
            session.tabs.push(tab);
            let next_index = session.tabs.len() - 1;
            session.active_tab_index = next_index;
            next_index
        };
        self.active_index.set(next_index);
        self.notify_session_changed("workspace preview tab opened");
        self.render();
        next_index
    }

    pub fn close_tab(&self, index: usize) -> bool {
        if let Some(key_update) =
            close_tab_in_preview_state(&self.session, &self.active_index, index)
        {
            apply_runtime_surface_key_update(
                &self.runtime_surfaces,
                key_update,
                "workspace preview tab closed",
            );
            self.notify_session_changed("workspace preview tab closed");
            self.render();
            true
        } else {
            false
        }
    }

    pub fn detach_tab_as_preview(
        &self,
        index: usize,
        on_session_changed: Option<SessionChangeHandler>,
    ) -> Option<Self> {
        let (detached_tab, key_update) =
            detach_tab_in_preview_state(&self.session, &self.active_index, index)?;
        let detached_runtime_surfaces =
            detach_runtime_surfaces_for_tab(&self.runtime_surfaces, &detached_tab, key_update);
        self.notify_session_changed("workspace preview tab detached");
        self.render();

        Some(Self::from_detached_tab(
            DetachedPreviewTab {
                tab: detached_tab,
                runtime_surfaces: detached_runtime_surfaces,
            },
            self.assets.as_ref().clone(),
            self.runtime_factory.clone(),
            on_session_changed,
        ))
    }

    pub fn take_single_tab_for_transfer(&self) -> Option<DetachedPreviewTab> {
        let tab = {
            let mut session = self.session.borrow_mut();
            if session.tabs.len() != 1 {
                return None;
            }
            session.active_tab_index = 0;
            session.tabs.pop()?
        };
        self.active_index.set(0);
        let runtime_surfaces = take_runtime_surfaces_for_tab(&self.runtime_surfaces, 0, &tab);
        self.notify_session_changed("detached workspace preview tab transferred");
        self.render();
        Some(DetachedPreviewTab {
            tab,
            runtime_surfaces,
        })
    }

    pub fn push_detached_tab(&self, detached_tab: DetachedPreviewTab) -> usize {
        let DetachedPreviewTab {
            tab,
            runtime_surfaces: mut detached_runtime_surfaces,
        } = detached_tab;
        let next_index = {
            let mut session = self.session.borrow_mut();
            session.tabs.push(tab.clone());
            let next_index = session.tabs.len() - 1;
            session.active_tab_index = next_index;
            next_index
        };
        self.active_index.set(next_index);
        insert_runtime_surfaces_for_tab(
            &self.runtime_surfaces,
            &tab,
            next_index,
            &mut detached_runtime_surfaces,
        );
        self.notify_session_changed("detached workspace preview tab reattached");
        self.render();
        next_index
    }

    pub fn move_tab(&self, index: usize, position: usize) -> bool {
        let Some(key_moves) =
            move_tab_in_preview_state(&self.session, &self.active_index, index, position)
        else {
            return false;
        };
        rekey_runtime_surfaces_after_tab_move(&self.runtime_surfaces, key_moves);
        self.notify_session_changed("workspace preview tabs reordered");
        self.render();
        true
    }

    pub fn tab_has_active_processes(&self, index: usize) -> bool {
        tab_runtime_surfaces(&self.session.borrow(), &self.runtime_surfaces, index)
            .into_iter()
            .filter_map(|surface| surface.active_process_checker)
            .any(|is_active| is_active())
    }

    pub fn rename_tab(&self, index: usize, requested_title: Option<String>) -> bool {
        let mut session = self.session.borrow_mut();
        let Some(tab) = session.tabs.get_mut(index) else {
            return false;
        };
        tab.custom_title = requested_title.and_then(|title| {
            let trimmed = title.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        });
        drop(session);
        self.notify_session_changed("workspace preview tab renamed");
        self.render();
        true
    }

    pub fn focus_next_alert(&self) -> bool {
        let Some(alert) = self
            .alert_store
            .snapshot()
            .into_iter()
            .find(|alert| alert.unread && alert.pane_id.is_some())
        else {
            return false;
        };
        let Some(pane_id) = alert.pane_id.clone() else {
            return false;
        };
        if self.focus_tile(&pane_id) {
            self.alert_store.mark_read(alert.id);
            true
        } else {
            false
        }
    }

    pub fn add_web_tile(&self, initial_url: &str) -> bool {
        if add_web_tile_to_active_session(&self.session, &self.active_index, initial_url) {
            self.prune_runtime_surfaces("workspace preview web tile added");
            self.notify_session_changed("workspace preview web tile added");
            self.render();
            true
        } else {
            false
        }
    }

    pub fn add_terminal_tile(&self) -> bool {
        if add_terminal_tile_to_active_session(&self.session, &self.active_index) {
            self.prune_runtime_surfaces("workspace preview terminal tile added");
            self.notify_session_changed("workspace preview terminal tile added");
            self.render();
            true
        } else {
            false
        }
    }

    pub fn run_runbook(&self, runbook: &Runbook) -> bool {
        let tile_specs = active_tab_tile_specs(&self.session, &self.active_index);
        match resolve_runbook(runbook, &TemplateVariableValues::new(), &tile_specs) {
            Ok(resolved) => {
                let sent = resolved
                    .commands
                    .iter()
                    .map(|command| {
                        send_command_to_active_runtime_surfaces(
                            &self.session,
                            &self.active_index,
                            &self.runtime_surfaces,
                            &resolved.target,
                            command,
                        )
                    })
                    .sum::<usize>();
                let mut alert = AlertEventInput::new(
                    AlertSourceKind::Runbook,
                    AlertSeverity::Info,
                    format!("Runbook '{}' executed", runbook.name),
                );
                alert.detail = format!(
                    "Targeted {} pane(s) with {} step(s); delivered to {} active runtime(s).",
                    resolved.matching_tile_ids.len(),
                    resolved.commands.len(),
                    sent
                );
                self.alert_store.push(alert);
                sent > 0
            }
            Err(error) => {
                let mut alert = AlertEventInput::new(
                    AlertSourceKind::Runbook,
                    AlertSeverity::Error,
                    format!("Runbook '{}' failed", runbook.name),
                );
                alert.detail = error.to_string();
                self.alert_store.push(alert);
                false
            }
        }
    }

    pub fn send_text_to_focused_terminal(&self, text: &str) -> bool {
        let session_ref = self.session.borrow();
        let Some(tab_index) = active_tab_index(&session_ref, self.active_index.get()) else {
            return false;
        };
        let Some(tab) = session_ref.tabs.get(tab_index) else {
            return false;
        };
        let surfaces = self.runtime_surfaces.borrow();
        let terminal_surfaces = tab
            .preset
            .layout
            .tile_specs()
            .into_iter()
            .filter(|tile| tile.tile_kind == TileKind::Terminal)
            .filter_map(|tile| {
                let key = runtime_surface_key(tab_index, tab, &tile);
                surfaces
                    .get(&key)
                    .and_then(|surface| surface.command_sender.as_ref().map(|send| (surface, send)))
            })
            .collect::<Vec<_>>();

        terminal_surfaces
            .iter()
            .find(|(surface, _)| surface.widget.has_focus())
            .or_else(|| {
                if terminal_surfaces.len() == 1 {
                    terminal_surfaces.first()
                } else {
                    None
                }
            })
            .is_some_and(|(_, send)| send(text))
    }

    pub fn focused_terminal_available(&self) -> bool {
        let session_ref = self.session.borrow();
        let Some(tab_index) = active_tab_index(&session_ref, self.active_index.get()) else {
            return false;
        };
        let Some(tab) = session_ref.tabs.get(tab_index) else {
            return false;
        };
        let surfaces = self.runtime_surfaces.borrow();
        let terminal_surfaces = tab
            .preset
            .layout
            .tile_specs()
            .into_iter()
            .filter(|tile| tile.tile_kind == TileKind::Terminal)
            .filter_map(|tile| {
                let key = runtime_surface_key(tab_index, tab, &tile);
                surfaces
                    .get(&key)
                    .filter(|surface| surface.command_sender.is_some())
            })
            .collect::<Vec<_>>();

        terminal_surfaces
            .iter()
            .any(|surface| surface.widget.has_focus())
            || terminal_surfaces.len() == 1
    }

    pub fn cycle_active_density(&self) -> Option<ApplicationDensity> {
        let next_density = {
            let mut session = self.session.borrow_mut();
            let active_index = self
                .active_index
                .get()
                .min(session.tabs.len().saturating_sub(1));
            let tab = session.tabs.get_mut(active_index)?;
            let next_density = tab.preset.density.next();
            tab.terminal_zoom_steps =
                clamp_terminal_zoom_steps(next_density, tab.terminal_zoom_steps);
            tab.preset.density = next_density;
            next_density
        };
        self.notify_session_changed("workspace preview density changed");
        self.render();
        Some(next_density)
    }

    pub fn adjust_active_zoom(&self, delta: i32) -> Option<i32> {
        let next_zoom_steps = {
            let mut session = self.session.borrow_mut();
            let active_index = self
                .active_index
                .get()
                .min(session.tabs.len().saturating_sub(1));
            let tab = session.tabs.get_mut(active_index)?;
            let next_zoom_steps = clamp_terminal_zoom_steps(
                tab.preset.density,
                tab.terminal_zoom_steps.saturating_add(delta),
            );
            if next_zoom_steps == tab.terminal_zoom_steps {
                return None;
            }
            tab.terminal_zoom_steps = next_zoom_steps;
            next_zoom_steps
        };
        self.notify_session_changed("workspace preview zoom changed");
        self.render();
        Some(next_zoom_steps)
    }

    pub fn terminate_all(&self, reason: &str) {
        let shutdowns = self
            .runtime_surfaces
            .borrow()
            .values()
            .filter_map(|surface| surface.shutdown.clone())
            .collect::<Vec<_>>();

        for shutdown in shutdowns {
            shutdown(reason);
        }
    }

    pub fn has_active_processes(&self) -> bool {
        self.runtime_surfaces
            .borrow()
            .values()
            .filter_map(|surface| surface.active_process_checker.as_ref())
            .any(|is_active| is_active())
    }

    fn focus_tile(&self, tile_id: &str) -> bool {
        let session_ref = self.session.borrow();
        let Some(tab_index) = active_tab_index(&session_ref, self.active_index.get()) else {
            return false;
        };
        let Some(tab) = session_ref.tabs.get(tab_index) else {
            return false;
        };
        let Some(tile) = tab
            .preset
            .layout
            .tile_specs()
            .into_iter()
            .find(|tile| tile.id == tile_id)
        else {
            return false;
        };
        let key = runtime_surface_key(tab_index, tab, &tile);
        if let Some(surface) = self.runtime_surfaces.borrow().get(&key) {
            surface.widget.grab_focus();
            true
        } else {
            false
        }
    }

    fn render(&self) {
        render_session_preview(
            &self.shell,
            &self.session,
            &self.assets,
            &self.active_index,
            self.show_inline_tab_strip,
            self.runtime_factory.clone(),
            self.runtime_surfaces.clone(),
            self.on_session_changed.clone(),
            self.alert_store.clone(),
        );
    }

    fn prune_runtime_surfaces(&self, reason: &str) {
        prune_runtime_surfaces(&self.runtime_surfaces, &self.session.borrow(), reason);
    }

    fn notify_session_changed(&self, reason: &'static str) {
        notify_session_changed(&self.on_session_changed, &self.session, reason);
    }

    fn from_detached_tab(
        detached_tab: DetachedPreviewTab,
        assets: WorkspaceAssets,
        runtime_factory: Option<TileRuntimeFactory>,
        on_session_changed: Option<SessionChangeHandler>,
    ) -> Self {
        let DetachedPreviewTab {
            tab,
            runtime_surfaces,
        } = detached_tab;
        let session = SavedSession {
            tabs: vec![tab],
            active_tab_index: 0,
        };
        Self::with_runtime_parts(
            &session,
            false,
            assets,
            runtime_factory,
            on_session_changed,
            Rc::new(RefCell::new(runtime_surfaces)),
        )
    }
}

fn clamp_terminal_zoom_steps(density: ApplicationDensity, zoom_steps: i32) -> i32 {
    let base_points = density.terminal_font_points();
    (base_points + zoom_steps).clamp(MIN_TERMINAL_FONT_POINTS, MAX_TERMINAL_FONT_POINTS)
        - base_points
}

fn close_tab_in_preview_state(
    session: &Rc<RefCell<SavedSession>>,
    active_index: &Rc<Cell<usize>>,
    index: usize,
) -> Option<RuntimeSurfaceKeyUpdate> {
    let mut session = session.borrow_mut();
    if session.tabs.is_empty() {
        return None;
    }

    let before_tabs = session.tabs.clone();
    let removed_index = index.min(session.tabs.len() - 1);
    let removed_tab = session.tabs.remove(removed_index);
    let next_index = if session.tabs.is_empty() {
        0
    } else {
        let current_active_index = active_index.get();
        if current_active_index == removed_index {
            removed_index.min(session.tabs.len() - 1)
        } else if current_active_index > removed_index {
            current_active_index - 1
        } else {
            current_active_index.min(session.tabs.len() - 1)
        }
    };
    session.active_tab_index = next_index;
    active_index.set(next_index);

    Some(RuntimeSurfaceKeyUpdate {
        stale_keys: runtime_surface_keys_for_tab(removed_index, &removed_tab),
        key_moves: runtime_surface_key_moves_after_tab_close(
            &before_tabs,
            &session.tabs,
            removed_index,
        ),
    })
}

fn detach_tab_in_preview_state(
    session: &Rc<RefCell<SavedSession>>,
    active_index: &Rc<Cell<usize>>,
    index: usize,
) -> Option<(SavedTab, RuntimeSurfaceKeyUpdate)> {
    let mut session = session.borrow_mut();
    if session.tabs.len() <= 1 {
        return None;
    }

    let before_tabs = session.tabs.clone();
    let removed_index = index.min(session.tabs.len() - 1);
    let removed_tab = session.tabs.remove(removed_index);
    let current_active_index = active_index.get();
    let next_index = if current_active_index == removed_index {
        removed_index.min(session.tabs.len() - 1)
    } else if current_active_index > removed_index {
        current_active_index - 1
    } else {
        current_active_index.min(session.tabs.len() - 1)
    };
    session.active_tab_index = next_index;
    active_index.set(next_index);

    let key_update = RuntimeSurfaceKeyUpdate {
        stale_keys: runtime_surface_keys_for_tab(removed_index, &removed_tab),
        key_moves: runtime_surface_key_moves_after_tab_close(
            &before_tabs,
            &session.tabs,
            removed_index,
        ),
    };
    Some((removed_tab, key_update))
}

fn move_tab_in_preview_state(
    session: &Rc<RefCell<SavedSession>>,
    active_index: &Rc<Cell<usize>>,
    index: usize,
    position: usize,
) -> Option<Vec<(String, String)>> {
    let mut session = session.borrow_mut();
    if index >= session.tabs.len() {
        return None;
    }

    let before_tabs = session.tabs.clone();
    let tab = session.tabs.remove(index);
    let insert_index = position.min(session.tabs.len());
    session.tabs.insert(insert_index, tab);
    if index == insert_index {
        return None;
    }

    let key_moves =
        runtime_surface_key_moves_for_tab_reorder(&before_tabs, &session.tabs, index, insert_index);
    let next_active_index = moved_tab_position(
        active_index.get().min(before_tabs.len().saturating_sub(1)),
        index,
        insert_index,
    );
    session.active_tab_index = next_active_index;
    active_index.set(next_active_index);
    Some(key_moves)
}

fn moved_tab_position(current_index: usize, from_index: usize, insert_index: usize) -> usize {
    if current_index == from_index {
        insert_index
    } else if from_index < insert_index
        && current_index > from_index
        && current_index <= insert_index
    {
        current_index - 1
    } else if insert_index < from_index
        && current_index >= insert_index
        && current_index < from_index
    {
        current_index + 1
    } else {
        current_index
    }
}

fn render_session_preview(
    shell: &gtk::Box,
    session: &Rc<RefCell<SavedSession>>,
    assets: &Rc<WorkspaceAssets>,
    active_index: &Rc<Cell<usize>>,
    show_inline_tab_strip: bool,
    runtime_factory: Option<TileRuntimeFactory>,
    runtime_surfaces: Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    on_session_changed: Option<SessionChangeHandler>,
    alert_store: AlertStore,
) {
    while let Some(child) = shell.first_child() {
        shell.remove(&child);
    }

    let session_ref = session.borrow();
    let current_index = active_index
        .get()
        .min(session_ref.tabs.len().saturating_sub(1));
    active_index.set(current_index);

    if show_inline_tab_strip && !session_ref.tabs.is_empty() {
        let on_close = {
            let shell = shell.clone();
            let session = session.clone();
            let assets = assets.clone();
            let active_index = active_index.clone();
            let runtime_factory = runtime_factory.clone();
            let runtime_surfaces = runtime_surfaces.clone();
            let on_session_changed = on_session_changed.clone();
            let alert_store = alert_store.clone();
            Rc::new(move |index: usize| {
                if let Some(key_update) = close_tab_in_preview_state(&session, &active_index, index)
                {
                    apply_runtime_surface_key_update(
                        &runtime_surfaces,
                        key_update,
                        "workspace preview tab closed",
                    );
                    notify_session_changed(
                        &on_session_changed,
                        &session,
                        "workspace preview tab closed",
                    );
                    render_session_preview(
                        &shell,
                        &session,
                        &assets,
                        &active_index,
                        true,
                        runtime_factory.clone(),
                        runtime_surfaces.clone(),
                        on_session_changed.clone(),
                        alert_store.clone(),
                    );
                }
            })
        };
        let on_select = {
            let shell = shell.clone();
            let session = session.clone();
            let assets = assets.clone();
            let active_index = active_index.clone();
            let runtime_factory = runtime_factory.clone();
            let runtime_surfaces = runtime_surfaces.clone();
            let on_session_changed = on_session_changed.clone();
            let alert_store = alert_store.clone();
            Rc::new(move |next_index: usize| {
                let next_index = {
                    let session = session.borrow();
                    next_index.min(session.tabs.len().saturating_sub(1))
                };
                active_index.set(next_index);
                session.borrow_mut().active_tab_index = next_index;
                notify_session_changed(
                    &on_session_changed,
                    &session,
                    "active workspace preview tab changed",
                );
                render_session_preview(
                    &shell,
                    &session,
                    &assets,
                    &active_index,
                    true,
                    runtime_factory.clone(),
                    runtime_surfaces.clone(),
                    on_session_changed.clone(),
                    alert_store.clone(),
                );
            })
        };
        shell.append(&build_tab_strip(
            &session_ref,
            current_index,
            on_select,
            on_close,
        ));
    }

    if let Some(tab) = session_ref.tabs.get(current_index) {
        let render_context = PreviewRenderContext {
            shell: shell.clone(),
            session: session.clone(),
            assets: assets.clone(),
            active_index: active_index.clone(),
            show_inline_tab_strip,
            runtime_factory: runtime_factory.clone(),
            runtime_surfaces: runtime_surfaces.clone(),
            on_session_changed: on_session_changed.clone(),
            alert_store: alert_store.clone(),
        };
        let summary = build_workspace_summary(tab, &render_context);
        shell.append(&summary.widget);
        let on_close_tile = {
            let render_context = render_context.clone();
            Rc::new(move |tile_id: String| {
                if close_active_session_tile(
                    &render_context.session,
                    &render_context.active_index,
                    &tile_id,
                ) {
                    render_context.prune_and_rerender();
                    render_context.notify_session_changed("workspace preview tile closed");
                }
            })
        };
        let layout = build_layout(
            current_index,
            tab,
            assets,
            &render_context,
            runtime_factory.as_ref(),
            &runtime_surfaces,
            on_close_tile,
        );
        shell.append(&build_workspace_content_chrome(
            &layout,
            &summary.alert_revealer,
        ));
    } else {
        shell.append(&build_empty_state());
    }
}

pub fn session_shape(session: &SavedSession) -> (usize, usize) {
    let pane_count = session
        .tabs
        .iter()
        .map(|tab| tab.preset.layout.tile_specs().len())
        .sum::<usize>();
    (session.tabs.len(), pane_count)
}

fn runtime_surface_keys(session: &SavedSession) -> Vec<String> {
    session
        .tabs
        .iter()
        .enumerate()
        .flat_map(|(index, tab)| {
            tab.preset
                .layout
                .tile_specs()
                .into_iter()
                .map(|tile| runtime_surface_key(index, tab, &tile))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn runtime_surface_key(tab_index: usize, tab: &SavedTab, tile: &TileSpec) -> String {
    format!(
        "{}::{}::{}::{}",
        tab_index,
        tab.workspace_root.display(),
        tab.preset.id,
        tile.id
    )
}

struct RuntimeSurfaceKeyUpdate {
    stale_keys: Vec<String>,
    key_moves: Vec<(String, String)>,
}

fn runtime_surface_keys_for_tab(tab_index: usize, tab: &SavedTab) -> Vec<String> {
    tab.preset
        .layout
        .tile_specs()
        .into_iter()
        .map(|tile| runtime_surface_key(tab_index, tab, &tile))
        .collect()
}

fn runtime_surface_key_moves_for_tab_index(
    tab: &SavedTab,
    old_index: usize,
    new_index: usize,
) -> Vec<(String, String)> {
    tab.preset
        .layout
        .tile_specs()
        .into_iter()
        .map(|tile| {
            (
                runtime_surface_key(old_index, tab, &tile),
                runtime_surface_key(new_index, tab, &tile),
            )
        })
        .filter(|(old_key, new_key)| old_key != new_key)
        .collect()
}

fn runtime_surface_key_moves_after_tab_close(
    before_tabs: &[SavedTab],
    after_tabs: &[SavedTab],
    removed_index: usize,
) -> Vec<(String, String)> {
    before_tabs
        .iter()
        .enumerate()
        .filter(|(old_index, _)| *old_index != removed_index)
        .filter_map(|(old_index, tab)| {
            let new_index = if old_index > removed_index {
                old_index - 1
            } else {
                old_index
            };
            let moved_tab = after_tabs.get(new_index)?;
            Some(
                tab.preset
                    .layout
                    .tile_specs()
                    .into_iter()
                    .map(move |tile| {
                        (
                            runtime_surface_key(old_index, tab, &tile),
                            runtime_surface_key(new_index, moved_tab, &tile),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .filter(|(old_key, new_key)| old_key != new_key)
        .collect()
}

fn runtime_surface_key_moves_for_tab_reorder(
    before_tabs: &[SavedTab],
    after_tabs: &[SavedTab],
    from_index: usize,
    insert_index: usize,
) -> Vec<(String, String)> {
    before_tabs
        .iter()
        .enumerate()
        .filter_map(|(old_index, tab)| {
            let new_index = moved_tab_position(old_index, from_index, insert_index);
            let moved_tab = after_tabs.get(new_index)?;
            Some(
                tab.preset
                    .layout
                    .tile_specs()
                    .into_iter()
                    .map(move |tile| {
                        (
                            runtime_surface_key(old_index, tab, &tile),
                            runtime_surface_key(new_index, moved_tab, &tile),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .filter(|(old_key, new_key)| old_key != new_key)
        .collect()
}

fn detach_runtime_surfaces_for_tab(
    runtime_surfaces: &Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    detached_tab: &SavedTab,
    key_update: RuntimeSurfaceKeyUpdate,
) -> HashMap<String, TileRuntimeSurface> {
    let RuntimeSurfaceKeyUpdate {
        stale_keys,
        key_moves,
    } = key_update;
    let mut surfaces = runtime_surfaces.borrow_mut();
    let mut detached_surfaces = HashMap::new();
    for (old_key, new_key) in stale_keys
        .into_iter()
        .zip(runtime_surface_keys_for_tab(0, detached_tab))
    {
        if let Some(surface) = surfaces.remove(&old_key) {
            detached_surfaces.insert(new_key, surface);
        }
    }

    let mut moved_surfaces = Vec::new();
    for (old_key, new_key) in key_moves {
        if let Some(surface) = surfaces.remove(&old_key) {
            moved_surfaces.push((new_key, surface));
        }
    }
    for (new_key, surface) in moved_surfaces {
        surfaces.insert(new_key, surface);
    }

    detached_surfaces
}

fn take_runtime_surfaces_for_tab(
    runtime_surfaces: &Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    tab_index: usize,
    tab: &SavedTab,
) -> HashMap<String, TileRuntimeSurface> {
    let mut surfaces = runtime_surfaces.borrow_mut();
    runtime_surface_keys_for_tab(tab_index, tab)
        .into_iter()
        .filter_map(|key| surfaces.remove(&key).map(|surface| (key, surface)))
        .collect()
}

fn insert_runtime_surfaces_for_tab(
    runtime_surfaces: &Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    tab: &SavedTab,
    tab_index: usize,
    detached_runtime_surfaces: &mut HashMap<String, TileRuntimeSurface>,
) {
    let mut surfaces = runtime_surfaces.borrow_mut();
    for (old_key, new_key) in runtime_surface_key_moves_for_tab_index(tab, 0, tab_index) {
        if let Some(surface) = detached_runtime_surfaces.remove(&old_key) {
            surfaces.insert(new_key, surface);
        }
    }
    if tab_index == 0 {
        for (key, surface) in detached_runtime_surfaces.drain() {
            surfaces.insert(key, surface);
        }
    }
}

fn apply_runtime_surface_key_update(
    runtime_surfaces: &Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    key_update: RuntimeSurfaceKeyUpdate,
    reason: &str,
) {
    let stale_surfaces = {
        let mut surfaces = runtime_surfaces.borrow_mut();
        let stale_surfaces = key_update
            .stale_keys
            .into_iter()
            .filter_map(|key| surfaces.remove(&key))
            .collect::<Vec<_>>();

        let mut moved_surfaces = Vec::new();
        for (old_key, new_key) in key_update.key_moves {
            if let Some(surface) = surfaces.remove(&old_key) {
                moved_surfaces.push((new_key, surface));
            }
        }
        for (new_key, surface) in moved_surfaces {
            surfaces.insert(new_key, surface);
        }

        stale_surfaces
    };

    for shutdown in stale_surfaces
        .into_iter()
        .filter_map(|surface| surface.shutdown)
    {
        shutdown(reason);
    }
}

fn rekey_runtime_surfaces_after_tab_move(
    runtime_surfaces: &Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    key_moves: Vec<(String, String)>,
) {
    apply_runtime_surface_key_update(
        runtime_surfaces,
        RuntimeSurfaceKeyUpdate {
            stale_keys: Vec::new(),
            key_moves,
        },
        "workspace preview tabs reordered",
    );
}

fn tab_runtime_surfaces(
    session: &SavedSession,
    runtime_surfaces: &Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    index: usize,
) -> Vec<TileRuntimeSurface> {
    let Some(tab) = session.tabs.get(index) else {
        return Vec::new();
    };
    let surfaces = runtime_surfaces.borrow();
    tab.preset
        .layout
        .tile_specs()
        .into_iter()
        .filter_map(|tile| {
            surfaces
                .get(&runtime_surface_key(index, tab, &tile))
                .cloned()
        })
        .collect()
}

fn detach_from_previous_parent(widget: &gtk::Widget) {
    if let Some(parent) = widget.parent()
        && let Ok(container) = parent.downcast::<gtk::Box>()
    {
        container.remove(widget);
    }
}

fn build_tab_strip(
    session: &SavedSession,
    active_index: usize,
    on_select: Rc<dyn Fn(usize)>,
    on_close: Rc<dyn Fn(usize)>,
) -> gtk::Widget {
    let strip = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .halign(gtk::Align::Start)
        .css_classes(["app-tab-strip"])
        .build();
    make_shrinkable(&strip);

    for (index, tab) in session.tabs.iter().enumerate() {
        strip.append(&build_tab_chip(
            tab,
            index,
            index == active_index,
            on_select.clone(),
            on_close.clone(),
        ));
    }

    let add_button = icons::icon_button(
        icon_name::ADD,
        "New workspace tab",
        &["flat", "app-tab-add"],
    );
    strip.append(&add_button);

    strip.upcast()
}

fn build_tab_chip(
    tab: &SavedTab,
    index: usize,
    active: bool,
    on_select: Rc<dyn Fn(usize)>,
    on_close: Rc<dyn Fn(usize)>,
) -> gtk::Widget {
    let title = tab
        .custom_title
        .as_deref()
        .unwrap_or(tab.preset.name.as_str());
    let chrome = build_interactive_title_tab(TitleTabInput {
        label: title.to_string(),
        tooltip: title.to_string(),
        active,
        close_enabled: true,
        on_select: Some(Rc::new({
            let on_select = on_select.clone();
            move || on_select(index)
        })),
        on_rename: None,
        on_close: Some(Rc::new(move || {
            on_close(index);
        })),
    });

    let shell = chrome.shell;
    shell.set_valign(gtk::Align::End);
    shell.upcast()
}

struct PreviewSummaryChrome {
    widget: gtk::Widget,
    alert_revealer: gtk::Revealer,
}

fn build_workspace_summary(
    tab: &SavedTab,
    render_context: &PreviewRenderContext,
) -> PreviewSummaryChrome {
    let summary = build_workspace_summary_chrome(WorkspaceSummaryInput {
        name: &tab.preset.name,
        path: tab.workspace_root.display().to_string(),
        pane_groups: saved_groups(tab),
        controls_sensitive: true,
    });

    let current_url = first_web_tile_url(tab);
    workspace_navigation::sync_web_navigation_controls(
        &summary.path_label,
        &summary.url_entry,
        &summary.url_reload_button,
        current_url.is_some(),
        current_url.as_deref(),
        current_url.is_some(),
    );

    let alert_store = render_context.alert_store.clone();
    let broadcast_target = Rc::new(RefCell::new(BroadcastTarget::Off));
    {
        let broadcast_target = broadcast_target.clone();
        let broadcast_state = summary.broadcast_state.clone();
        summary.broadcast_selector.connect_changed(move |combo| {
            let next_target = target_from_selector_id(combo.active_id().as_deref());
            broadcast_state.set_text(&next_target.label());
            *broadcast_target.borrow_mut() = next_target;
        });
    }

    let send_broadcast = Rc::new({
        let session = render_context.session.clone();
        let active_index = render_context.active_index.clone();
        let runtime_surfaces = render_context.runtime_surfaces.clone();
        let broadcast_target = broadcast_target.clone();
        let broadcast_entry = summary.broadcast_entry.clone();
        let broadcast_state = summary.broadcast_state.clone();
        let alert_store = alert_store.clone();
        move || {
            let Some(payload) = quick_send_payload(&broadcast_entry.text()) else {
                return;
            };
            let target = broadcast_target.borrow().clone();
            let sent = send_command_to_active_runtime_surfaces(
                &session,
                &active_index,
                &runtime_surfaces,
                &target,
                &payload,
            );
            broadcast_state.set_text(&sent_status_label(&target.label(), sent));
            alert_store.push(AlertEventInput {
                source: AlertSourceKind::Runbook,
                severity: AlertSeverity::Info,
                title: "Quick send executed".into(),
                detail: quick_send_detail(sent),
                pane_id: None,
                allows_reconnect: false,
            });
        }
    });
    {
        let send_broadcast = send_broadcast.clone();
        summary
            .broadcast_button
            .connect_clicked(move |_| send_broadcast());
    }
    {
        let send_broadcast = send_broadcast.clone();
        summary
            .broadcast_entry
            .connect_activate(move |_| send_broadcast());
    }

    {
        let render_context = render_context.clone();
        summary.add_terminal_tile_button.connect_clicked(move |_| {
            if add_terminal_tile_to_active_session(
                &render_context.session,
                &render_context.active_index,
            ) {
                render_context.prune_and_rerender();
                render_context.notify_session_changed("workspace preview terminal tile added");
            }
        });
    }

    {
        let render_context = render_context.clone();
        let url_entry = summary.url_entry.clone();
        summary.add_web_tile_button.connect_clicked(move |_| {
            if add_web_tile_to_active_session(
                &render_context.session,
                &render_context.active_index,
                &url_entry.text(),
            ) {
                render_context.prune_and_rerender();
                render_context.notify_session_changed("workspace preview web tile added");
            }
        });
    }

    let update_web_url = Rc::new({
        let render_context = render_context.clone();
        let url_entry = summary.url_entry.clone();
        move || {
            if update_active_web_tile_url(
                &render_context.session,
                &render_context.active_index,
                &url_entry.text(),
            ) {
                render_context.prune_and_rerender();
                render_context.notify_session_changed("workspace preview web URL changed");
            }
        }
    });
    {
        let update_web_url = update_web_url.clone();
        summary
            .url_entry
            .connect_activate(move |_| update_web_url());
    }
    {
        let update_web_url = update_web_url.clone();
        summary
            .url_reload_button
            .connect_clicked(move |_| update_web_url());
    }

    bind_preview_runbook_controls(&summary, render_context, &alert_store);

    let alert_sidebar = build_workspace_alert_sidebar_chrome(true);
    let alert_revealer = build_workspace_alert_revealer(&alert_sidebar.widget);
    bind_preview_alert_controls(&summary, &alert_sidebar, &alert_revealer, &alert_store);

    PreviewSummaryChrome {
        widget: summary.widget,
        alert_revealer,
    }
}

fn saved_groups(tab: &SavedTab) -> Vec<String> {
    saved_groups_for_tiles(&tab.preset.layout.tile_specs())
}

fn bind_preview_runbook_controls(
    summary: &WorkspaceSummaryChrome,
    render_context: &PreviewRenderContext,
    alert_store: &AlertStore,
) {
    runbook_controls::sync_runbook_selector(
        &summary.runbook_selector,
        &summary.runbook_button,
        &render_context.assets.runbooks,
        None,
    );

    let session = render_context.session.clone();
    let active_index = render_context.active_index.clone();
    let runtime_surfaces = render_context.runtime_surfaces.clone();
    let assets = render_context.assets.clone();
    let runbook_selector = summary.runbook_selector.clone();
    let broadcast_state = summary.broadcast_state.clone();
    let alert_store = alert_store.clone();
    summary.runbook_button.connect_clicked(move |button| {
        let Some(runbook_id) = runbook_selector.active_id() else {
            return;
        };
        if runbook_id.is_empty() {
            return;
        }
        let Some(runbook) = assets
            .runbooks
            .iter()
            .find(|runbook| runbook.id == runbook_id)
        else {
            return;
        };
        present_preview_runbook_dialog(
            button,
            runbook,
            PreviewRunbookContext {
                session: session.clone(),
                active_index: active_index.clone(),
                runtime_surfaces: runtime_surfaces.clone(),
                broadcast_state: broadcast_state.clone(),
                alert_store: alert_store.clone(),
            },
        );
    });
}

#[derive(Clone)]
struct PreviewRunbookContext {
    session: Rc<RefCell<SavedSession>>,
    active_index: Rc<Cell<usize>>,
    runtime_surfaces: Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    broadcast_state: gtk::Label,
    alert_store: AlertStore,
}

fn present_preview_runbook_dialog(
    button: &gtk::Button,
    runbook: &Runbook,
    context: PreviewRunbookContext,
) {
    let runbook_for_dialog = runbook.clone();
    let runbook_for_execute = runbook_for_dialog.clone();
    runbook_dialog::present(
        button,
        &runbook_for_dialog,
        Rc::new(move |variables| {
            execute_preview_runbook(&runbook_for_execute, variables, &context);
        }),
    );
}

fn execute_preview_runbook(
    runbook: &Runbook,
    variables: TemplateVariableValues,
    context: &PreviewRunbookContext,
) {
    let tile_specs = active_tab_tile_specs(&context.session, &context.active_index);
    match resolve_runbook(runbook, &variables, &tile_specs) {
        Ok(resolved) => {
            let sent = resolved
                .commands
                .iter()
                .map(|command| {
                    send_command_to_active_runtime_surfaces(
                        &context.session,
                        &context.active_index,
                        &context.runtime_surfaces,
                        &resolved.target,
                        command,
                    )
                })
                .sum::<usize>();
            context
                .broadcast_state
                .set_text(&format!("{}  •  sent to {}", resolved.target_label, sent));
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
            context.alert_store.push(alert);
        }
        Err(error) => {
            let error = error.to_string();
            let mut alert = AlertEventInput::new(
                AlertSourceKind::Runbook,
                AlertSeverity::Error,
                format!("Runbook '{}' failed", runbook.name),
            );
            alert.detail = error.clone();
            context.alert_store.push(alert);
            context.broadcast_state.set_text(&error);
        }
    }
}

fn bind_preview_alert_controls(
    summary: &WorkspaceSummaryChrome,
    alert_sidebar: &WorkspaceAlertSidebarChrome,
    alert_revealer: &gtk::Revealer,
    alert_store: &AlertStore,
) {
    {
        let alert_revealer = alert_revealer.clone();
        summary.alert_button.connect_clicked(move |_| {
            alert_revealer.set_reveal_child(!alert_revealer.reveals_child());
        });
    }
    {
        let alert_store = alert_store.clone();
        alert_sidebar
            .mark_all_read_button
            .connect_clicked(move |_| {
                alert_store.mark_all_read();
            });
    }

    workspace_alerts::bind_alert_list(WorkspaceAlertListInput {
        alert_store: alert_store.clone(),
        alert_button: summary.alert_button.clone(),
        alert_list: alert_sidebar.alert_list.clone(),
        action_provider: None,
    });
}

fn bind_preview_terminal_snippets(
    snippet_button: &gtk::Button,
    tile: &TileSpec,
    render_context: &PreviewRenderContext,
) {
    let snippet_context = PreviewSnippetContext {
        tile_id: tile.id.clone(),
        session: render_context.session.clone(),
        active_index: render_context.active_index.clone(),
        runtime_surfaces: render_context.runtime_surfaces.clone(),
    };
    let snippets = render_context.assets.snippets.clone();
    snippet_popover::install(
        snippet_button,
        SnippetPopoverInput {
            snippets_provider: Rc::new(move || snippets.clone()),
            before_popup: None,
            execute: Rc::new(move |snippet, variables, _| {
                execute_preview_snippet(snippet, variables, &snippet_context)
            }),
        },
    );
}

#[derive(Clone)]
struct PreviewSnippetContext {
    tile_id: String,
    session: Rc<RefCell<SavedSession>>,
    active_index: Rc<Cell<usize>>,
    runtime_surfaces: Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
}

fn execute_preview_snippet(
    snippet: &CliSnippet,
    variables: TemplateVariableValues,
    context: &PreviewSnippetContext,
) -> Result<(), String> {
    let command = resolve_snippet(snippet, &variables).map_err(|error| error.to_string())?;
    if send_command_to_active_runtime_surface(
        &context.session,
        &context.active_index,
        &context.runtime_surfaces,
        &context.tile_id,
        &command,
    ) {
        Ok(())
    } else {
        Err("This pane is not ready to receive input.".into())
    }
}

fn active_tab_tile_specs(
    session: &Rc<RefCell<SavedSession>>,
    active_index: &Rc<Cell<usize>>,
) -> Vec<TileSpec> {
    let session_ref = session.borrow();
    let Some(tab_index) = active_tab_index(&session_ref, active_index.get()) else {
        return Vec::new();
    };
    session_ref
        .tabs
        .get(tab_index)
        .map(|tab| tab.preset.layout.tile_specs())
        .unwrap_or_default()
}

fn prune_runtime_surfaces(
    runtime_surfaces: &Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    session: &SavedSession,
    reason: &str,
) {
    let live_keys = runtime_surface_keys(session);
    let stale_surfaces = {
        let mut surfaces = runtime_surfaces.borrow_mut();
        let stale_keys = surfaces
            .keys()
            .filter(|key| !live_keys.iter().any(|live_key| live_key == *key))
            .cloned()
            .collect::<Vec<_>>();
        stale_keys
            .into_iter()
            .filter_map(|key| surfaces.remove(&key))
            .collect::<Vec<_>>()
    };

    for shutdown in stale_surfaces
        .into_iter()
        .filter_map(|surface| surface.shutdown)
    {
        shutdown(reason);
    }
}

fn notify_session_changed(
    on_session_changed: &Option<SessionChangeHandler>,
    session: &Rc<RefCell<SavedSession>>,
    reason: &'static str,
) {
    if let Some(on_session_changed) = on_session_changed {
        on_session_changed(session.borrow().clone(), reason);
    }
}

fn active_tab_index(session: &SavedSession, active_index: usize) -> Option<usize> {
    (!session.tabs.is_empty()).then_some(active_index.min(session.tabs.len() - 1))
}

fn add_web_tile_to_active_session(
    session: &Rc<RefCell<SavedSession>>,
    active_index: &Rc<Cell<usize>>,
    initial_url: &str,
) -> bool {
    let mut session_ref = session.borrow_mut();
    let Some(tab_index) = active_tab_index(&session_ref, active_index.get()) else {
        return false;
    };
    let Some(target_tile_id) = session_ref.tabs[tab_index]
        .preset
        .layout
        .tile_specs()
        .first()
        .map(|tile| tile.id.clone())
    else {
        return false;
    };
    let Some((next_layout, _new_tile_id)) = split_web_tile(
        &session_ref.tabs[tab_index].preset.layout,
        &target_tile_id,
        SplitAxis::Horizontal,
        initial_url,
    ) else {
        return false;
    };
    session_ref.tabs[tab_index].preset.layout = next_layout;
    true
}

fn add_terminal_tile_to_active_session(
    session: &Rc<RefCell<SavedSession>>,
    active_index: &Rc<Cell<usize>>,
) -> bool {
    let mut session_ref = session.borrow_mut();
    let Some(tab_index) = active_tab_index(&session_ref, active_index.get()) else {
        return false;
    };
    let Some(target_tile_id) = session_ref.tabs[tab_index]
        .preset
        .layout
        .tile_specs()
        .first()
        .map(|tile| tile.id.clone())
    else {
        return false;
    };
    let Some((next_layout, _new_tile_id)) = split_tile_with_kind(
        &session_ref.tabs[tab_index].preset.layout,
        &target_tile_id,
        SplitAxis::Horizontal,
        false,
        TileKind::Terminal,
    ) else {
        return false;
    };
    session_ref.tabs[tab_index].preset.layout = next_layout;
    true
}

fn swap_active_session_tiles(
    session: &Rc<RefCell<SavedSession>>,
    active_index: &Rc<Cell<usize>>,
    dragged_id: &str,
    target_id: &str,
) -> bool {
    let mut session_ref = session.borrow_mut();
    let Some(tab_index) = active_tab_index(&session_ref, active_index.get()) else {
        return false;
    };
    let Some(next_layout) = session_ref.tabs[tab_index]
        .preset
        .layout
        .swap_tile_positions(dragged_id, target_id)
    else {
        return false;
    };
    session_ref.tabs[tab_index].preset.layout = next_layout;
    true
}

fn update_active_split_ratio(
    session: &Rc<RefCell<SavedSession>>,
    active_index: &Rc<Cell<usize>>,
    split_path: &[bool],
    ratio: f32,
) -> bool {
    let mut session_ref = session.borrow_mut();
    let Some(tab_index) = active_tab_index(&session_ref, active_index.get()) else {
        return false;
    };
    let Some(next_layout) = update_split_ratio(
        &session_ref.tabs[tab_index].preset.layout,
        split_path,
        ratio,
    ) else {
        return false;
    };
    session_ref.tabs[tab_index].preset.layout = next_layout;
    true
}

fn close_active_session_tile(
    session: &Rc<RefCell<SavedSession>>,
    active_index: &Rc<Cell<usize>>,
    tile_id: &str,
) -> bool {
    let mut session_ref = session.borrow_mut();
    let Some(tab_index) = active_tab_index(&session_ref, active_index.get()) else {
        return false;
    };
    let Some(next_layout) = close_tile(&session_ref.tabs[tab_index].preset.layout, tile_id) else {
        return false;
    };
    session_ref.tabs[tab_index].preset.layout = next_layout;
    true
}

fn first_web_tile_url(tab: &SavedTab) -> Option<String> {
    tab.preset
        .layout
        .tile_specs()
        .iter()
        .find(|tile| tile.tile_kind == TileKind::WebView)
        .map(|tile| normalize_web_url(tile.url.as_deref().unwrap_or(DEFAULT_WEB_URL)))
}

fn active_web_tile_settings(
    session: &Rc<RefCell<SavedSession>>,
    active_index: &Rc<Cell<usize>>,
    tile_id: &str,
) -> Option<(String, Option<u32>)> {
    let session_ref = session.borrow();
    let tab_index = active_tab_index(&session_ref, active_index.get())?;
    session_ref.tabs.get(tab_index).and_then(|tab| {
        tab.preset
            .layout
            .tile_specs()
            .iter()
            .find(|tile| tile.id == tile_id && tile.tile_kind == TileKind::WebView)
            .map(|tile| {
                (
                    normalize_web_url(tile.url.as_deref().unwrap_or(DEFAULT_WEB_URL)),
                    tile.auto_refresh_seconds,
                )
            })
    })
}

fn update_active_web_tile_url(
    session: &Rc<RefCell<SavedSession>>,
    active_index: &Rc<Cell<usize>>,
    url: &str,
) -> bool {
    let normalized_url = normalize_web_url(url);
    let mut session_ref = session.borrow_mut();
    let Some(tab_index) = active_tab_index(&session_ref, active_index.get()) else {
        return false;
    };
    let Some(tile_id) = session_ref.tabs[tab_index]
        .preset
        .layout
        .tile_specs()
        .iter()
        .find(|tile| tile.tile_kind == TileKind::WebView)
        .map(|tile| tile.id.clone())
    else {
        return false;
    };
    let Some(tile) = session_ref.tabs[tab_index]
        .preset
        .layout
        .tile_spec_mut_by_id(&tile_id)
    else {
        return false;
    };
    if tile.url.as_deref() == Some(normalized_url.as_str()) {
        return false;
    }
    tile.url = Some(normalized_url);
    true
}

fn update_active_web_tile_settings(
    session: &Rc<RefCell<SavedSession>>,
    active_index: &Rc<Cell<usize>>,
    tile_id: &str,
    url: &str,
    auto_refresh_seconds: Option<u32>,
) -> bool {
    let normalized_url = normalize_web_url(url);
    let mut session_ref = session.borrow_mut();
    let Some(tab_index) = active_tab_index(&session_ref, active_index.get()) else {
        return false;
    };
    let Some(tile) = session_ref.tabs[tab_index]
        .preset
        .layout
        .tile_spec_mut_by_id(tile_id)
    else {
        return false;
    };
    if tile.tile_kind != TileKind::WebView {
        return false;
    }
    let url_changed = tile.url.as_deref() != Some(normalized_url.as_str());
    let refresh_changed = tile.auto_refresh_seconds != auto_refresh_seconds;
    if url_changed {
        tile.url = Some(normalized_url);
    }
    if refresh_changed {
        tile.auto_refresh_seconds = auto_refresh_seconds;
    }
    url_changed || refresh_changed
}

fn reapply_active_web_runtime_url(
    session: &Rc<RefCell<SavedSession>>,
    active_index: &Rc<Cell<usize>>,
    runtime_surfaces: &Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    tile_id: &str,
) -> bool {
    let session_ref = session.borrow();
    let Some(tab_index) = active_tab_index(&session_ref, active_index.get()) else {
        return false;
    };
    let Some(tab) = session_ref.tabs.get(tab_index) else {
        return false;
    };
    let tile_specs = tab.preset.layout.tile_specs();
    let Some(tile) = tile_specs
        .iter()
        .find(|tile| tile.id == tile_id && tile.tile_kind == TileKind::WebView)
    else {
        return false;
    };
    let key = runtime_surface_key(tab_index, tab, tile);
    runtime_surfaces
        .borrow()
        .get(&key)
        .and_then(|surface| surface.url_applier.as_ref())
        .is_some_and(|apply_url| {
            apply_url(tile.url.as_deref().unwrap_or(DEFAULT_WEB_URL));
            true
        })
}

fn send_command_to_active_runtime_surfaces(
    session: &Rc<RefCell<SavedSession>>,
    active_index: &Rc<Cell<usize>>,
    runtime_surfaces: &Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    target: &BroadcastTarget,
    command: &str,
) -> usize {
    let session_ref = session.borrow();
    let Some(tab_index) = active_tab_index(&session_ref, active_index.get()) else {
        return 0;
    };
    let Some(tab) = session_ref.tabs.get(tab_index) else {
        return 0;
    };
    let surfaces = runtime_surfaces.borrow();
    tab.preset
        .layout
        .tile_specs()
        .iter()
        .filter(|tile| tile.tile_kind == TileKind::Terminal && target.includes(tile))
        .filter(|tile| {
            let key = runtime_surface_key(tab_index, tab, tile);
            surfaces
                .get(&key)
                .and_then(|surface| surface.command_sender.as_ref())
                .is_some_and(|send| send(command))
        })
        .count()
}

fn send_command_to_active_runtime_surface(
    session: &Rc<RefCell<SavedSession>>,
    active_index: &Rc<Cell<usize>>,
    runtime_surfaces: &Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    tile_id: &str,
    command: &str,
) -> bool {
    let session_ref = session.borrow();
    let Some(tab_index) = active_tab_index(&session_ref, active_index.get()) else {
        return false;
    };
    let Some(tab) = session_ref.tabs.get(tab_index) else {
        return false;
    };
    let Some(tile) = tab
        .preset
        .layout
        .tile_specs()
        .iter()
        .find(|tile| tile.id == tile_id && tile.tile_kind == TileKind::Terminal)
        .cloned()
    else {
        return false;
    };
    let key = runtime_surface_key(tab_index, tab, &tile);
    runtime_surfaces
        .borrow()
        .get(&key)
        .and_then(|surface| surface.command_sender.as_ref())
        .is_some_and(|send| send(command))
}

fn connect_preview_tile_close(
    close_button: &gtk::Button,
    tile: &TileSpec,
    on_close_tile: Rc<dyn Fn(String)>,
) {
    let tile_id = tile.id.clone();
    close_button.connect_clicked(move |_| on_close_tile(tile_id.clone()));
}

fn install_preview_tile_drag_and_drop(
    drag_handle: &gtk::Box,
    shell: &gtk::Box,
    tile: &TileSpec,
    on_swap_tile: Rc<dyn Fn(String, String)>,
) {
    let drag_source = gtk::DragSource::builder()
        .actions(gtk::gdk::DragAction::MOVE)
        .build();
    {
        let tile_id = tile.id.clone();
        drag_source.connect_prepare(move |_, _, _| {
            Some(gtk::gdk::ContentProvider::for_value(
                &TileDragPayload::new(tile_id.clone()).to_value(),
            ))
        });
    }
    {
        let shell = shell.clone();
        drag_source.connect_drag_begin(move |_, _| {
            shell.add_css_class("is-dragging");
        });
    }
    {
        let shell = shell.clone();
        drag_source.connect_drag_end(move |_, _, _| {
            shell.remove_css_class("is-dragging");
        });
    }
    drag_handle.add_controller(drag_source);

    let drop_target =
        gtk::DropTarget::new(TileDragPayload::static_type(), gtk::gdk::DragAction::MOVE);
    {
        let shell = shell.clone();
        drop_target.connect_enter(move |_, _, _| {
            shell.add_css_class("is-drop-target");
            gtk::gdk::DragAction::MOVE
        });
    }
    {
        let shell = shell.clone();
        drop_target.connect_leave(move |_| {
            shell.remove_css_class("is-drop-target");
        });
    }
    {
        let shell = shell.clone();
        let target_id = tile.id.clone();
        drop_target.connect_drop(move |_, value, _, _| {
            shell.remove_css_class("is-drop-target");
            let Ok(payload) = value.get::<TileDragPayload>() else {
                return false;
            };
            on_swap_tile(payload.into_tile_id(), target_id.clone());
            true
        });
    }
    shell.add_controller(drop_target);
}

fn install_preview_dropped_file_target(
    shell: &gtk::Box,
    dropped_paths_sender: DroppedPathsSender,
    show_recovery_prompt: Option<Rc<dyn Fn()>>,
) {
    let file_list_drop_target = gtk::DropTarget::new(
        gtk::gdk::FileList::static_type(),
        gtk::gdk::DragAction::COPY,
    );
    {
        let shell = shell.clone();
        file_list_drop_target.connect_enter(move |_, _, _| {
            shell.add_css_class("is-drop-target");
            gtk::gdk::DragAction::COPY
        });
    }
    {
        let shell = shell.clone();
        file_list_drop_target.connect_leave(move |_| {
            shell.remove_css_class("is-drop-target");
        });
    }
    {
        let shell = shell.clone();
        let dropped_paths_sender = dropped_paths_sender.clone();
        let show_recovery_prompt = show_recovery_prompt.clone();
        file_list_drop_target.connect_drop(move |_, value, _, _| {
            shell.remove_css_class("is-drop-target");
            let Ok(files) = value.get::<gtk::gdk::FileList>() else {
                return false;
            };
            let paths = local_paths_from_gio_files(files.files());
            dropped_paths_sender(&paths, show_recovery_prompt.as_deref())
        });
    }
    shell.add_controller(file_list_drop_target);

    let single_file_drop_target =
        gtk::DropTarget::new(gtk::gio::File::static_type(), gtk::gdk::DragAction::COPY);
    {
        let shell = shell.clone();
        single_file_drop_target.connect_enter(move |_, _, _| {
            shell.add_css_class("is-drop-target");
            gtk::gdk::DragAction::COPY
        });
    }
    {
        let shell = shell.clone();
        single_file_drop_target.connect_leave(move |_| {
            shell.remove_css_class("is-drop-target");
        });
    }
    {
        let shell = shell.clone();
        let dropped_paths_sender = dropped_paths_sender.clone();
        let show_recovery_prompt = show_recovery_prompt.clone();
        single_file_drop_target.connect_drop(move |_, value, _, _| {
            shell.remove_css_class("is-drop-target");
            let Ok(file) = value.get::<gtk::gio::File>() else {
                return false;
            };
            let paths = local_paths_from_gio_files([file]);
            dropped_paths_sender(&paths, show_recovery_prompt.as_deref())
        });
    }
    shell.add_controller(single_file_drop_target);

    let uri_list_formats =
        gtk::gdk::ContentFormats::new(&["text/uri-list", "x-special/gnome-copied-files"]);
    let uri_list_drop_target =
        gtk::DropTargetAsync::new(Some(uri_list_formats), gtk::gdk::DragAction::COPY);
    uri_list_drop_target
        .connect_accept(|_, drop| drop_formats_can_contain_uri_list(&drop.formats()));
    {
        let shell = shell.clone();
        uri_list_drop_target.connect_drag_enter(move |_, _, _, _| {
            shell.add_css_class("is-drop-target");
            gtk::gdk::DragAction::COPY
        });
    }
    {
        let shell = shell.clone();
        uri_list_drop_target.connect_drag_leave(move |_, _| {
            shell.remove_css_class("is-drop-target");
        });
    }
    {
        let shell = shell.clone();
        uri_list_drop_target.connect_drop(move |_, drop, _, _| {
            let shell = shell.clone();
            let dropped_paths_sender = dropped_paths_sender.clone();
            let show_recovery_prompt = show_recovery_prompt.clone();
            let drop = drop.clone();
            let drop_for_finish = drop.clone();
            drop.read_async(
                &["text/uri-list", "x-special/gnome-copied-files"],
                gtk::glib::Priority::DEFAULT,
                None::<&gtk::gio::Cancellable>,
                move |result| {
                    shell.remove_css_class("is-drop-target");
                    let Ok((stream, _mime_type)) = result else {
                        drop_for_finish.finish(gtk::gdk::DragAction::empty());
                        return;
                    };
                    gtk::glib::MainContext::default().spawn_local(async move {
                        let Ok(text) = read_drop_stream_text(stream).await else {
                            drop_for_finish.finish(gtk::gdk::DragAction::empty());
                            return;
                        };
                        let paths = local_paths_from_uri_list_text(&text);
                        let accepted =
                            dropped_paths_sender(&paths, show_recovery_prompt.as_deref());
                        drop_for_finish.finish(if accepted {
                            gtk::gdk::DragAction::COPY
                        } else {
                            gtk::gdk::DragAction::empty()
                        });
                    });
                },
            );
            true
        });
    }
    shell.add_controller(uri_list_drop_target);
}

fn drop_formats_can_contain_uri_list(formats: &gtk::gdk::ContentFormats) -> bool {
    formats.contain_mime_type("text/uri-list")
        || formats.contain_mime_type("x-special/gnome-copied-files")
}

async fn read_drop_stream_text(stream: gtk::gio::InputStream) -> Result<String, gtk::glib::Error> {
    let mut bytes = Vec::new();
    loop {
        let chunk = stream
            .read_bytes_future(16 * 1024, gtk::glib::Priority::DEFAULT)
            .await?;
        if chunk.is_empty() {
            break;
        }
        bytes.extend_from_slice(chunk.as_ref());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn local_paths_from_gio_files<I>(files: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = gtk::gio::File>,
{
    files.into_iter().filter_map(|file| file.path()).collect()
}

fn local_paths_from_uri_list_text(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#'))
        .filter(|line| !line.eq_ignore_ascii_case("copy") && !line.eq_ignore_ascii_case("cut"))
        .filter_map(local_path_from_drop_text_line)
        .collect()
}

fn local_path_from_drop_text_line(line: &str) -> Option<PathBuf> {
    if line.starts_with("file://") {
        gtk::gio::File::for_uri(line).path()
    } else if line.starts_with('/') {
        Some(PathBuf::from(line))
    } else {
        None
    }
}

fn build_layout(
    tab_index: usize,
    tab: &SavedTab,
    assets: &WorkspaceAssets,
    render_context: &PreviewRenderContext,
    runtime_factory: Option<&TileRuntimeFactory>,
    runtime_surfaces: &Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    on_close_tile: Rc<dyn Fn(String)>,
) -> gtk::Widget {
    let layout = &tab.preset.layout;
    let ratio_session = render_context.session.clone();
    let ratio_active_index = render_context.active_index.clone();
    let ratio_on_session_changed = render_context.on_session_changed.clone();
    let shell = crate::ui::layout_tree::build(
        layout,
        Some(Rc::new(move |split_path, ratio| {
            if update_active_split_ratio(&ratio_session, &ratio_active_index, &split_path, ratio) {
                notify_session_changed(
                    &ratio_on_session_changed,
                    &ratio_session,
                    "workspace preview split ratio changed",
                );
            }
        })),
    );
    let on_swap_tile = {
        let render_context = render_context.clone();
        Rc::new(move |dragged_id: String, target_id: String| {
            if swap_active_session_tiles(
                &render_context.session,
                &render_context.active_index,
                &dragged_id,
                &target_id,
            ) {
                render_context.rerender();
                render_context.notify_session_changed("workspace preview tile order changed");
            }
        })
    };
    for (index, tile) in layout.tile_specs().iter().enumerate() {
        let Some(slot) = shell.slots.get(index) else {
            continue;
        };
        slot.append(&build_tile(
            tab_index,
            tile,
            tab,
            assets,
            index == 0,
            runtime_factory,
            runtime_surfaces,
            on_close_tile.clone(),
            on_swap_tile.clone(),
            render_context,
        ));
    }
    shell.widget
}

fn build_tile(
    tab_index: usize,
    tile: &TileSpec,
    tab: &SavedTab,
    assets: &WorkspaceAssets,
    active: bool,
    runtime_factory: Option<&TileRuntimeFactory>,
    runtime_surfaces: &Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    on_close_tile: Rc<dyn Fn(String)>,
    on_swap_tile: Rc<dyn Fn(String, String)>,
    render_context: &PreviewRenderContext,
) -> gtk::Widget {
    let shell = build_tile_shell(tile);
    workspace_tile_state::set_tile_active_class(&shell, active);

    let badge_text = tile_badge_text(tile);
    let badge_tooltip = tile_badge_tooltip(tile);
    let (status_text, status_tooltip) = match tile.tile_kind {
        TileKind::Terminal => {
            let label = initial_status_snapshot(tile, &tab.workspace_root, assets).to_line();
            (label.clone(), label)
        }
        TileKind::WebView => {
            let url = normalize_web_url(tile.url.as_deref().unwrap_or("https://google.com"));
            (domain_from_url(&url), url)
        }
    };
    let header = build_tile_header_chrome(TileHeaderInput {
        tile,
        badge_text: &badge_text,
        badge_tooltip: &badge_tooltip,
        badge_max_chars: match tile.tile_kind {
            TileKind::Terminal => TERMINAL_HEADER_BADGE_MAX_CHARS,
            TileKind::WebView => WEB_HEADER_BADGE_MAX_CHARS,
        },
        status_text: &status_text,
        status_tooltip: &status_tooltip,
        status_ellipsize: match tile.tile_kind {
            TileKind::Terminal => pango::EllipsizeMode::Start,
            TileKind::WebView => pango::EllipsizeMode::End,
        },
        drag_tooltip: match tile.tile_kind {
            TileKind::Terminal => "Drag this header to swap terminal positions",
            TileKind::WebView => "Drag this header to swap tile positions",
        },
    });
    install_preview_tile_drag_and_drop(&header.drag_handle, &shell, tile, on_swap_tile);

    let frame_class = match tile.tile_kind {
        TileKind::Terminal => "terminal-frame",
        TileKind::WebView => "web-tile-frame",
    };
    let frame = build_tile_frame(frame_class);

    let (surface, runtime_surface) = if let Some(runtime_factory) = runtime_factory {
        let key = runtime_surface_key(tab_index, tab, tile);
        let mut surfaces = runtime_surfaces.borrow_mut();
        let surface = surfaces
            .entry(key)
            .or_insert_with(|| runtime_factory(tab_index, tile, tab, assets))
            .clone();
        if let Some(apply_appearance) = &surface.appearance_applier {
            apply_appearance(
                resolved_theme_uses_dark_palette(tab.preset.theme),
                tab.preset.density,
                tab.terminal_zoom_steps,
            );
        }
        if tile.tile_kind == TileKind::WebView
            && let Some(apply_url) = &surface.url_applier
        {
            apply_url(tile.url.as_deref().unwrap_or(DEFAULT_WEB_URL));
        }
        if tile.tile_kind == TileKind::WebView
            && let Some(apply_settings) = &surface.web_settings_applier
        {
            apply_settings(
                tile.url.as_deref().unwrap_or(DEFAULT_WEB_URL),
                tile.auto_refresh_seconds,
            );
        }
        detach_from_previous_parent(&surface.widget);
        (surface.widget.clone(), Some(surface))
    } else {
        (build_tile_surface(tile).upcast(), None)
    };

    let actions = header.actions.clone();
    let can_close = tab.preset.layout.tile_count() > 1;
    match tile.tile_kind {
        TileKind::Terminal => {
            let tile_actions = build_terminal_tile_action_chrome(can_close);
            let show_recovery_prompt = if let Some(recovery_binder) = runtime_surface
                .as_ref()
                .and_then(|surface| surface.recovery_binder.as_ref())
            {
                (recovery_binder.bind)(
                    &shell,
                    &header.status_label,
                    &tile_actions.recovery_button,
                    &header.title_label,
                )
            } else {
                None
            };
            if let Some(dropped_paths_sender) = runtime_surface
                .as_ref()
                .and_then(|surface| surface.dropped_paths_sender.as_ref())
            {
                install_preview_dropped_file_target(
                    &shell,
                    dropped_paths_sender.clone(),
                    show_recovery_prompt,
                );
            }
            bind_preview_terminal_snippets(&tile_actions.snippet_button, tile, render_context);
            connect_preview_tile_close(&tile_actions.close_button, tile, on_close_tile.clone());
            append_terminal_tile_action_chrome(&actions, &tile_actions);
        }
        TileKind::WebView => {
            let tile_actions = build_web_tile_action_chrome(can_close);
            bind_preview_web_tile_settings(&tile_actions.settings_button, tile, render_context);
            connect_preview_tile_close(&tile_actions.close_button, tile, on_close_tile.clone());
            append_web_tile_action_chrome(&actions, &tile_actions);
        }
    }
    shell.append(&header.widget);

    frame.append(&surface);
    shell.append(&frame);

    shell.upcast()
}

fn bind_preview_web_tile_settings(
    settings_button: &gtk::Button,
    tile: &TileSpec,
    render_context: &PreviewRenderContext,
) {
    let get_settings = {
        let session = render_context.session.clone();
        let active_index = render_context.active_index.clone();
        Rc::new(move |tile_id: String| active_web_tile_settings(&session, &active_index, &tile_id))
    };
    let on_update_settings = {
        let render_context = render_context.clone();
        Rc::new(
            move |tile_id: String, url: String, auto_refresh_seconds: Option<u32>| {
                if update_active_web_tile_settings(
                    &render_context.session,
                    &render_context.active_index,
                    &tile_id,
                    &url,
                    auto_refresh_seconds,
                ) {
                    render_context.prune_and_rerender();
                    render_context
                        .notify_session_changed("workspace preview web tile settings changed");
                }
            },
        )
    };
    let on_reload = {
        let session = render_context.session.clone();
        let active_index = render_context.active_index.clone();
        let runtime_surfaces = render_context.runtime_surfaces.clone();
        Rc::new(move |tile_id: String| {
            reapply_active_web_runtime_url(&session, &active_index, &runtime_surfaces, &tile_id);
        })
    };
    bind_web_tile_settings_popover(
        settings_button,
        &tile.id,
        get_settings,
        on_update_settings,
        on_reload,
    );
}

fn build_tile_surface(tile: &TileSpec) -> gtk::Box {
    let surface = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .hexpand(true)
        .vexpand(true)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Fill)
        .css_classes(["terminal-surface"])
        .build();
    make_shrinkable(&surface);

    let primary = tile_surface_primary(tile);
    surface.append(
        &gtk::Label::builder()
            .label(&primary)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .margin_top(12)
            .margin_start(12)
            .css_classes(["tile-directory"])
            .build(),
    );

    let detail = tile_surface_detail(tile);
    surface.append(
        &gtk::Label::builder()
            .label(&detail)
            .halign(gtk::Align::Start)
            .margin_start(12)
            .wrap(true)
            .css_classes(["tile-meta"])
            .build(),
    );

    surface
}

fn tile_surface_primary(tile: &TileSpec) -> String {
    match tile.tile_kind {
        TileKind::Terminal => tile
            .startup_command
            .as_deref()
            .map(str::trim)
            .filter(|command| !command.is_empty())
            .map(|command| format!("$ {command}"))
            .unwrap_or_else(|| "$ ready".into()),
        TileKind::WebView => {
            let url = normalize_web_url(tile.url.as_deref().unwrap_or(DEFAULT_WEB_URL));
            domain_from_url(&url)
        }
    }
}

fn tile_surface_detail(tile: &TileSpec) -> String {
    match tile.tile_kind {
        TileKind::Terminal => format!("{} • {}", tile.title, tile.working_directory.short_label()),
        TileKind::WebView => normalize_web_url(tile.url.as_deref().unwrap_or(DEFAULT_WEB_URL)),
    }
}

fn tile_badge_text(tile: &TileSpec) -> String {
    match tile.tile_kind {
        TileKind::Terminal => tile.agent_label.clone(),
        TileKind::WebView => "🌐".into(),
    }
}

fn tile_badge_tooltip(tile: &TileSpec) -> String {
    match tile.tile_kind {
        TileKind::Terminal => tile.agent_label.clone(),
        TileKind::WebView => "Web tile".into(),
    }
}

fn build_empty_state() -> gtk::Widget {
    gtk::Label::builder()
        .label("No saved workspace session")
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .vexpand(true)
        .css_classes(["workspace-summary-subtitle"])
        .build()
        .upcast()
}

#[cfg(test)]
mod tests {
    use super::{detach_tab_in_preview_state, runtime_surface_keys_for_tab};
    use crate::model::layout::{SplitAxis, WorkingDirectory, split, tile};
    use crate::model::preset::{ApplicationDensity, ThemeMode, WorkspacePreset};
    use crate::storage::session_store::{SavedSession, SavedTab};
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;
    use std::rc::Rc;

    fn test_tab(preset_id: &str, tile_ids: &[&str]) -> SavedTab {
        let layout = match tile_ids {
            [only] => tile(
                only,
                only,
                "Shell",
                "accent-cyan",
                WorkingDirectory::WorkspaceRoot,
                None,
            ),
            [first, second] => split(
                SplitAxis::Horizontal,
                0.5,
                tile(
                    first,
                    first,
                    "Shell",
                    "accent-cyan",
                    WorkingDirectory::WorkspaceRoot,
                    None,
                ),
                tile(
                    second,
                    second,
                    "Shell",
                    "accent-cyan",
                    WorkingDirectory::WorkspaceRoot,
                    None,
                ),
            ),
            _ => panic!("test helper supports one or two tiles"),
        };

        SavedTab {
            preset: WorkspacePreset {
                id: preset_id.into(),
                name: preset_id.into(),
                description: String::new(),
                tags: Vec::new(),
                root_label: "Workspace root".into(),
                workspace_root: None,
                theme: ThemeMode::System,
                density: ApplicationDensity::Standard,
                layout,
            },
            workspace_root: PathBuf::from(format!("/tmp/{preset_id}")),
            custom_title: None,
            terminal_zoom_steps: 0,
            terminal_history: Vec::new(),
        }
    }

    #[test]
    fn detach_preview_tab_preserves_following_surface_key_moves() {
        let first = test_tab("first", &["a"]);
        let detached = test_tab("detached", &["b1", "b2"]);
        let following = test_tab("following", &["c"]);
        let session = Rc::new(RefCell::new(SavedSession {
            tabs: vec![first.clone(), detached.clone(), following.clone()],
            active_tab_index: 2,
        }));
        let active_index = Rc::new(Cell::new(2));

        let (removed_tab, key_update) =
            detach_tab_in_preview_state(&session, &active_index, 1).expect("detaches middle tab");

        assert_eq!(removed_tab.preset.id, "detached");
        assert_eq!(
            session
                .borrow()
                .tabs
                .iter()
                .map(|tab| tab.preset.id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "following"]
        );
        assert_eq!(session.borrow().active_tab_index, 1);
        assert_eq!(active_index.get(), 1);
        assert_eq!(
            key_update.stale_keys,
            runtime_surface_keys_for_tab(1, &detached)
        );
        assert_eq!(
            key_update.key_moves,
            vec![(
                runtime_surface_keys_for_tab(2, &following)
                    .into_iter()
                    .next()
                    .expect("following tab has a tile"),
                runtime_surface_keys_for_tab(1, &following)
                    .into_iter()
                    .next()
                    .expect("following tab has a tile"),
            )]
        );
    }

    #[test]
    fn detach_preview_tab_keeps_single_tab_sessions_intact() {
        let only = test_tab("only", &["a"]);
        let session = Rc::new(RefCell::new(SavedSession {
            tabs: vec![only.clone()],
            active_tab_index: 0,
        }));
        let active_index = Rc::new(Cell::new(0));

        assert!(detach_tab_in_preview_state(&session, &active_index, 0).is_none());
        assert_eq!(session.borrow().tabs.len(), 1);
        assert_eq!(session.borrow().tabs[0].preset.id, "only");
        assert_eq!(session.borrow().active_tab_index, 0);
        assert_eq!(active_index.get(), 0);
    }
}
