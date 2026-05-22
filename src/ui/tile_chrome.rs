use crate::model::layout::TileSpec;
use crate::ui::icons;
use gtk::prelude::*;

pub(crate) const TERMINAL_HEADER_BADGE_MAX_CHARS: i32 = 12;
pub(crate) const WEB_HEADER_BADGE_MAX_CHARS: i32 = 4;
pub(crate) const HEADER_GROUP_MAX_CHARS: i32 = 16;
pub(crate) const HEADER_STATUS_MAX_CHARS: i32 = 28;
pub(crate) const HEADER_TITLE_MAX_CHARS: i32 = 28;

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
