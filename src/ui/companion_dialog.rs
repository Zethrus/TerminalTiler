use std::sync::Arc;

use adw::prelude::*;
use gtk::gio;

use crate::extension::{
    CompanionAction, CompanionActionInput, CompanionActionStyle, CompanionIntegration,
    CompanionPanelSnapshot, CompanionRow, CompanionTextInput,
};
use crate::logging;
use crate::ui::dialog_chrome;
use crate::ui::icons::{self, name as icon_name};

pub fn present(window: &adw::ApplicationWindow, companion: Arc<dyn CompanionIntegration>) {
    let snapshot = companion.snapshot();
    let dialog = adw::Dialog::new();
    dialog.set_title(&snapshot.title);
    dialog.set_follows_content_size(false);
    dialog.set_content_width(640);
    dialog.set_content_height(620);
    dialog_chrome::sync_dialog_chrome_classes(window, &dialog, "companion-dialog-window");

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();

    content.append(
        &gtk::Label::builder()
            .label(&snapshot.title)
            .halign(gtk::Align::Start)
            .css_classes(["section-title"])
            .build(),
    );
    content.append(
        &gtk::Label::builder()
            .label(format!(
                "{} · {}",
                snapshot.status.label(),
                snapshot.subtitle
            ))
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint"])
            .build(),
    );

    append_section(&content, "Account", &snapshot.account_rows);
    append_section(&content, "Sync", &snapshot.sync_rows);
    append_section(&content, "Devices and teams", &snapshot.device_rows);
    append_actions(&content, window, &dialog, companion, &snapshot);

    let close_button = icons::labeled_button("Close", icon_name::CLOSE, &["pill-button", "flat"]);
    close_button.set_halign(gtk::Align::End);
    content.append(&close_button);

    dialog.set_child(Some(&content));
    dialog.set_default_widget(Some(&close_button));
    {
        let dialog = dialog.clone();
        close_button.connect_clicked(move |_| {
            dialog.close();
        });
    }

    dialog.present(Some(window));
}

fn append_section(content: &gtk::Box, title: &str, rows: &[CompanionRow]) {
    if rows.is_empty() {
        return;
    }
    content.append(
        &gtk::Label::builder()
            .label(title)
            .halign(gtk::Align::Start)
            .css_classes(["card-title"])
            .build(),
    );
    let list = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["card"])
        .build();
    for row in rows {
        let item = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .halign(gtk::Align::Fill)
            .hexpand(true)
            .build();
        item.append(
            &gtk::Label::builder()
                .label(format!("{}: {}", row.label, row.value))
                .halign(gtk::Align::Start)
                .wrap(true)
                .build(),
        );
        if let Some(detail) = &row.detail {
            item.append(
                &gtk::Label::builder()
                    .label(detail)
                    .halign(gtk::Align::Start)
                    .wrap(true)
                    .css_classes(["field-hint"])
                    .build(),
            );
        }
        list.append(&item);
    }
    content.append(&list);
}

fn append_actions(
    content: &gtk::Box,
    window: &adw::ApplicationWindow,
    dialog: &adw::Dialog,
    companion: Arc<dyn CompanionIntegration>,
    snapshot: &CompanionPanelSnapshot,
) {
    if snapshot.actions.is_empty() {
        return;
    }
    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::Start)
        .build();
    for action in &snapshot.actions {
        let button = icons::labeled_button(&action.label, icon_name::NEXT, &button_classes(action));
        let action = action.clone();
        let companion = companion.clone();
        let parent = window.clone();
        let parent_dialog = dialog.clone();
        button.connect_clicked(move |_| {
            invoke_action(&parent, &parent_dialog, companion.clone(), action.clone());
        });
        actions.append(&button);
    }
    content.append(&actions);
}

fn button_classes(action: &CompanionAction) -> Vec<&'static str> {
    match action.style {
        CompanionActionStyle::Primary => vec!["pill-button", "suggested-action"],
        CompanionActionStyle::Destructive => vec!["pill-button", "destructive-action"],
        CompanionActionStyle::Normal => vec!["pill-button", "flat"],
    }
}

fn invoke_action(
    window: &adw::ApplicationWindow,
    dialog: &adw::Dialog,
    companion: Arc<dyn CompanionIntegration>,
    action: CompanionAction,
) {
    if let Some(url) = action.external_url.as_deref() {
        if let Err(error) =
            gio::AppInfo::launch_default_for_uri(url, None::<&gio::AppLaunchContext>)
        {
            logging::error(format!("failed to open companion URL '{}': {}", url, error));
        }
        return;
    }

    if let Some(input) = action.input.clone() {
        present_input_prompt(window, companion, action, input);
        dialog.close();
        return;
    }

    match companion.invoke(&action.id, CompanionActionInput::default()) {
        Ok(result) => {
            logging::info(format!(
                "companion action '{}' completed: {}",
                action.id, result.message
            ));
            dialog.close();
            present(window, companion);
        }
        Err(error) => logging::error(format!(
            "companion action '{}' failed: {}",
            action.id, error
        )),
    }
}

fn present_input_prompt(
    window: &adw::ApplicationWindow,
    companion: Arc<dyn CompanionIntegration>,
    action: CompanionAction,
    input: CompanionTextInput,
) {
    let dialog = adw::Dialog::new();
    dialog.set_title(&action.label);
    dialog.set_content_width(520);
    dialog_chrome::sync_dialog_chrome_classes(window, &dialog, "companion-input-dialog-window");
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    content.append(
        &gtk::Label::builder()
            .label(&input.prompt)
            .wrap(true)
            .halign(gtk::Align::Start)
            .build(),
    );
    let entry = gtk::Entry::builder()
        .placeholder_text(input.placeholder.as_deref().unwrap_or(""))
        .visibility(!input.secret)
        .hexpand(true)
        .build();
    content.append(&entry);
    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .build();
    let cancel = icons::labeled_button("Cancel", icon_name::CLOSE, &["pill-button", "flat"]);
    let submit = icons::labeled_button(
        &action.label,
        icon_name::NEXT,
        &["pill-button", "suggested-action"],
    );
    actions.append(&cancel);
    actions.append(&submit);
    content.append(&actions);
    dialog.set_child(Some(&content));
    dialog.set_default_widget(Some(&submit));
    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| {
            dialog.close();
        });
    }
    {
        let companion = companion.clone();
        let parent = window.clone();
        let dialog = dialog.clone();
        let entry_for_submit = entry.clone();
        let action_for_submit = action.clone();
        submit.connect_clicked(move |_| {
            let text = entry_for_submit.text().trim().to_string();
            match companion.invoke(
                &action_for_submit.id,
                CompanionActionInput { text: Some(text) },
            ) {
                Ok(_) => {
                    dialog.close();
                    present(&parent, companion.clone());
                }
                Err(error) => logging::error(format!(
                    "companion action '{}' failed: {}",
                    action_for_submit.id, error
                )),
            }
        });
    }
    {
        let submit = submit.clone();
        entry.connect_activate(move |_| submit.emit_clicked());
    }
    dialog.present(Some(window));
    entry.grab_focus();
}
