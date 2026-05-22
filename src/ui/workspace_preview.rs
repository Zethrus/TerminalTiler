use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk::pango;
use gtk::prelude::*;

use crate::model::assets::WorkspaceAssets;
use crate::model::layout::{DEFAULT_WEB_URL, TileKind, TileSpec, normalize_web_url};
use crate::storage::session_store::{SavedSession, SavedTab};
use crate::ui::icons::{self, name as icon_name};
use crate::ui::pane_status::initial_status_snapshot;
use crate::ui::tile_chrome::{
    TERMINAL_HEADER_BADGE_MAX_CHARS, TileHeaderInput, WEB_HEADER_BADGE_MAX_CHARS,
    append_terminal_tile_action_chrome, append_web_tile_action_chrome,
    build_terminal_tile_action_chrome, build_tile_frame, build_tile_header_chrome,
    build_tile_shell, build_web_tile_action_chrome, domain_from_url, make_shrinkable,
};
use crate::ui::title_chrome::build_title_tab_chrome;
use crate::ui::workspace_chrome::{
    WorkspaceSummaryInput, build_workspace_alert_revealer, build_workspace_alert_sidebar_chrome,
    build_workspace_content_chrome, build_workspace_shell_chrome, build_workspace_summary_chrome,
};

pub type TileRuntimeFactory = Rc<dyn Fn(&TileSpec, &SavedTab, &WorkspaceAssets) -> gtk::Widget>;

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
    runtime_surfaces: Rc<RefCell<HashMap<String, gtk::Widget>>>,
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
    runtime_surfaces: Rc<RefCell<HashMap<String, gtk::Widget>>>,
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
        shell.append(&build_workspace_summary(tab));
        let layout = build_layout(
            current_index,
            tab,
            assets,
            runtime_factory.as_ref(),
            &runtime_surfaces,
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

fn build_workspace_summary(tab: &SavedTab) -> gtk::Widget {
    build_workspace_summary_chrome(WorkspaceSummaryInput {
        name: &tab.preset.name,
        path: tab.workspace_root.display().to_string(),
        pane_groups: saved_groups(tab),
        controls_sensitive: true,
    })
    .widget
}

fn saved_groups(tab: &SavedTab) -> Vec<String> {
    let mut groups = tab
        .preset
        .layout
        .tile_specs()
        .into_iter()
        .flat_map(|tile| tile.pane_groups)
        .filter(|group| !group.trim().is_empty())
        .collect::<Vec<_>>();
    groups.sort();
    groups.dedup();
    groups
}

fn build_layout(
    tab_index: usize,
    tab: &SavedTab,
    assets: &WorkspaceAssets,
    runtime_factory: Option<&TileRuntimeFactory>,
    runtime_surfaces: &Rc<RefCell<HashMap<String, gtk::Widget>>>,
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
    runtime_surfaces: &Rc<RefCell<HashMap<String, gtk::Widget>>>,
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
    match tile.tile_kind {
        TileKind::Terminal => {
            let tile_actions = build_terminal_tile_action_chrome(true);
            append_terminal_tile_action_chrome(&actions, &tile_actions);
        }
        TileKind::WebView => {
            let tile_actions = build_web_tile_action_chrome(true);
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
        detach_from_previous_parent(&surface);
        surface
    } else {
        build_tile_surface(tile).upcast()
    };
    frame.append(&surface);
    shell.append(&frame);

    shell.upcast()
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
