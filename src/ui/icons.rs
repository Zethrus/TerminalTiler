use std::path::PathBuf;

use gtk::prelude::*;

/// Native GTK symbolic icons selected to match the action vocabulary from
/// itsHover's animated catalog (terminal, dashboard, save, refresh, copy,
/// trash, arrows, etc.) while keeping TerminalTiler dependency-free and
/// theme-recolorable on GTK/libadwaita.
pub(crate) mod name {
    pub(crate) const ADD: &str = "list-add-symbolic";
    pub(crate) const ALERTS: &str = "hover:triangle-alert-icon";
    pub(crate) const APPLY: &str = "hover:checked-icon";
    pub(crate) const ASSETS: &str = "folder-saved-search-symbolic";
    pub(crate) const BACK: &str = "hover:arrow-back-icon";
    pub(crate) const BROADCAST: &str = "hover:send-horizontal-icon";
    pub(crate) const CLOSE: &str = "hover:x-icon";
    pub(crate) const COPY: &str = "hover:copy-icon";
    pub(crate) const DELETE: &str = "hover:trash-icon";
    pub(crate) const EDIT: &str = "document-edit-symbolic";
    pub(crate) const FOLDER: &str = "folder-open-symbolic";
    pub(crate) const FULLSCREEN: &str = "view-fullscreen-symbolic";
    pub(crate) const KEYBOARD: &str = "input-keyboard-symbolic";
    pub(crate) const LAUNCH: &str = "hover:player-icon";
    pub(crate) const LAYOUT: &str = "hover:layout-dashboard-icon";
    pub(crate) const NEXT: &str = "hover:arrow-narrow-right-icon";
    pub(crate) const OPEN: &str = "document-open-symbolic";
    pub(crate) const RECORD: &str = "media-record-symbolic";
    pub(crate) const RECOVER: &str = "system-run-symbolic";
    pub(crate) const REFRESH: &str = "hover:refresh-icon";
    pub(crate) const RESET: &str = "edit-clear-all-symbolic";
    pub(crate) const RESTORE: &str = "view-restore-symbolic";
    pub(crate) const RUN: &str = "system-run-symbolic";
    pub(crate) const SAVE: &str = "hover:save-icon";
    pub(crate) const SEARCH: &str = "system-search-symbolic";
    pub(crate) const SETTINGS: &str = "preferences-system-symbolic";
    pub(crate) const SNIPPET: &str = "insert-text-symbolic";
    pub(crate) const TERMINAL: &str = "hover:terminal-icon";
    pub(crate) const THEME: &str = "preferences-desktop-appearance-symbolic";
    pub(crate) const WEB: &str = "hover:external-link-icon";
    pub(crate) const WORKSPACES: &str = "hover:layout-dashboard-icon";
}

pub(crate) fn prime_button_icon(button: &gtk::Button) {
    if let Some(child) = button.first_child() {
        let _ = child.pango_context();
    }
}

pub(crate) fn icon_button(icon_name: &str, tooltip: &str, css_classes: &[&str]) -> gtk::Button {
    let button = gtk::Button::builder().focus_on_click(false).build();
    for class_name in css_classes {
        button.add_css_class(class_name);
    }
    let icon = image(icon_name);
    icon.set_pixel_size(15);
    icon.set_valign(gtk::Align::Center);
    icon.add_css_class("button-leading-icon");
    button.set_child(Some(&icon));
    button.set_tooltip_text(Some(tooltip));
    prime_button_icon(&button);
    button
}

pub(crate) fn labeled_button(label: &str, icon_name: &str, css_classes: &[&str]) -> gtk::Button {
    let button = gtk::Button::builder().build();
    for class_name in css_classes {
        button.add_css_class(class_name);
    }
    set_button_icon_label(&button, label, icon_name);
    button
}

pub(crate) fn set_button_icon_label(button: &gtk::Button, label: &str, icon_name: &str) {
    set_button_icon_label_with_alignment(button, label, icon_name, gtk::Align::Center);
}

pub(crate) fn set_button_icon_label_start(button: &gtk::Button, label: &str, icon_name: &str) {
    set_button_icon_label_with_alignment(button, label, icon_name, gtk::Align::Start);
}

fn set_button_icon_label_with_alignment(
    button: &gtk::Button,
    label: &str,
    icon_name: &str,
    halign: gtk::Align,
) {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .halign(halign)
        .valign(gtk::Align::Center)
        .css_classes(["button-icon-label-content"])
        .build();

    let icon = image(icon_name);
    icon.set_pixel_size(15);
    icon.set_valign(gtk::Align::Center);
    icon.add_css_class("button-leading-icon");
    let _ = icon.pango_context();
    row.append(&icon);

    row.append(
        &gtk::Label::builder()
            .label(label)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Center)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["button-text-label"])
            .build(),
    );

    button.set_child(Some(&row));
}

pub(crate) fn image(icon_name: &str) -> gtk::Image {
    if let Some(path) = hover_icon_path(icon_name)
        && path.exists()
    {
        gtk::Image::from_file(path)
    } else {
        gtk::Image::from_icon_name(icon_name)
    }
}

fn hover_icon_path(icon_name: &str) -> Option<PathBuf> {
    let file_name = match icon_name {
        "hover:arrow-back-icon" => Some("arrow-back.svg"),
        "hover:arrow-narrow-right-icon" => Some("arrow-narrow-right.svg"),
        "hover:checked-icon" => Some("checked.svg"),
        "hover:copy-icon" => Some("copy.svg"),
        "hover:external-link-icon" => Some("external-link.svg"),
        "hover:layout-dashboard-icon" => Some("layout-dashboard.svg"),
        "hover:player-icon" => Some("player.svg"),
        "hover:refresh-icon" => Some("refresh.svg"),
        "hover:save-icon" => Some("save.svg"),
        "hover:send-horizontal-icon" => Some("send-horizontal.svg"),
        "hover:terminal-icon" => Some("terminal.svg"),
        "hover:trash-icon" => Some("trash.svg"),
        "hover:triangle-alert-icon" => Some("triangle-alert.svg"),
        "hover:x-icon" => Some("x.svg"),
        _ => None,
    }?;
    let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources/hover-icons")
        .join(file_name);
    if manifest_path.exists() {
        return Some(manifest_path);
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        let portable_path = exe_dir.join("share/hover-icons").join(file_name);
        if portable_path.exists() {
            return Some(portable_path);
        }
        if let Some(app_root) = exe_dir.parent() {
            return Some(app_root.join("share/hover-icons").join(file_name));
        }
    }

    Some(PathBuf::from("/usr/share/terminaltiler/hover-icons").join(file_name))
}
