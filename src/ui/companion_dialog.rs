use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::{Arc, mpsc};
use std::time::Instant;

use adw::prelude::*;
use gtk::gio;

use crate::extension::{
    CompanionAction, CompanionActionInput, CompanionActionStyle, CompanionIntegration,
    CompanionPanelSnapshot, CompanionRefreshScope, CompanionRow, CompanionStatus,
    CompanionTextInput,
};
use crate::logging;
use crate::ui::dialog_chrome;
use crate::ui::dialog_smoke;
use crate::ui::icons::{self, name as icon_name};

pub fn present(window: &adw::ApplicationWindow, companion: Arc<dyn CompanionIntegration>) {
    present_with_notice(window, companion, None);
}

fn present_with_notice(
    window: &adw::ApplicationWindow,
    companion: Arc<dyn CompanionIntegration>,
    notice: Option<String>,
) {
    let snapshot = companion.snapshot();
    let dialog = adw::Dialog::new();
    dialog.set_title(&snapshot.title);
    dialog.set_follows_content_size(false);
    dialog.set_content_width(680);
    dialog.set_content_height(620);
    dialog_chrome::sync_dialog_chrome_classes(window, &dialog, "companion-dialog-window");
    dialog_smoke::register_companion_dialog(&dialog);

    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .vexpand(true)
        .build();

    let scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .css_classes(["companion-dialog-scroller"])
        .build();
    scroller.set_has_frame(false);
    root.append(&scroller);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .css_classes(["settings-dialog-content", "companion-dialog-content"])
        .build();
    scroller.set_child(Some(&content));

    content.append(&build_companion_summary(&snapshot));
    append_section(&content, "Account", &snapshot.account_rows);
    append_section(&content, "Sync", &snapshot.sync_rows);
    append_section(&content, "Devices and teams", &snapshot.device_rows);

    let footer = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_top(12)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .css_classes(["companion-footer"])
        .build();
    let busy = Rc::new(RefCell::new(HashSet::new()));
    let action_bar = build_action_bar(window, &dialog, companion, &snapshot, busy);
    footer.append(&action_bar);

    let close_button = icons::labeled_button(
        "Close",
        icon_name::CLOSE,
        &[
            "pill-button",
            "ghost-link-button",
            "settings-close-button",
            "companion-close-button",
        ],
    );
    close_button.set_halign(gtk::Align::End);
    footer.append(&close_button);
    root.append(&footer);

    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&root));
    dialog.set_child(Some(&toast_overlay));
    dialog.set_default_widget(Some(&close_button));
    {
        let dialog = dialog.clone();
        close_button.connect_clicked(move |_| {
            dialog.close();
        });
    }

    dialog.present(Some(window));
    if let Some(notice) = notice {
        toast_overlay.add_toast(adw::Toast::new(&notice));
    }
}

fn build_companion_summary(snapshot: &CompanionPanelSnapshot) -> gtk::Widget {
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(14)
        .css_classes([
            "config-panel",
            "settings-section",
            "settings-summary",
            "companion-summary",
        ])
        .build();

    let icon = gtk::Box::builder()
        .width_request(40)
        .height_request(40)
        .valign(gtk::Align::Start)
        .css_classes(["settings-summary-icon", "companion-summary-icon"])
        .build();
    let account_icon = icons::image(icon_name::SETTINGS);
    account_icon.set_valign(gtk::Align::Center);
    icon.append(&account_icon);
    shell.append(&icon);

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .css_classes(["settings-summary-body", "companion-summary-body"])
        .build();
    body.append(
        &gtk::Label::builder()
            .label(&snapshot.title)
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["section-title", "settings-title", "settings-summary-title"])
            .build(),
    );
    body.append(
        &gtk::Label::builder()
            .label(&snapshot.subtitle)
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint", "settings-copy", "settings-summary-copy"])
            .build(),
    );
    shell.append(&body);

    let status_chip = gtk::Label::builder()
        .label(snapshot.status.label())
        .valign(gtk::Align::Center)
        .halign(gtk::Align::End)
        .css_classes([
            "status-chip",
            "settings-meta-chip",
            "companion-status-chip",
            status_class(snapshot.status),
        ])
        .build();
    shell.append(&status_chip);

    shell.upcast()
}

fn append_section(content: &gtk::Box, title: &str, rows: &[CompanionRow]) {
    if rows.is_empty() {
        return;
    }

    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel", "settings-section", "companion-section"])
        .build();
    section.append(
        &gtk::Label::builder()
            .label(title.to_uppercase())
            .halign(gtk::Align::Start)
            .css_classes([
                "eyebrow",
                "settings-section-heading",
                "companion-section-heading",
            ])
            .build(),
    );

    let list = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .css_classes(["companion-row-list"])
        .build();
    for row in rows {
        list.append(&build_row(row));
    }
    section.append(&list);
    content.append(&section);
}

fn build_row(row: &CompanionRow) -> gtk::Widget {
    let item = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(5)
        .halign(gtk::Align::Fill)
        .hexpand(true)
        .css_classes(["companion-row"])
        .build();

    let primary = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::Fill)
        .hexpand(true)
        .build();
    primary.append(
        &gtk::Label::builder()
            .label(&row.label)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .css_classes(["field-hint", "companion-row-label"])
            .build(),
    );
    primary.append(
        &gtk::Label::builder()
            .label(&row.value)
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .hexpand(true)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::Char)
            .selectable(true)
            .css_classes(["companion-row-value"])
            .build(),
    );
    item.append(&primary);

    if let Some(detail) = &row.detail {
        item.append(
            &gtk::Label::builder()
                .label(detail)
                .halign(gtk::Align::Start)
                .hexpand(true)
                .wrap(true)
                .css_classes([
                    "field-hint",
                    "settings-section-copy",
                    "companion-row-detail",
                ])
                .build(),
        );
    }

    item.upcast()
}

fn build_action_bar(
    window: &adw::ApplicationWindow,
    dialog: &adw::Dialog,
    companion: Arc<dyn CompanionIntegration>,
    snapshot: &CompanionPanelSnapshot,
    busy: Rc<RefCell<HashSet<String>>>,
) -> gtk::Widget {
    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .css_classes(["companion-actions"])
        .build();
    for action in &snapshot.actions {
        let button =
            icons::labeled_button(&action.label, action_icon(action), &button_classes(action));
        let action = action.clone();
        let companion = companion.clone();
        let parent = window.clone();
        let parent_dialog = dialog.clone();
        let button_for_click = button.clone();
        let busy = busy.clone();
        button.connect_clicked(move |_| {
            invoke_action(
                &parent,
                &parent_dialog,
                companion.clone(),
                action.clone(),
                button_for_click.clone(),
                busy.clone(),
            );
        });
        actions.append(&button);
    }
    actions.upcast()
}

fn action_icon(action: &CompanionAction) -> &'static str {
    let id = action.id.to_ascii_lowercase();
    if action.external_url.is_some() || id.contains("manage") || id.contains("portal") {
        icon_name::WEB
    } else if id.contains("refresh") {
        icon_name::REFRESH
    } else if id.contains("sync") {
        icon_name::APPLY
    } else if matches!(action.style, CompanionActionStyle::Destructive) {
        icon_name::DELETE
    } else {
        icon_name::NEXT
    }
}

fn button_classes(action: &CompanionAction) -> Vec<&'static str> {
    match action.style {
        CompanionActionStyle::Primary => {
            vec!["pill-button", "suggested-action", "companion-action-button"]
        }
        CompanionActionStyle::Destructive => {
            vec![
                "pill-button",
                "destructive-action",
                "companion-action-button",
            ]
        }
        CompanionActionStyle::Normal => {
            vec!["pill-button", "secondary-button", "companion-action-button"]
        }
    }
}

fn status_class(status: CompanionStatus) -> &'static str {
    match status {
        CompanionStatus::Ok => "is-ok",
        CompanionStatus::Warning => "is-warning",
        CompanionStatus::Error => "is-error",
        CompanionStatus::Syncing => "is-syncing",
        CompanionStatus::Inactive => "is-inactive",
    }
}

fn invoke_action(
    window: &adw::ApplicationWindow,
    dialog: &adw::Dialog,
    companion: Arc<dyn CompanionIntegration>,
    action: CompanionAction,
    button: gtk::Button,
    busy: Rc<RefCell<HashSet<String>>>,
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
        present_input_prompt(window, companion, action, input, busy);
        dialog.close();
        return;
    }
    dispatch_action(
        window,
        dialog,
        companion,
        action,
        CompanionActionInput::default(),
        button,
        busy,
    );
}

fn present_input_prompt(
    window: &adw::ApplicationWindow,
    companion: Arc<dyn CompanionIntegration>,
    action: CompanionAction,
    input: CompanionTextInput,
    busy: Rc<RefCell<HashSet<String>>>,
) {
    let dialog = adw::Dialog::new();
    dialog.set_title(&action.label);
    dialog.set_content_width(520);
    dialog_chrome::sync_dialog_chrome_classes(window, &dialog, "companion-input-dialog-window");

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .css_classes(["settings-dialog-content", "companion-dialog-content"])
        .build();

    let panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .css_classes(["config-panel", "settings-section", "companion-input-panel"])
        .build();
    panel.append(
        &gtk::Label::builder()
            .label(&action.label)
            .halign(gtk::Align::Start)
            .css_classes(["section-title", "settings-title"])
            .build(),
    );
    panel.append(
        &gtk::Label::builder()
            .label(&input.prompt)
            .wrap(true)
            .halign(gtk::Align::Start)
            .css_classes(["field-hint", "settings-copy"])
            .build(),
    );

    let entry = gtk::Entry::builder()
        .placeholder_text(input.placeholder.as_deref().unwrap_or(""))
        .visibility(!input.secret)
        .hexpand(true)
        .css_classes(["companion-input-entry"])
        .build();
    panel.append(&entry);
    content.append(&panel);

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .css_classes(["companion-actions"])
        .build();
    let cancel = icons::labeled_button(
        "Cancel",
        icon_name::CLOSE,
        &["pill-button", "ghost-link-button"],
    );
    let submit = icons::labeled_button(
        &action.label,
        action_icon(&action),
        &["pill-button", "suggested-action", "companion-action-button"],
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
        let submit_for_dispatch = submit.clone();
        let busy = busy.clone();
        submit.connect_clicked(move |_| {
            let text = entry_for_submit.text().trim().to_string();
            dispatch_action(
                &parent,
                &dialog,
                companion.clone(),
                action_for_submit.clone(),
                CompanionActionInput { text: Some(text) },
                submit_for_dispatch.clone(),
                busy.clone(),
            );
        });
    }
    {
        let submit = submit.clone();
        entry.connect_activate(move |_| submit.emit_clicked());
    }
    dialog.present(Some(window));
    entry.grab_focus();
}

fn dispatch_action(
    window: &adw::ApplicationWindow,
    dialog: &adw::Dialog,
    companion: Arc<dyn CompanionIntegration>,
    action: CompanionAction,
    input: CompanionActionInput,
    button: gtk::Button,
    busy: Rc<RefCell<HashSet<String>>>,
) {
    if !busy.borrow_mut().insert(action.id.clone()) {
        return;
    }
    button.set_sensitive(false);
    let (sender, receiver) = mpsc::channel();
    let action_id = action.id.clone();
    let timeout = action.timeout;
    let worker_companion = companion.clone();
    std::thread::spawn(move || {
        let result = worker_companion.invoke(&action_id, input);
        let _ = sender.send(result);
    });

    let started = Instant::now();
    let window = window.clone();
    let dialog = dialog.clone();
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        let completion = receiver.try_recv();
        let timed_out = started.elapsed() >= timeout;
        if matches!(completion, Err(mpsc::TryRecvError::Empty)) && !timed_out {
            return gtk::glib::ControlFlow::Continue;
        }

        busy.borrow_mut().remove(&action.id);
        button.set_sensitive(true);
        let (notice, refresh_scope) = match completion {
            Ok(Ok(result)) => {
                logging::info(format!(
                    "companion action '{}' completed: {}",
                    action.id, result.message
                ));
                (result.message, result.refresh_scope)
            }
            Ok(Err(error)) => {
                logging::error(format!(
                    "companion action '{}' failed: {}",
                    action.id, error
                ));
                (
                    format!("{} failed: {error}", action.label),
                    CompanionRefreshScope::Panel,
                )
            }
            Err(mpsc::TryRecvError::Empty) => {
                let error = format!(
                    "{} timed out after {} seconds",
                    action.label,
                    timeout.as_secs()
                );
                logging::error(&error);
                (error, CompanionRefreshScope::Panel)
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                let error = format!("{} worker stopped unexpectedly", action.label);
                logging::error(&error);
                (error, CompanionRefreshScope::Panel)
            }
        };
        if refresh_scope.refreshes_main_content() {
            let _ = gtk::prelude::WidgetExt::activate_action(&window, "win.refresh-catalog", None);
        }
        dialog.close();
        present_with_notice(&window, companion.clone(), Some(notice));
        gtk::glib::ControlFlow::Break
    });
}
