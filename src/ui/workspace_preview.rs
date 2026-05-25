use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk::pango;
use gtk::prelude::*;

use crate::model::assets::{TemplateVariableValues, WorkspaceAssets};
use crate::model::layout::{DEFAULT_WEB_URL, SplitAxis, TileKind, TileSpec, normalize_web_url};
use crate::model::preset::ApplicationDensity;
use crate::services::broadcast::{BroadcastTarget, saved_groups_for_tiles};
use crate::services::layout_editor::{close_tile, split_web_tile};
use crate::services::runbooks::resolve_runbook;
use crate::storage::session_store::{SavedSession, SavedTab};
use crate::ui::icons::{self, name as icon_name};
use crate::ui::pane_status::initial_status_snapshot;
use crate::ui::tile_chrome::{
    TERMINAL_HEADER_BADGE_MAX_CHARS, TileHeaderInput, WEB_HEADER_BADGE_MAX_CHARS,
    append_terminal_tile_action_chrome, append_web_tile_action_chrome,
    bind_web_tile_settings_popover, build_terminal_tile_action_chrome, build_tile_frame,
    build_tile_header_chrome, build_tile_shell, build_web_tile_action_chrome, domain_from_url,
    make_shrinkable,
};
use crate::ui::title_chrome::build_title_tab_chrome;
use crate::ui::workspace_chrome::{
    WorkspaceSummaryChrome, WorkspaceSummaryInput, build_workspace_alert_revealer,
    build_workspace_alert_sidebar_chrome, build_workspace_content_chrome,
    build_workspace_shell_chrome, build_workspace_summary_chrome,
};

#[derive(Clone)]
pub struct TileRuntimeSurface {
    pub widget: gtk::Widget,
    pub command_sender: Option<Rc<dyn Fn(&str) -> bool>>,
    pub appearance_applier: Option<Rc<dyn Fn(ApplicationDensity, i32)>>,
    pub url_applier: Option<Rc<dyn Fn(&str)>>,
}

impl TileRuntimeSurface {
    pub fn widget(widget: gtk::Widget) -> Self {
        Self {
            widget,
            command_sender: None,
            appearance_applier: None,
            url_applier: None,
        }
    }
}

pub type TileRuntimeFactory =
    Rc<dyn Fn(&TileSpec, &SavedTab, &WorkspaceAssets) -> TileRuntimeSurface>;

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
        );
    }

    fn prune_and_rerender(&self) {
        prune_runtime_surfaces(&self.runtime_surfaces, &self.session.borrow());
        self.rerender();
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
            runtime_surfaces: Rc::new(RefCell::new(HashMap::new())),
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
        self.render();
    }

    pub fn active_index(&self) -> usize {
        self.active_index.get()
    }

    pub fn snapshot(&self) -> SavedSession {
        self.session.borrow().clone()
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
        self.render();
        next_index
    }

    pub fn close_tab(&self, index: usize) -> bool {
        if close_tab_in_preview_state(&self.session, &self.active_index, index) {
            self.prune_runtime_surfaces();
            self.render();
            true
        } else {
            false
        }
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
        self.render();
        Some(next_zoom_steps)
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
        );
    }

    fn prune_runtime_surfaces(&self) {
        let live_keys = runtime_surface_keys(&self.session.borrow());
        self.runtime_surfaces
            .borrow_mut()
            .retain(|key, _| live_keys.iter().any(|live_key| live_key == key));
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
) -> bool {
    let mut session = session.borrow_mut();
    if session.tabs.is_empty() {
        return false;
    }

    let removed_index = index.min(session.tabs.len() - 1);
    session.tabs.remove(removed_index);
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
    true
}

fn render_session_preview(
    shell: &gtk::Box,
    session: &Rc<RefCell<SavedSession>>,
    assets: &Rc<WorkspaceAssets>,
    active_index: &Rc<Cell<usize>>,
    show_inline_tab_strip: bool,
    runtime_factory: Option<TileRuntimeFactory>,
    runtime_surfaces: Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
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
            Rc::new(move |index: usize| {
                if close_tab_in_preview_state(&session, &active_index, index) {
                    let live_keys = runtime_surface_keys(&session.borrow());
                    runtime_surfaces
                        .borrow_mut()
                        .retain(|key, _| live_keys.iter().any(|live_key| live_key == key));
                    render_session_preview(
                        &shell,
                        &session,
                        &assets,
                        &active_index,
                        true,
                        runtime_factory.clone(),
                        runtime_surfaces.clone(),
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
            Rc::new(move |next_index: usize| {
                let next_index = {
                    let session = session.borrow();
                    next_index.min(session.tabs.len().saturating_sub(1))
                };
                active_index.set(next_index);
                session.borrow_mut().active_tab_index = next_index;
                render_session_preview(
                    &shell,
                    &session,
                    &assets,
                    &active_index,
                    true,
                    runtime_factory.clone(),
                    runtime_surfaces.clone(),
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
        };
        shell.append(&build_workspace_summary(tab, &render_context));
        let on_close_tile = {
            let render_context = render_context.clone();
            Rc::new(move |tile_id: String| {
                if close_active_session_tile(
                    &render_context.session,
                    &render_context.active_index,
                    &tile_id,
                ) {
                    render_context.prune_and_rerender();
                }
            })
        };
        let layout = build_layout(
            current_index,
            tab,
            assets,
            runtime_factory.as_ref(),
            &runtime_surfaces,
            on_close_tile,
        );
        let alert_sidebar = build_workspace_alert_sidebar_chrome(true);
        let alert_revealer = build_workspace_alert_revealer(&alert_sidebar.widget);
        shell.append(&build_workspace_content_chrome(&layout, &alert_revealer));
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
    let chrome = build_title_tab_chrome();
    let shell = chrome.shell;
    shell.set_valign(gtk::Align::End);
    shell.remove_css_class("is-inactive");
    shell.remove_css_class("is-active");
    shell.add_css_class(if active { "is-active" } else { "is-inactive" });

    let title = tab
        .custom_title
        .as_deref()
        .unwrap_or(tab.preset.name.as_str());
    chrome.title_label.set_label(title);
    chrome.title_label.set_tooltip_text(Some(title));
    {
        let on_select = on_select.clone();
        chrome.select_button.connect_clicked(move |_| {
            on_select(index);
        });
    }

    chrome.close_button.connect_clicked(move |_| {
        on_close(index);
    });

    shell.upcast()
}

fn build_workspace_summary(tab: &SavedTab, render_context: &PreviewRenderContext) -> gtk::Widget {
    let summary = build_workspace_summary_chrome(WorkspaceSummaryInput {
        name: &tab.preset.name,
        path: tab.workspace_root.display().to_string(),
        pane_groups: saved_groups(tab),
        controls_sensitive: true,
    });

    let has_web_tiles = tab
        .preset
        .layout
        .tile_specs()
        .iter()
        .any(|tile| tile.tile_kind == TileKind::WebView);
    summary.path_label.set_visible(!has_web_tiles);
    summary.url_entry.set_visible(has_web_tiles);
    summary.url_reload_button.set_visible(has_web_tiles);
    if let Some(url) = first_web_tile_url(tab) {
        summary.url_entry.set_text(&url);
    }

    let broadcast_target = Rc::new(RefCell::new(BroadcastTarget::Off));
    {
        let broadcast_target = broadcast_target.clone();
        let broadcast_state = summary.broadcast_state.clone();
        summary.broadcast_selector.connect_changed(move |combo| {
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

    let send_broadcast = Rc::new({
        let session = render_context.session.clone();
        let active_index = render_context.active_index.clone();
        let runtime_surfaces = render_context.runtime_surfaces.clone();
        let broadcast_target = broadcast_target.clone();
        let broadcast_entry = summary.broadcast_entry.clone();
        let broadcast_state = summary.broadcast_state.clone();
        move || {
            let command = broadcast_entry.text().trim().to_string();
            if command.is_empty() {
                return;
            }
            let target = broadcast_target.borrow().clone();
            let sent = send_command_to_active_runtime_surfaces(
                &session,
                &active_index,
                &runtime_surfaces,
                &target,
                &command,
            );
            broadcast_state.set_text(&format!("{}  •  sent to {}", target.label(), sent));
            if sent > 0 {
                broadcast_entry.set_text("");
            }
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
        let url_entry = summary.url_entry.clone();
        summary.add_web_tile_button.connect_clicked(move |_| {
            if add_web_tile_to_active_session(
                &render_context.session,
                &render_context.active_index,
                &url_entry.text(),
            ) {
                render_context.prune_and_rerender();
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

    bind_preview_runbook_controls(&summary, render_context);

    summary.widget
}

fn saved_groups(tab: &SavedTab) -> Vec<String> {
    saved_groups_for_tiles(&tab.preset.layout.tile_specs())
}

fn bind_preview_runbook_controls(
    summary: &WorkspaceSummaryChrome,
    render_context: &PreviewRenderContext,
) {
    summary.runbook_selector.remove_all();
    summary.runbook_selector.append(Some(""), "Runbook");
    for runbook in &render_context.assets.runbooks {
        summary
            .runbook_selector
            .append(Some(&runbook.id), &runbook.name);
    }
    summary.runbook_selector.set_active_id(Some(""));
    summary
        .runbook_button
        .set_sensitive(!render_context.assets.runbooks.is_empty());

    let session = render_context.session.clone();
    let active_index = render_context.active_index.clone();
    let runtime_surfaces = render_context.runtime_surfaces.clone();
    let assets = render_context.assets.clone();
    let runbook_selector = summary.runbook_selector.clone();
    let broadcast_state = summary.broadcast_state.clone();
    summary.runbook_button.connect_clicked(move |_| {
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
        if !runbook.variables.is_empty() {
            broadcast_state.set_text("Runbook requires variables");
            return;
        }
        let tile_specs = active_tab_tile_specs(&session, &active_index);
        match resolve_runbook(runbook, &TemplateVariableValues::default(), &tile_specs) {
            Ok(resolved) => {
                let sent = resolved
                    .commands
                    .iter()
                    .map(|command| {
                        send_command_to_active_runtime_surfaces(
                            &session,
                            &active_index,
                            &runtime_surfaces,
                            &resolved.target,
                            command,
                        )
                    })
                    .sum::<usize>();
                broadcast_state
                    .set_text(&format!("{}  •  sent to {}", resolved.target_label, sent));
            }
            Err(error) => broadcast_state.set_text(&error.to_string()),
        }
    });
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
) {
    let live_keys = runtime_surface_keys(session);
    runtime_surfaces
        .borrow_mut()
        .retain(|key, _| live_keys.iter().any(|live_key| live_key == key));
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
    let Some(tile) = tab
        .preset
        .layout
        .tile_specs()
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

fn connect_preview_tile_close(
    close_button: &gtk::Button,
    tile: &TileSpec,
    on_close_tile: Rc<dyn Fn(String)>,
) {
    let tile_id = tile.id.clone();
    close_button.connect_clicked(move |_| on_close_tile(tile_id.clone()));
}

fn build_layout(
    tab_index: usize,
    tab: &SavedTab,
    assets: &WorkspaceAssets,
    runtime_factory: Option<&TileRuntimeFactory>,
    runtime_surfaces: &Rc<RefCell<HashMap<String, TileRuntimeSurface>>>,
    on_close_tile: Rc<dyn Fn(String)>,
) -> gtk::Widget {
    let layout = &tab.preset.layout;
    let shell = crate::ui::layout_tree::build(layout, None);
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
) -> gtk::Widget {
    let shell = build_tile_shell(tile);
    if active {
        shell.add_css_class("is-active-tile");
    }

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

    let actions = header.actions.clone();
    let can_close = tab.preset.layout.tile_count() > 1;
    match tile.tile_kind {
        TileKind::Terminal => {
            let tile_actions = build_terminal_tile_action_chrome(can_close);
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

    let frame_class = match tile.tile_kind {
        TileKind::Terminal => "terminal-frame",
        TileKind::WebView => "web-tile-frame",
    };
    let frame = build_tile_frame(frame_class);

    let surface = if let Some(runtime_factory) = runtime_factory {
        let key = runtime_surface_key(tab_index, tab, tile);
        let mut surfaces = runtime_surfaces.borrow_mut();
        let surface = surfaces
            .entry(key)
            .or_insert_with(|| runtime_factory(tile, tab, assets))
            .clone();
        if let Some(apply_appearance) = &surface.appearance_applier {
            apply_appearance(tab.preset.density, tab.terminal_zoom_steps);
        }
        if tile.tile_kind == TileKind::WebView
            && let Some(apply_url) = &surface.url_applier
        {
            apply_url(tile.url.as_deref().unwrap_or(DEFAULT_WEB_URL));
        }
        detach_from_previous_parent(&surface.widget);
        surface.widget
    } else {
        build_tile_surface(tile).upcast()
    };
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
