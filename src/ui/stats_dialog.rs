//! Usage statistics dialog.
//!
//! Presents a [`StatsSnapshot`] (see [`crate::services::stats`]) grouped into
//! Today / This Week / Lifetime, with an explicit destructive reset action.

use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::services::stats::StatsSnapshot;
use crate::stats_hub;
use crate::ui::dialog_chrome;
use crate::ui::icons::{self, name as icon_name};

const STATS_DIALOG_TITLE: &str = "Usage Statistics";

pub fn present_shared(window: &adw::ApplicationWindow) {
    present(
        window,
        stats_hub::recorder().snapshot(),
        Rc::new(|| {
            stats_hub::reset();
            stats_hub::recorder().snapshot()
        }),
    );
}

fn present(
    window: &adw::ApplicationWindow,
    snapshot: StatsSnapshot,
    on_reset: Rc<dyn Fn() -> StatsSnapshot>,
) {
    let dialog = adw::Dialog::new();
    dialog.set_title(STATS_DIALOG_TITLE);
    dialog.set_follows_content_size(false);
    dialog.set_content_width(460);
    dialog.set_content_height(560);
    dialog_chrome::sync_dialog_chrome_classes(window, &dialog, "stats-dialog-window");

    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .vexpand(true)
        .build();

    let scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .css_classes(["settings-dialog-scroller", "stats-dialog-scroller"])
        .build();
    scroller.set_has_frame(false);
    root.append(&scroller);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .css_classes(["settings-dialog-content", "stats-dialog-content"])
        .build();
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    scroller.set_child(Some(&content));

    replace_stats_sections(&content, snapshot);

    let reset_button = icons::labeled_button(
        "Reset Statistics",
        icon_name::RESET,
        &["pill-button", "destructive-button", "stats-reset-button"],
    );
    {
        let window = window.clone();
        let content = content.clone();
        let on_reset = on_reset.clone();
        reset_button.connect_clicked(move |_| {
            let content = content.clone();
            let on_reset = on_reset.clone();
            dialog_chrome::confirm_destructive_action(
                &window,
                "Reset Usage Statistics?",
                "This clears usage statistics for today, this week, and all time. This cannot be undone.",
                "Reset",
                move || {
                    let snapshot = on_reset();
                    replace_stats_sections(&content, snapshot);
                },
            );
        });
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
    footer.append(&reset_button);
    footer.append(&gtk::Box::builder().hexpand(true).build());
    footer.append(&close_button);
    root.append(&footer);

    dialog.set_child(Some(&root));
    dialog.set_default_widget(Some(&close_button));
    dialog.present(Some(window));
}

/// Replace all stat cards with values from `snapshot`.
fn replace_stats_sections(content: &gtk::Box, snapshot: StatsSnapshot) {
    while let Some(child) = content.first_child() {
        content.remove(&child);
    }

    content.append(&build_section(
        "Today",
        "Since local midnight",
        &[
            metric("Characters typed", &group_thousands(snapshot.chars_today)),
            metric("Words typed", &group_thousands(snapshot.words_today)),
            metric("Average WPM", &format_wpm(snapshot.today_wpm)),
            metric(
                "Active typing time",
                &format_minutes(snapshot.today_active_minutes),
            ),
        ],
    ));

    content.append(&build_section(
        "This Week",
        "Last 7 days",
        &[
            metric("Characters typed", &group_thousands(snapshot.chars_week)),
            metric("Words typed", &group_thousands(snapshot.words_week)),
            metric("Average WPM", &format_wpm(snapshot.week_wpm)),
        ],
    ));

    content.append(&build_section(
        "Lifetime",
        "All time",
        &[
            metric("Total characters", &group_thousands(snapshot.total_chars)),
            metric("Total words", &group_thousands(snapshot.total_words)),
            metric("Average WPM", &format_wpm(snapshot.avg_wpm)),
            metric(
                "Active typing time",
                &format_minutes(snapshot.total_active_minutes),
            ),
        ],
    ));
}

/// A titled card holding a list of `label: value` metric rows.
fn build_section(title: &str, meta: &str, rows: &[gtk::Widget]) -> gtk::Widget {
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["config-panel", "settings-section", "stats-section"])
        .build();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .css_classes(["settings-section-top"])
        .build();
    header.append(
        &gtk::Label::builder()
            .label(title)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .css_classes([
                "eyebrow",
                "settings-section-heading",
                "stats-section-heading",
            ])
            .build(),
    );
    header.append(
        &gtk::Label::builder()
            .label(meta)
            .halign(gtk::Align::End)
            .css_classes(["status-chip", "settings-meta-chip"])
            .build(),
    );
    shell.append(&header);

    for row in rows {
        shell.append(row);
    }

    shell.upcast()
}

/// One `label .... value` row with the value emphasized.
fn metric(label: &str, value: &str) -> gtk::Widget {
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
            .label(value)
            .halign(gtk::Align::End)
            .css_classes(["stats-metric-value"])
            .build(),
    );
    row.upcast()
}

fn format_wpm(wpm: f64) -> String {
    format!("{wpm:.1}")
}

fn format_minutes(minutes: f64) -> String {
    if minutes >= 60.0 {
        let hours = minutes / 60.0;
        format!("{hours:.1} h")
    } else {
        format!("{minutes:.1} min")
    }
}

/// Group an integer with thousands separators, e.g. `1234567` -> `1,234,567`.
fn group_thousands(value: u64) -> String {
    let digits = value.to_string();
    let bytes = digits.as_bytes();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let len = bytes.len();
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 && (len - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*byte as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{format_minutes, format_wpm, group_thousands};

    #[test]
    fn groups_thousands() {
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(42), "42");
        assert_eq!(group_thousands(1_000), "1,000");
        assert_eq!(group_thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn formats_wpm_and_minutes() {
        assert_eq!(format_wpm(72.345), "72.3");
        assert_eq!(format_minutes(12.34), "12.3 min");
        assert_eq!(format_minutes(90.0), "1.5 h");
    }
}
