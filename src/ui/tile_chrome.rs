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
