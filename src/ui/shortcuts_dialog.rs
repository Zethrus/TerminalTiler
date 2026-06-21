//! Keyboard-shortcut cheat-sheet dialog.
//!
//! Lists the app's active accelerators grouped by area. Configurable shortcuts
//! are passed in by the caller (read from the preference store); fixed ones
//! (terminal copy/paste, quick send, pane maximize) are listed for discovery.
//! Mirrors the presentation of [`crate::ui::stats_dialog`].

use adw::prelude::*;
use gtk::glib;

use crate::ui::dialog_chrome;
use crate::ui::icons::{self, name as icon_name};

const SHORTCUTS_DIALOG_TITLE: &str = "Keyboard Shortcuts";

/// Default accelerator for the maximize/restore-pane action. Single source of
/// truth shared by the shortcut installer and the cheat sheet.
pub const DEFAULT_MAXIMIZE_ACCEL: &str = "<Ctrl><Shift>M";

/// Default accelerator for the add-terminal-tile action. Single source of truth
/// shared by the shortcut installer and the cheat sheet.
pub const DEFAULT_ADD_TERMINAL_TILE_ACCEL: &str = "<Ctrl><Shift>Return";

/// The configurable accelerators (read from the preference store) needed to
/// render the cheat sheet. Fixed shortcuts (copy/paste, quick send) are added
/// by [`sections_from_summary`].
pub struct ShortcutSummary {
    pub fullscreen: String,
    pub density: String,
    pub zoom_in: String,
    pub zoom_out: String,
    pub command_palette: String,
    pub maximize: String,
    pub add_terminal_tile: String,
}

/// Build the grouped cheat-sheet rows from a [`ShortcutSummary`] plus the fixed
/// terminal/quick-send shortcuts. Shared by the Linux and Windows GTK shells.
pub fn sections_from_summary(summary: &ShortcutSummary) -> Vec<ShortcutSection> {
    vec![
        ShortcutSection::new(
            "Workspace",
            vec![
                ShortcutRow::new("Toggle fullscreen", summary.fullscreen.clone()),
                ShortcutRow::new("Cycle density", summary.density.clone()),
                ShortcutRow::new("Zoom in text", summary.zoom_in.clone()),
                ShortcutRow::new("Zoom out text", summary.zoom_out.clone()),
                ShortcutRow::new("Maximize / restore pane", summary.maximize.clone()),
                ShortcutRow::new("Add terminal tile", summary.add_terminal_tile.clone()),
            ],
        ),
        ShortcutSection::new(
            "Command palette",
            vec![ShortcutRow::new(
                "Open command palette",
                summary.command_palette.clone(),
            )],
        ),
        ShortcutSection::new(
            "Terminal",
            vec![
                ShortcutRow::new("Copy selection", "<Ctrl><Shift>C"),
                ShortcutRow::new("Paste", "<Ctrl><Shift>V"),
                ShortcutRow::new("Quick send command", "Enter"),
            ],
        ),
    ]
}

/// A single shortcut entry: a human label and its accelerator in GTK form
/// (e.g. `<Ctrl><Shift>P`). The accelerator is rendered as a key chip.
pub struct ShortcutRow {
    pub label: String,
    pub accel: String,
}

impl ShortcutRow {
    pub fn new(label: impl Into<String>, accel: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            accel: accel.into(),
        }
    }
}

/// A titled group of shortcut rows.
pub struct ShortcutSection {
    pub title: String,
    pub rows: Vec<ShortcutRow>,
}

impl ShortcutSection {
    pub fn new(title: impl Into<String>, rows: Vec<ShortcutRow>) -> Self {
        Self {
            title: title.into(),
            rows,
        }
    }
}

pub fn present(window: &adw::ApplicationWindow, sections: Vec<ShortcutSection>) {
    let dialog = adw::Dialog::new();
    dialog.set_title(SHORTCUTS_DIALOG_TITLE);
    dialog.set_follows_content_size(false);
    dialog.set_content_width(460);
    dialog.set_content_height(560);
    dialog_chrome::sync_dialog_chrome_classes(window, &dialog, "shortcuts-dialog-window");

    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .vexpand(true)
        .build();

    let scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .css_classes(["settings-dialog-scroller"])
        .build();
    scroller.set_has_frame(false);
    root.append(&scroller);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .css_classes(["settings-dialog-content"])
        .build();
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    scroller.set_child(Some(&content));

    for section in &sections {
        content.append(&build_section(section));
    }

    let close_button = icons::labeled_button(
        "Close",
        icon_name::CLOSE,
        &["pill-button", "ghost-link-button", "settings-close-button"],
    );
    {
        let dialog = dialog.clone();
        close_button.connect_clicked(move |_| {
            let dialog = dialog.clone();
            glib::idle_add_local_once(move || {
                dialog.close();
            });
        });
    }

    let footer = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();
    footer.append(&gtk::Box::builder().hexpand(true).build());
    footer.append(&close_button);
    root.append(&footer);

    dialog.set_child(Some(&root));
    dialog.set_default_widget(Some(&close_button));
    dialog.present(Some(window));
}

/// A titled card holding a list of `label: accelerator` rows.
fn build_section(section: &ShortcutSection) -> gtk::Widget {
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["config-panel", "settings-section"])
        .build();

    shell.append(
        &gtk::Label::builder()
            .label(&section.title)
            .halign(gtk::Align::Start)
            .css_classes(["eyebrow", "settings-section-heading"])
            .build(),
    );

    for row in &section.rows {
        shell.append(&shortcut_row(&row.label, &row.accel));
    }

    shell.upcast()
}

/// One `label .... accelerator` row with the accelerator shown as a key chip.
fn shortcut_row(label: &str, accel: &str) -> gtk::Widget {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(["stats-metric-row"])
        .build();
    row.append(
        &gtk::Label::builder()
            .label(label)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .css_classes(["settings-shortcut-title"])
            .build(),
    );
    row.append(
        &gtk::Label::builder()
            .label(humanize_accelerator(accel))
            .halign(gtk::Align::End)
            .css_classes(["status-chip", "settings-meta-chip"])
            .build(),
    );
    row.upcast()
}

/// Render a GTK accelerator string (`<Ctrl><Shift>P`) as a readable label
/// (`Ctrl+Shift+P`). Falls back to the raw string when it cannot be parsed
/// (e.g. the synthetic `Enter` placeholder for the quick-send entry) and shows
/// `Unset` for an empty accelerator.
fn humanize_accelerator(accel: &str) -> String {
    let trimmed = accel.trim();
    if trimmed.is_empty() {
        return "Unset".to_string();
    }
    match gtk::ShortcutTrigger::parse_string(trimmed) {
        Some(trigger) => trigger.to_str().to_string(),
        None => trimmed.to_string(),
    }
}
