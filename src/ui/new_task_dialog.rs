//! Modal for creating a Kanban task. Mirrors `tab_rename_dialog` so it inherits the
//! app's dialog chrome (theme + density) and Enter-to-submit behaviour.

use std::rc::Rc;

use adw::prelude::*;

use crate::model::board::TaskStatus;
use crate::ui::dialog_chrome;
use crate::ui::dialog_form;
use crate::ui::icons::{self, name as icon_name};

/// Present the dialog. `on_submit` receives `(title, description, status)` when applied
/// with a non-empty title.
pub(crate) fn present<F>(window: &adw::ApplicationWindow, on_submit: F)
where
    F: Fn(String, String, TaskStatus) + 'static,
{
    let dialog = adw::Dialog::new();
    dialog.set_title("New Task");
    dialog.set_content_width(440);
    dialog_chrome::sync_dialog_chrome_classes(window, &dialog, "new-task-dialog-window");

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();

    let title_entry = dialog_form::text_input("Task title");
    content.append(&dialog_form::field_label("Title"));
    content.append(&title_entry);

    let (description_scroller, description_view) = dialog_form::multiline_input(96);
    content.append(&dialog_form::field_label("Description"));
    content.append(&description_scroller);

    let status_labels: Vec<&str> = TaskStatus::ALL.iter().map(|s| s.column_title()).collect();
    let status_model = gtk::StringList::new(&status_labels);
    let status_dropdown = gtk::DropDown::builder()
        .model(&status_model)
        .css_classes(["dialog-select"])
        .build();
    content.append(&dialog_form::field_label("Column"));
    content.append(&status_dropdown);

    let action_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .build();
    let cancel_button = icons::labeled_button("Cancel", icon_name::CLOSE, &["pill-button", "flat"]);
    let create_button = icons::labeled_button(
        "Create",
        icon_name::ADD,
        &["pill-button", "suggested-action"],
    );
    action_row.append(&cancel_button);
    action_row.append(&create_button);
    content.append(&action_row);

    dialog.set_child(Some(&content));
    dialog.set_default_widget(Some(&create_button));

    {
        let dialog = dialog.clone();
        cancel_button.connect_clicked(move |_| {
            dialog.close();
        });
    }

    let on_submit = Rc::new(on_submit);
    {
        let dialog = dialog.clone();
        let title_entry = title_entry.clone();
        let description_view = description_view.clone();
        let status_dropdown = status_dropdown.clone();
        let on_submit = on_submit.clone();
        create_button.connect_clicked(move |_| {
            let title = title_entry.text().trim().to_string();
            if title.is_empty() {
                title_entry.grab_focus();
                return;
            }
            let buffer = description_view.buffer();
            let (start, end) = buffer.bounds();
            let description = buffer.text(&start, &end, false).trim().to_string();
            let index = status_dropdown.selected() as usize;
            let status = TaskStatus::ALL
                .get(index)
                .copied()
                .unwrap_or(TaskStatus::Todo);
            on_submit(title, description, status);
            dialog.close();
        });
    }

    dialog.present(Some(window));
    title_entry.grab_focus();
}
