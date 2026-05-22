use std::cell::Cell;
use std::rc::Rc;

use gtk::pango;
use gtk::prelude::*;

use crate::model::layout::{DEFAULT_WEB_URL, LayoutNode, TileKind, TileSpec, normalize_web_url};
use crate::storage::session_store::{SavedSession, SavedTab};
use crate::ui::icons::{self, name as icon_name};
use crate::ui::tile_chrome::{
    TERMINAL_HEADER_BADGE_MAX_CHARS, TileHeaderInput, WEB_HEADER_BADGE_MAX_CHARS,
    build_header_icon_button, build_tile_frame, build_tile_header_chrome, build_tile_shell,
    domain_from_url, make_shrinkable,
};
use crate::ui::title_chrome::build_title_tab_chrome;
use crate::ui::workspace_chrome::{
    WorkspaceSummaryInput, build_workspace_alert_revealer, build_workspace_alert_sidebar_chrome,
    build_workspace_content_chrome, build_workspace_shell_chrome, build_workspace_summary_chrome,
};

/// Build a GTK workspace shell that mirrors the Linux workspace chrome without
/// binding to a platform-specific terminal/web runtime.
///
/// Windows uses this as the visual parity surface while its ConPTY/WebView2
/// adapters are being moved behind the shared GTK layout.  The widget therefore
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
    session: Rc<SavedSession>,
    active_index: Rc<Cell<usize>>,
    show_inline_tab_strip: bool,
}

impl SessionPreview {
    pub fn new(session: &SavedSession, show_inline_tab_strip: bool) -> Self {
        let session = Rc::new(session.clone());
        let active_index = Rc::new(Cell::new(
            session
                .active_tab_index
                .min(session.tabs.len().saturating_sub(1)),
        ));

        let shell = build_workspace_shell_chrome();

        let preview = Self {
            shell,
            session,
            active_index,
            show_inline_tab_strip,
        };
        preview.render();
        preview
    }

    pub fn widget(&self) -> gtk::Widget {
        self.shell.clone().upcast()
    }

    pub fn select_tab(&self, next_index: usize) {
        self.active_index
            .set(next_index.min(self.session.tabs.len().saturating_sub(1)));
        self.render();
    }

    pub fn active_index(&self) -> usize {
        self.active_index.get()
    }

    fn render(&self) {
        render_session_preview(
            &self.shell,
            &self.session,
            &self.active_index,
            self.show_inline_tab_strip,
        );
    }
}

fn render_session_preview(
    shell: &gtk::Box,
    session: &Rc<SavedSession>,
    active_index: &Rc<Cell<usize>>,
    show_inline_tab_strip: bool,
) {
    while let Some(child) = shell.first_child() {
        shell.remove(&child);
    }

    let current_index = active_index.get().min(session.tabs.len().saturating_sub(1));
    active_index.set(current_index);
    let active_tab = session.tabs.get(current_index);

    if show_inline_tab_strip && !session.tabs.is_empty() {
        let on_select = {
            let shell = shell.clone();
            let session = session.clone();
            let active_index = active_index.clone();
            Rc::new(move |next_index: usize| {
                active_index.set(next_index.min(session.tabs.len().saturating_sub(1)));
                render_session_preview(&shell, &session, &active_index, true);
            })
        };
        shell.append(&build_tab_strip(session, current_index, on_select));
    }

    if let Some(tab) = active_tab {
        shell.append(&build_workspace_summary(tab));
        let layout = build_layout(&tab.preset.layout);
        let alert_sidebar = build_workspace_alert_sidebar_chrome(false);
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

fn build_tab_strip(
    session: &SavedSession,
    active_index: usize,
    on_select: Rc<dyn Fn(usize)>,
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
        ));
    }

    let add_button = icons::icon_button(
        icon_name::ADD,
        "New workspace tab",
        &["flat", "app-tab-add"],
    );
    add_button.set_sensitive(false);
    strip.append(&add_button);

    strip.upcast()
}

fn build_tab_chip(
    tab: &SavedTab,
    index: usize,
    active: bool,
    on_select: Rc<dyn Fn(usize)>,
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
    chrome
        .badge_label
        .set_label(&tab.preset.layout.tile_specs().len().to_string());
    chrome.badge_label.set_visible(true);
    {
        let on_select = on_select.clone();
        chrome.select_button.connect_clicked(move |_| {
            on_select(index);
        });
    }

    chrome.close_button.set_sensitive(false);

    shell.upcast()
}

fn build_workspace_summary(tab: &SavedTab) -> gtk::Widget {
    build_workspace_summary_chrome(WorkspaceSummaryInput {
        name: &tab.preset.name,
        path: tab.workspace_root.display().to_string(),
        pane_groups: saved_groups(tab),
        controls_sensitive: false,
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

fn build_layout(layout: &LayoutNode) -> gtk::Widget {
    let shell = crate::ui::layout_tree::build(layout, None);
    for (index, tile) in layout.tile_specs().iter().enumerate() {
        let Some(slot) = shell.slots.get(index) else {
            continue;
        };
        slot.append(&build_tile(tile, index == 0));
    }
    shell.widget
}

fn build_tile(tile: &TileSpec, active: bool) -> gtk::Widget {
    let shell = build_tile_shell(tile);
    if active {
        shell.add_css_class("is-active-tile");
    }

    let badge_text = tile_badge_text(tile);
    let badge_tooltip = tile_badge_tooltip(tile);
    let (status_text, status_tooltip) = match tile.tile_kind {
        TileKind::Terminal => {
            let label = tile.working_directory.short_label();
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
            let recovery_button = build_header_icon_button(icon_name::RECOVER, "Recover pane");
            recovery_button.add_css_class("tile-recovery-action");
            recovery_button.set_sensitive(false);
            let snippet_button = build_header_icon_button(icon_name::SNIPPET, "Run CLI snippet");
            snippet_button.add_css_class("tile-snippet-action");
            snippet_button.set_sensitive(false);
            actions.append(&recovery_button);
            actions.append(&snippet_button);
        }
        TileKind::WebView => {
            let settings_button =
                build_header_icon_button(icon_name::SETTINGS, "Edit URL and refresh settings");
            settings_button.set_sensitive(false);
            actions.append(&settings_button);
        }
    }

    let close_button = build_header_icon_button(icon_name::CLOSE, "Close tile");
    close_button.set_sensitive(false);
    actions.append(&close_button);
    shell.append(&header.widget);

    let frame_class = match tile.tile_kind {
        TileKind::Terminal => "terminal-frame",
        TileKind::WebView => "web-tile-frame",
    };
    let frame = build_tile_frame(frame_class);

    let surface = build_tile_surface(tile);
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
