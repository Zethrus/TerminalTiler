use crate::model::layout::{DEFAULT_WEB_URL, TileSpec};
use crate::ui::dialog_chrome;
use crate::ui::icons::{self, name as icon_name};
use gtk::prelude::*;
use std::rc::Rc;

pub(crate) const TERMINAL_HEADER_BADGE_MAX_CHARS: i32 = 12;
pub(crate) const WEB_HEADER_BADGE_MAX_CHARS: i32 = 4;
pub(crate) const HEADER_GROUP_MAX_CHARS: i32 = 16;
pub(crate) const HEADER_STATUS_MAX_CHARS: i32 = 36;
/// The title label is not width-capped (`-1`): it expands to fill the header and only
/// ellipsizes when the tile is physically too narrow, so agent session titles show in full.
pub(crate) const HEADER_TITLE_MAX_CHARS: i32 = -1;

pub(crate) fn build_header_icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    icons::icon_button(
        icon_name,
        tooltip,
        &["flat", "tile-header-action", "tile-header-close"],
    )
}

pub(crate) struct TileHeaderInput<'a> {
    pub(crate) tile: &'a TileSpec,
    pub(crate) badge_text: &'a str,
    pub(crate) badge_tooltip: &'a str,
    pub(crate) badge_max_chars: i32,
    pub(crate) status_text: &'a str,
    pub(crate) status_tooltip: &'a str,
    pub(crate) status_ellipsize: gtk::pango::EllipsizeMode,
    pub(crate) drag_tooltip: &'a str,
}

pub(crate) struct TileHeaderChrome {
    pub(crate) widget: gtk::Box,
    pub(crate) drag_handle: gtk::Box,
    pub(crate) actions: gtk::Box,
    pub(crate) title_label: gtk::Label,
    pub(crate) status_label: gtk::Label,
}

pub(crate) struct TerminalTileActionChrome {
    pub(crate) recovery_button: gtk::Button,
    pub(crate) snippet_button: gtk::Button,
    pub(crate) close_button: gtk::Button,
}

pub(crate) struct WebTileActionChrome {
    pub(crate) settings_button: gtk::Button,
    pub(crate) close_button: gtk::Button,
}

pub(crate) type GetWebTileSettings = Rc<dyn Fn(String) -> Option<(String, Option<u32>)>>;

pub(crate) fn build_terminal_tile_action_chrome(can_close: bool) -> TerminalTileActionChrome {
    let recovery_button = build_header_icon_button(icon_name::RECOVER, "Recover pane");
    recovery_button.add_css_class("tile-recovery-action");
    recovery_button.set_visible(false);
    recovery_button.set_sensitive(false);

    let snippet_button = build_header_icon_button(icon_name::SNIPPET, "Run CLI snippet");
    snippet_button.add_css_class("tile-snippet-action");

    let close_button = build_tile_close_button(can_close);

    TerminalTileActionChrome {
        recovery_button,
        snippet_button,
        close_button,
    }
}

pub(crate) fn build_web_tile_action_chrome(can_close: bool) -> WebTileActionChrome {
    let settings_button =
        build_header_icon_button(icon_name::SETTINGS, "Edit URL and refresh settings");
    let close_button = build_tile_close_button(can_close);

    WebTileActionChrome {
        settings_button,
        close_button,
    }
}

pub(crate) fn append_terminal_tile_action_chrome(
    actions: &gtk::Box,
    chrome: &TerminalTileActionChrome,
) {
    actions.append(&chrome.recovery_button);
    actions.append(&chrome.snippet_button);
    actions.append(&chrome.close_button);
}

pub(crate) fn append_web_tile_action_chrome(actions: &gtk::Box, chrome: &WebTileActionChrome) {
    actions.append(&chrome.settings_button);
    actions.append(&chrome.close_button);
}

pub(crate) fn bind_web_tile_settings_popover(
    settings_button: &gtk::Button,
    tile_id: &str,
    get_settings: GetWebTileSettings,
    on_update_settings: Rc<dyn Fn(String, String, Option<u32>)>,
    on_reload: Rc<dyn Fn(String)>,
) {
    let settings_popover = gtk::Popover::new();
    settings_popover.add_css_class("web-tile-settings-popover");
    settings_popover.set_autohide(true);
    settings_popover.set_has_arrow(true);
    settings_popover.set_position(gtk::PositionType::Bottom);
    settings_popover.set_parent(settings_button);
    dialog_chrome::sync_popover_chrome_classes(
        settings_button,
        &settings_popover,
        "web-tile-settings-popover-window",
    );

    let settings_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    settings_box.append(&build_settings_label("URL"));

    let url_entry = gtk::Entry::builder()
        .hexpand(true)
        .placeholder_text("https://example.com")
        .css_classes(["workspace-url-entry", "web-tile-settings-entry"])
        .build();
    settings_box.append(&url_entry);

    settings_box.append(&build_settings_label("Auto-refresh (seconds)"));
    let auto_refresh = gtk::SpinButton::with_range(0.0, 3600.0, 5.0);
    auto_refresh.set_numeric(true);
    auto_refresh.set_width_chars(6);
    auto_refresh.add_css_class("tile-count-input");
    auto_refresh.set_tooltip_text(Some(
        "Auto-refresh in seconds, 0 disables automatic reload.",
    ));
    settings_box.append(&auto_refresh);

    let settings_actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    let reload_button =
        icons::labeled_button("Reload", icon_name::REFRESH, &["flat", "surface-button"]);
    reload_button.set_focus_on_click(false);
    let apply_button =
        icons::labeled_button("Apply", icon_name::APPLY, &["flat", "surface-button"]);
    apply_button.set_focus_on_click(false);
    settings_actions.append(&reload_button);
    settings_actions.append(&apply_button);
    settings_box.append(&settings_actions);
    settings_popover.set_child(Some(&settings_box));

    let tile_id = tile_id.to_string();
    let sync_settings_inputs = Rc::new({
        let url_entry = url_entry.clone();
        let auto_refresh = auto_refresh.clone();
        let get_settings = get_settings.clone();
        let tile_id = tile_id.clone();
        move || {
            let (current_url, refresh_seconds) =
                get_settings(tile_id.clone()).unwrap_or_else(|| (DEFAULT_WEB_URL.into(), None));
            url_entry.set_text(&current_url);
            auto_refresh.set_value(refresh_seconds.unwrap_or_default() as f64);
        }
    });
    {
        let sync_settings_inputs = sync_settings_inputs.clone();
        let settings_popover = settings_popover.clone();
        let url_entry = url_entry.clone();
        settings_button.connect_clicked(move |_| {
            sync_settings_inputs();
            if settings_popover.is_visible() {
                settings_popover.popdown();
            } else {
                settings_popover.popup();
                url_entry.grab_focus();
            }
        });
    }

    let apply_settings = Rc::new({
        let url_entry = url_entry.clone();
        let auto_refresh = auto_refresh.clone();
        let on_update_settings = on_update_settings.clone();
        let settings_popover = settings_popover.clone();
        let tile_id = tile_id.clone();
        move || {
            let refresh_seconds = match auto_refresh.value_as_int().max(0) {
                0 => None,
                value => Some(value as u32),
            };
            on_update_settings(
                tile_id.clone(),
                url_entry.text().to_string(),
                refresh_seconds,
            );
            settings_popover.popdown();
        }
    });
    {
        let apply_settings = apply_settings.clone();
        apply_button.connect_clicked(move |_| {
            apply_settings();
        });
    }
    {
        let apply_settings = apply_settings.clone();
        url_entry.connect_activate(move |_| {
            apply_settings();
        });
    }
    {
        let settings_popover = settings_popover.clone();
        reload_button.connect_clicked(move |_| {
            on_reload(tile_id.clone());
            settings_popover.popdown();
        });
    }
}

fn build_tile_close_button(can_close: bool) -> gtk::Button {
    let close_button = build_header_icon_button(
        icon_name::CLOSE,
        if can_close {
            "Close tile"
        } else {
            "Cannot close the last tile"
        },
    );
    close_button.set_sensitive(can_close);
    close_button
}

fn build_settings_label(label: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(label)
        .halign(gtk::Align::Start)
        .css_classes(["tile-header-popover-label"])
        .build()
}

pub(crate) fn build_tile_shell(tile: &TileSpec) -> gtk::Box {
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .hexpand(true)
        .vexpand(true)
        .css_classes(["terminal-card", tile.accent_class.as_str()])
        .build();
    make_shrinkable(&shell);
    shell
}

pub(crate) fn build_tile_frame(css_class: &str) -> gtk::Box {
    let frame = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .hexpand(true)
        .vexpand(true)
        .css_classes([css_class])
        .build();
    make_shrinkable(&frame);
    frame
}

pub(crate) fn build_tile_header_chrome(input: TileHeaderInput<'_>) -> TileHeaderChrome {
    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["terminal-header"])
        .build();
    make_shrinkable(&header);

    let drag_handle = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    make_shrinkable(&drag_handle);
    drag_handle.set_tooltip_text(Some(input.drag_tooltip));

    let badge = gtk::Label::builder()
        .label(input.badge_text)
        .halign(gtk::Align::Start)
        .css_classes(["agent-badge"])
        .build();
    configure_dynamic_header_label(
        &badge,
        input.badge_tooltip,
        input.badge_max_chars,
        gtk::pango::EllipsizeMode::End,
    );
    drag_handle.append(&badge);

    let title_label = gtk::Label::builder()
        .label(&input.tile.title)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .css_classes(["tile-title"])
        .build();
    configure_dynamic_header_label(
        &title_label,
        &input.tile.title,
        HEADER_TITLE_MAX_CHARS,
        gtk::pango::EllipsizeMode::End,
    );
    drag_handle.append(&title_label);

    if let Some(pane_group_label) = build_pane_group_chip(&input.tile.pane_groups) {
        drag_handle.append(&pane_group_label);
    }

    let status_label = gtk::Label::builder()
        .label(input.status_text)
        .valign(gtk::Align::Center)
        .css_classes(["status-chip"])
        .build();
    configure_dynamic_header_label(
        &status_label,
        input.status_tooltip,
        HEADER_STATUS_MAX_CHARS,
        input.status_ellipsize,
    );

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    actions.append(&status_label);

    header.append(&drag_handle);
    header.append(&actions);

    TileHeaderChrome {
        widget: header,
        drag_handle,
        actions,
        title_label,
        status_label,
    }
}

pub(crate) fn configure_dynamic_header_label(
    label: &gtk::Label,
    full_text: &str,
    max_width_chars: i32,
    ellipsize: gtk::pango::EllipsizeMode,
) {
    label.set_ellipsize(ellipsize);
    label.set_max_width_chars(max_width_chars);
    label.set_single_line_mode(true);
    label.set_tooltip_text(Some(full_text));
}

pub(crate) fn build_pane_group_chip(pane_groups: &[String]) -> Option<gtk::Label> {
    if pane_groups.is_empty() {
        return None;
    }

    let pane_groups = pane_groups.join(", ");
    let pane_group_label = gtk::Label::builder()
        .label(&pane_groups)
        .halign(gtk::Align::Start)
        .css_classes(["status-chip", "muted-chip"])
        .build();
    configure_dynamic_header_label(
        &pane_group_label,
        &pane_groups,
        HEADER_GROUP_MAX_CHARS,
        gtk::pango::EllipsizeMode::End,
    );
    pane_group_label.set_tooltip_text(Some(&format!("Pane groups: {pane_groups}")));

    Some(pane_group_label)
}

pub(crate) fn domain_from_url(url: &str) -> String {
    url.split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or(url)
        .to_string()
}

pub(crate) fn make_shrinkable<W: gtk::glib::object::IsA<gtk::Widget>>(widget: &W) {
    widget.set_size_request(0, 0);
    widget.set_overflow(gtk::Overflow::Hidden);
}
