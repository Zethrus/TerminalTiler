use gtk::prelude::*;
use gtk::{glib, pango};

use crate::model::layout::{LayoutNode, TileKind, TileSpec, normalize_web_url};
use crate::storage::session_store::{SavedSession, SavedTab};
use crate::ui::icons::{self, name as icon_name};
use crate::ui::tile_chrome::{
    HEADER_STATUS_MAX_CHARS, HEADER_TITLE_MAX_CHARS, TERMINAL_HEADER_BADGE_MAX_CHARS,
    WEB_HEADER_BADGE_MAX_CHARS, build_header_icon_button, build_pane_group_chip,
    configure_dynamic_header_label, domain_from_url,
};
use crate::ui::workspace_chrome::{WorkspaceSummaryInput, build_workspace_summary_chrome};

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
    let active_index = session
        .active_tab_index
        .min(session.tabs.len().saturating_sub(1));
    let active_tab = session.tabs.get(active_index);

    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(4)
        .margin_end(4)
        .hexpand(true)
        .vexpand(true)
        .build();
    make_shrinkable(&shell);

    if !session.tabs.is_empty() {
        shell.append(&build_tab_strip(session, active_index));
    }

    if let Some(tab) = active_tab {
        shell.append(&build_workspace_summary(tab));
        let layout = build_layout(&tab.preset.layout);
        shell.append(&layout);
    } else {
        shell.append(&build_empty_state());
    }

    shell.upcast()
}

pub fn session_shape(session: &SavedSession) -> (usize, usize) {
    let pane_count = session
        .tabs
        .iter()
        .map(|tab| tab.preset.layout.tile_specs().len())
        .sum::<usize>();
    (session.tabs.len(), pane_count)
}

fn build_tab_strip(session: &SavedSession, active_index: usize) -> gtk::Widget {
    let strip = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .halign(gtk::Align::Start)
        .css_classes(["app-tab-strip"])
        .build();
    make_shrinkable(&strip);

    for (index, tab) in session.tabs.iter().enumerate() {
        strip.append(&build_tab_chip(tab, index == active_index));
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

fn build_tab_chip(tab: &SavedTab, active: bool) -> gtk::Widget {
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .valign(gtk::Align::End)
        .css_classes(["app-tab-shell"])
        .build();
    shell.add_css_class(if active { "is-active" } else { "is-inactive" });

    let select = gtk::Button::builder()
        .css_classes(["app-tab-select"])
        .build();
    select.set_sensitive(false);
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    content.append(
        &gtk::Image::builder()
            .icon_name(icon_name::TERMINAL)
            .css_classes(["app-tab-icon"])
            .build(),
    );
    let title = tab
        .custom_title
        .as_deref()
        .unwrap_or(tab.preset.name.as_str());
    let title_label = gtk::Label::builder()
        .label(title)
        .ellipsize(pango::EllipsizeMode::End)
        .css_classes(["app-tab-title"])
        .build();
    title_label.set_tooltip_text(Some(title));
    content.append(&title_label);
    content.append(
        &gtk::Label::builder()
            .label(tab.preset.layout.tile_specs().len().to_string())
            .css_classes(["app-tab-badge"])
            .build(),
    );
    select.set_child(Some(&content));
    shell.append(&select);

    let close = icons::icon_button(icon_name::CLOSE, "Close tab", &["flat", "app-tab-close"]);
    close.set_sensitive(false);
    shell.append(&close);

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
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .hexpand(true)
        .vexpand(true)
        .css_classes(["terminal-card", tile.accent_class.as_str()])
        .build();
    if active {
        shell.add_css_class("is-active-tile");
    }
    make_shrinkable(&shell);

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["terminal-header"])
        .build();
    make_shrinkable(&header);

    let left = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    make_shrinkable(&left);
    left.set_tooltip_text(Some(match tile.tile_kind {
        TileKind::Terminal => "Drag this header to swap terminal positions",
        TileKind::WebView => "Drag this header to swap tile positions",
    }));

    let badge_text = tile_badge_text(tile);
    let badge_tooltip = tile_badge_tooltip(tile);
    let badge = gtk::Label::builder()
        .label(&badge_text)
        .halign(gtk::Align::Start)
        .css_classes(["agent-badge"])
        .build();
    configure_dynamic_header_label(
        &badge,
        &badge_tooltip,
        match tile.tile_kind {
            TileKind::Terminal => TERMINAL_HEADER_BADGE_MAX_CHARS,
            TileKind::WebView => WEB_HEADER_BADGE_MAX_CHARS,
        },
        pango::EllipsizeMode::End,
    );
    left.append(&badge);

    let title = gtk::Label::builder()
        .label(&tile.title)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .css_classes(["tile-title"])
        .build();
    configure_dynamic_header_label(
        &title,
        &tile.title,
        HEADER_TITLE_MAX_CHARS,
        pango::EllipsizeMode::End,
    );
    left.append(&title);

    if let Some(pane_group_label) = build_pane_group_chip(&tile.pane_groups) {
        left.append(&pane_group_label);
    }
    header.append(&left);

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
    let status = gtk::Label::builder()
        .label(&status_text)
        .valign(gtk::Align::Center)
        .css_classes(["status-chip"])
        .build();
    configure_dynamic_header_label(
        &status,
        &status_tooltip,
        HEADER_STATUS_MAX_CHARS,
        match tile.tile_kind {
            TileKind::Terminal => pango::EllipsizeMode::Start,
            TileKind::WebView => pango::EllipsizeMode::End,
        },
    );

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    actions.append(&status);
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
    header.append(&actions);
    shell.append(&header);

    let frame_class = match tile.tile_kind {
        TileKind::Terminal => "terminal-frame",
        TileKind::WebView => "web-tile-frame",
    };
    let frame = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .hexpand(true)
        .vexpand(true)
        .css_classes([frame_class])
        .build();
    make_shrinkable(&frame);

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
    surface.append(
        &gtk::Label::builder()
            .label(match tile.tile_kind {
                TileKind::Terminal => "$ terminal runtime adapter",
                TileKind::WebView => "web runtime adapter",
            })
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .margin_top(12)
            .margin_start(12)
            .css_classes(["tile-directory"])
            .build(),
    );
    surface.append(
        &gtk::Label::builder()
            .label("Windows GTK shell is using the shared Linux workspace layout contract.")
            .halign(gtk::Align::Start)
            .margin_start(12)
            .wrap(true)
            .css_classes(["tile-meta"])
            .build(),
    );
    frame.append(&surface);
    shell.append(&frame);

    shell.upcast()
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

fn make_shrinkable<W: glib::object::IsA<gtk::Widget>>(widget: &W) {
    widget.set_size_request(0, 0);
    widget.set_overflow(gtk::Overflow::Hidden);
}
