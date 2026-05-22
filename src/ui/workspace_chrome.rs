use gtk::glib;
use gtk::prelude::*;

use crate::ui::icons::{self, name as icon_name};

pub(crate) struct WorkspaceSummaryInput<'a> {
    pub(crate) name: &'a str,
    pub(crate) path: String,
    pub(crate) pane_groups: Vec<String>,
    pub(crate) controls_sensitive: bool,
}

pub(crate) struct WorkspaceSummaryChrome {
    pub(crate) widget: gtk::Widget,
    pub(crate) alert_button: gtk::Button,
    pub(crate) broadcast_state: gtk::Label,
    pub(crate) broadcast_selector: gtk::ComboBoxText,
    pub(crate) broadcast_entry: gtk::Entry,
    pub(crate) broadcast_button: gtk::Button,
    pub(crate) add_web_tile_button: gtk::Button,
    pub(crate) url_entry: gtk::Entry,
    pub(crate) url_reload_button: gtk::Button,
    pub(crate) runbook_selector: gtk::ComboBoxText,
    pub(crate) runbook_button: gtk::Button,
    pub(crate) path_label: gtk::Label,
}

pub(crate) fn build_workspace_shell_chrome() -> gtk::Box {
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
    shell
}

pub(crate) fn build_workspace_content_chrome(
    layout_host: &impl glib::object::IsA<gtk::Widget>,
    alert_revealer: &impl glib::object::IsA<gtk::Widget>,
) -> gtk::Widget {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .hexpand(true)
        .vexpand(true)
        .build();
    content.append(layout_host);
    content.append(alert_revealer);
    content.upcast()
}

pub(crate) fn build_workspace_alert_revealer(
    alert_sidebar: &impl glib::object::IsA<gtk::Widget>,
) -> gtk::Revealer {
    let alert_revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideLeft)
        .reveal_child(false)
        .build();
    alert_revealer.set_child(Some(alert_sidebar));
    alert_revealer
}

pub(crate) fn build_workspace_summary_chrome(
    input: WorkspaceSummaryInput<'_>,
) -> WorkspaceSummaryChrome {
    let summary = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(["workspace-summary"])
        .build();

    let name_label = gtk::Label::builder()
        .label(input.name)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["workspace-summary-name"])
        .build();
    make_shrinkable(&name_label);

    let alert_button =
        icons::labeled_button("Alerts (0)", icon_name::ALERTS, &["flat", "surface-button"]);
    alert_button.set_sensitive(input.controls_sensitive);

    let broadcast_state = gtk::Label::builder()
        .label("Broadcast Off")
        .valign(gtk::Align::Center)
        .css_classes(["status-chip", "muted-chip"])
        .build();

    let broadcast_selector = gtk::ComboBoxText::new();
    broadcast_selector.add_css_class("surface-select-control");
    broadcast_selector.append(Some("off"), "Broadcast Off");
    broadcast_selector.append(Some("all"), "Broadcast All");
    for group in input.pane_groups {
        let id = format!("group:{group}");
        broadcast_selector.append(Some(&id), &format!("Group: {group}"));
    }
    broadcast_selector.set_active_id(Some("off"));
    broadcast_selector.set_sensitive(input.controls_sensitive);

    let broadcast_entry = gtk::Entry::builder()
        .placeholder_text("Quick send command")
        .width_chars(18)
        .css_classes(["workspace-broadcast-entry"])
        .sensitive(input.controls_sensitive)
        .build();
    let broadcast_button =
        icons::labeled_button("Send", icon_name::BROADCAST, &["flat", "surface-button"]);
    broadcast_button.set_sensitive(input.controls_sensitive);

    let add_web_tile_button =
        icons::labeled_button("Add Web Tile", icon_name::WEB, &["flat", "surface-button"]);
    add_web_tile_button.set_sensitive(input.controls_sensitive);

    let url_entry = gtk::Entry::builder()
        .placeholder_text("URL")
        .width_chars(30)
        .hexpand(false)
        .css_classes(["workspace-url-entry"])
        .sensitive(input.controls_sensitive)
        .build();
    let url_reload_button =
        icons::labeled_button("Reload", icon_name::REFRESH, &["flat", "surface-button"]);
    url_reload_button.set_sensitive(input.controls_sensitive);

    let runbook_selector = gtk::ComboBoxText::new();
    runbook_selector.add_css_class("surface-select-control");
    runbook_selector.append(Some(""), "Runbook");
    runbook_selector.set_active_id(Some(""));
    runbook_selector.set_sensitive(input.controls_sensitive);
    let runbook_button = icons::labeled_button("Run", icon_name::RUN, &["flat", "surface-button"]);
    runbook_button.set_sensitive(input.controls_sensitive);

    let path_label = gtk::Label::builder()
        .label(input.path)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::Start)
        .css_classes(["workspace-summary-path"])
        .build();
    make_shrinkable(&path_label);

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

    WorkspaceSummaryChrome {
        widget: summary.upcast(),
        alert_button,
        broadcast_state,
        broadcast_selector,
        broadcast_entry,
        broadcast_button,
        add_web_tile_button,
        url_entry,
        url_reload_button,
        runbook_selector,
        runbook_button,
        path_label,
    }
}

fn make_shrinkable<W: glib::object::IsA<gtk::Widget>>(widget: &W) {
    widget.set_size_request(0, 0);
    widget.set_overflow(gtk::Overflow::Hidden);
}
