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

    let view = build_companion_view(window, &dialog, companion, &snapshot);
    let close_button = view.close_button;
    let root = view.root;

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

struct CompanionDialogView {
    root: gtk::Box,
    close_button: gtk::Button,
}

fn build_companion_view(
    window: &adw::ApplicationWindow,
    dialog: &adw::Dialog,
    companion: Arc<dyn CompanionIntegration>,
    snapshot: &CompanionPanelSnapshot,
) -> CompanionDialogView {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Fill)
        .hexpand(true)
        .vexpand(true)
        .css_classes(["companion-dialog-root"])
        .build();

    let scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .min_content_width(0)
        .propagate_natural_width(false)
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
        .halign(gtk::Align::Fill)
        .hexpand(true)
        .css_classes(["settings-dialog-content", "companion-dialog-content"])
        .build();
    scroller.set_child(Some(&content));

    content.append(&build_companion_summary(snapshot));
    append_section(&content, "Account", &snapshot.account_rows);
    append_section(&content, "Sync", &snapshot.sync_rows);
    append_section(&content, "Devices and teams", &snapshot.device_rows);
    let busy = Rc::new(RefCell::new(HashSet::new()));
    append_action_groups(&content, window, dialog, companion, snapshot, busy);

    let footer = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::Fill)
        .hexpand(true)
        .margin_top(12)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .css_classes(["companion-footer"])
        .build();

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

    CompanionDialogView { root, close_button }
}

fn build_companion_summary(snapshot: &CompanionPanelSnapshot) -> gtk::Widget {
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(14)
        .halign(gtk::Align::Fill)
        .hexpand(true)
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
        .halign(gtk::Align::Fill)
        .hexpand(true)
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
        .halign(gtk::Align::Fill)
        .hexpand(true)
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
        .css_classes(["companion-row-primary"])
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
            .halign(gtk::Align::Fill)
            .valign(gtk::Align::Start)
            .hexpand(true)
            .xalign(1.0)
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

/// Partition actions into titled groups, preserving first-seen group order.
/// Actions without a group fall into the default section so ungrouped
/// snapshots render exactly as they did before grouping existed.
fn grouped_actions(snapshot: &CompanionPanelSnapshot) -> Vec<(String, Vec<&CompanionAction>)> {
    let mut groups: Vec<(String, Vec<&CompanionAction>)> = Vec::new();
    for action in &snapshot.actions {
        let key = action
            .group
            .clone()
            .unwrap_or_else(|| "Actions".to_string());
        if let Some((_, actions)) = groups.iter_mut().find(|(name, _)| *name == key) {
            actions.push(action);
        } else {
            groups.push((key, vec![action]));
        }
    }
    groups
}

fn append_action_groups(
    content: &gtk::Box,
    window: &adw::ApplicationWindow,
    dialog: &adw::Dialog,
    companion: Arc<dyn CompanionIntegration>,
    snapshot: &CompanionPanelSnapshot,
    busy: Rc<RefCell<HashSet<String>>>,
) {
    if snapshot.actions.is_empty() {
        return;
    }

    let all_grouped = snapshot.actions.iter().all(|action| action.group.is_some());
    let groups = grouped_actions(snapshot);
    // A fully-grouped snapshot with a single group is typically a hero
    // call-to-action (for example "Activate Pro"); the redundant eyebrow
    // heading only adds noise there.
    let hide_heading = should_hide_action_group_heading(all_grouped, groups.len());
    for (group, actions) in groups {
        let section = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .halign(gtk::Align::Fill)
            .hexpand(true)
            .css_classes([
                "config-panel",
                "settings-section",
                "companion-section",
                "companion-actions-section",
            ])
            .build();
        if !hide_heading {
            section.append(
                &gtk::Label::builder()
                    .label(group.to_uppercase())
                    .halign(gtk::Align::Start)
                    .css_classes([
                        "eyebrow",
                        "settings-section-heading",
                        "companion-section-heading",
                    ])
                    .build(),
            );
        }
        section.append(&build_action_grid(
            window,
            dialog,
            companion.clone(),
            &actions,
            busy.clone(),
        ));
        content.append(&section);
    }
}

fn should_hide_action_group_heading(all_grouped: bool, group_count: usize) -> bool {
    all_grouped && group_count == 1
}

fn build_action_grid(
    window: &adw::ApplicationWindow,
    dialog: &adw::Dialog,
    companion: Arc<dyn CompanionIntegration>,
    actions: &[&CompanionAction],
    busy: Rc<RefCell<HashSet<String>>>,
) -> gtk::FlowBox {
    let grid = gtk::FlowBox::builder()
        .orientation(gtk::Orientation::Horizontal)
        .selection_mode(gtk::SelectionMode::None)
        .activate_on_single_click(false)
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(3)
        .column_spacing(8)
        .row_spacing(8)
        .halign(gtk::Align::Fill)
        .hexpand(true)
        .css_classes(["companion-action-grid"])
        .build();
    for action in actions {
        let button = build_action_button(action);
        button.set_halign(gtk::Align::Fill);
        button.set_hexpand(true);
        let action = (*action).clone();
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
        grid.append(&button);
    }
    grid
}

fn build_action_button(action: &CompanionAction) -> gtk::Button {
    let button = gtk::Button::builder().build();
    for class_name in button_classes(action) {
        button.add_css_class(class_name);
    }

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    let title = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::Center)
        .build();
    title.append(&icons::image(action_icon(action)));
    title.append(
        &gtk::Label::builder()
            .label(&action.label)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(28)
            .build(),
    );
    body.append(&title);
    if let Some(detail) = &action.detail {
        body.append(
            &gtk::Label::builder()
                .label(detail)
                .wrap(true)
                .max_width_chars(30)
                .css_classes(["field-hint", "companion-action-detail"])
                .build(),
        );
    }
    button.set_child(Some(&body));
    button
}

fn action_icon(action: &CompanionAction) -> &'static str {
    let id = action.id.to_ascii_lowercase();
    if action.external_url.is_some() || id.contains("manage") || id.contains("portal") {
        icon_name::WEB
    } else if id.contains("refresh") {
        icon_name::REFRESH
    } else if id.starts_with("voice") {
        icon_name::RECORD
    } else if id.contains("key") || id.contains("activate") {
        icon_name::SAVE_SYMBOLIC
    } else if id.contains("conflict") {
        icon_name::ALERTS
    } else if id.contains("version") || id.contains("restore") {
        icon_name::RESTORE
    } else if id.contains("workspace") {
        icon_name::WORKSPACES
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
    if matches!(action.style, CompanionActionStyle::Destructive) {
        let parent = window.clone();
        let parent_dialog = dialog.clone();
        let heading = action.label.clone();
        let body = destructive_confirmation_copy(&action).to_string();
        dialog_chrome::confirm_destructive_action(window, &heading, &body, &heading, move || {
            dispatch_action(
                &parent,
                &parent_dialog,
                companion.clone(),
                action.clone(),
                CompanionActionInput::default(),
                button.clone(),
                busy.clone(),
            );
        });
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

fn destructive_confirmation_copy(action: &CompanionAction) -> &str {
    action
        .detail
        .as_deref()
        .filter(|detail| !detail.trim().is_empty())
        .unwrap_or("This action cannot be undone.")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_actions_partition_in_first_seen_order() {
        let snapshot = CompanionPanelSnapshot {
            actions: vec![
                CompanionAction::button("sync_now", "Sync now").in_group("General"),
                CompanionAction::button("voice_enable", "Enable voice control")
                    .in_group("Voice Control"),
                CompanionAction::button("refresh", "Refresh status").in_group("General"),
                CompanionAction::button("deactivate", "Deactivate").in_group("Danger zone"),
            ],
            ..CompanionPanelSnapshot::default()
        };

        let groups = grouped_actions(&snapshot);
        let names: Vec<&str> = groups.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["General", "Voice Control", "Danger zone"]);
        let general_ids: Vec<&str> = groups[0]
            .1
            .iter()
            .map(|action| action.id.as_str())
            .collect();
        assert_eq!(general_ids, ["sync_now", "refresh"]);
    }

    #[test]
    fn ungrouped_actions_fall_into_default_section() {
        let snapshot = CompanionPanelSnapshot {
            actions: vec![
                CompanionAction::button("refresh", "Refresh status"),
                CompanionAction::button("sync_now", "Sync now"),
            ],
            ..CompanionPanelSnapshot::default()
        };

        let groups = grouped_actions(&snapshot);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "Actions");
        assert_eq!(groups[0].1.len(), 2);
    }

    #[test]
    fn action_builder_sets_group_detail_and_input() {
        let action = CompanionAction::button("sync_conflict_resolve", "Resolve conflict")
            .in_group("Advanced")
            .with_detail("Pick which side wins for a conflicted object.")
            .with_input(CompanionTextInput {
                prompt: "Enter '<object-id> <local|remote>'.".to_string(),
                placeholder: Some("object-id local".to_string()),
                secret: false,
            });

        assert_eq!(action.group.as_deref(), Some("Advanced"));
        assert!(action.detail.is_some());
        assert!(action.input.is_some());
    }

    #[test]
    fn a_single_fully_grouped_activation_section_hides_its_heading() {
        assert!(should_hide_action_group_heading(true, 1));
        assert!(!should_hide_action_group_heading(true, 2));
        assert!(!should_hide_action_group_heading(false, 1));
    }

    #[test]
    fn destructive_confirmation_prefers_action_detail_with_a_safe_fallback() {
        let detailed = CompanionAction::button("deactivate", "Deactivate this device")
            .with_detail("Local Pro features stop, but the account stays active.");
        assert_eq!(
            destructive_confirmation_copy(&detailed),
            "Local Pro features stop, but the account stays active."
        );

        let generic = CompanionAction::button("delete", "Delete");
        assert_eq!(
            destructive_confirmation_copy(&generic),
            "This action cannot be undone."
        );
    }

    #[derive(Clone)]
    struct DenseCompanion {
        snapshot: CompanionPanelSnapshot,
    }

    impl CompanionIntegration for DenseCompanion {
        fn snapshot(&self) -> CompanionPanelSnapshot {
            self.snapshot.clone()
        }

        fn invoke(
            &self,
            action_id: &str,
            _input: CompanionActionInput,
        ) -> Result<crate::extension::CompanionActionResult, String> {
            Ok(crate::extension::CompanionActionResult::message(format!(
                "invoked {action_id}"
            )))
        }
    }

    fn dense_snapshot() -> CompanionPanelSnapshot {
        let long_id = "8a6b7d3e-4a56-4e90-9a7d-5c10a94ed58f";
        let actions = (0..24)
            .map(|index| {
                CompanionAction::button(
                    format!("dense_action_{index}"),
                    format!("Action {index}: inspect a long account or synchronization setting"),
                )
                .in_group(if index < 8 {
                    "General"
                } else if index < 16 {
                    "Voice Control"
                } else {
                    "Advanced"
                })
            })
            .collect();
        CompanionPanelSnapshot {
            title: "TerminalTiler Pro Account / Sync".into(),
            subtitle: "Activation, billing, devices, voice orchestration, and Cloud Sync Preview."
                .into(),
            status: CompanionStatus::Ok,
            account_rows: vec![
                CompanionRow::new("Activation", "Active"),
                CompanionRow::new("Plan", "individual_monthly"),
                CompanionRow::new("Entitlement", "active"),
                CompanionRow::new(
                    "Output device",
                    "alsa_output.usb-TerminalTiler_Studio_Interface-00.analog-stereo",
                ),
            ],
            sync_rows: vec![
                CompanionRow::new("Vault readiness", "Personal ready"),
                CompanionRow::new(
                    "Workspace root",
                    "/home/alice/projects/a-very-long-workspace-name-that-must-wrap",
                ),
            ],
            device_rows: vec![CompanionRow::new("Current device", long_id)],
            actions,
        }
    }

    fn descendants(root: &gtk::Widget) -> Vec<gtk::Widget> {
        let mut widgets = Vec::new();
        let mut child = root.first_child();
        while let Some(widget) = child {
            widgets.push(widget.clone());
            widgets.extend(descendants(&widget));
            child = widget.next_sibling();
        }
        widgets
    }

    fn settle_layout() {
        let context = gtk::glib::MainContext::default();
        for _ in 0..20 {
            while context.iteration(false) {}
        }
    }

    #[test]
    fn dense_companion_layout_stays_bounded_and_reflows_actions() {
        const CHILD_ENV: &str = "TERMINALTILER_COMPANION_LAYOUT_TEST_CHILD";
        if std::env::var_os(CHILD_ENV).is_none() {
            let executable = std::env::current_exe().expect("current Core test executable");
            let test_name = "ui::companion_dialog::tests::dense_companion_layout_stays_bounded_and_reflows_actions";
            let mut command = if std::env::var_os("DISPLAY").is_some() {
                let mut command = std::process::Command::new(executable);
                command.args(["--exact", test_name, "--nocapture"]);
                command
            } else {
                let mut command = std::process::Command::new("xvfb-run");
                command
                    .arg("-a")
                    .arg(executable)
                    .args(["--exact", test_name, "--nocapture"]);
                command
            };
            let status = command
                .env(CHILD_ENV, "1")
                .status()
                .expect("Xvfb companion layout test process must start");
            assert!(status.success(), "Xvfb companion layout test failed");
            return;
        }

        adw::init().expect("GTK must initialize under Xvfb");
        let application = adw::Application::builder()
            .application_id("app.terminaltiler.CompanionLayoutTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        application
            .register(gio::Cancellable::NONE)
            .expect("test application must register");

        for viewport_width in [480, 680] {
            let snapshot = dense_snapshot();
            let companion: Arc<dyn CompanionIntegration> = Arc::new(DenseCompanion {
                snapshot: snapshot.clone(),
            });
            let window = adw::ApplicationWindow::new(&application);
            let dialog = adw::Dialog::new();
            let view = build_companion_view(&window, &dialog, companion, &snapshot);
            let root = view.root.clone().upcast::<gtk::Widget>();
            let (minimum_width, _, _, _) = root.measure(gtk::Orientation::Horizontal, -1);
            assert!(
                minimum_width <= viewport_width,
                "root minimum width {minimum_width} exceeded {viewport_width}px viewport"
            );
            root.allocate(viewport_width, 620, -1, None);
            settle_layout();
            assert_eq!(root.allocated_width(), viewport_width);

            let widgets = descendants(&root);
            let scroller = widgets
                .iter()
                .find_map(|widget| widget.clone().downcast::<gtk::ScrolledWindow>().ok())
                .expect("companion scroller");
            assert_eq!(scroller.hscrollbar_policy(), gtk::PolicyType::Never);
            assert_eq!(scroller.min_content_width(), 0);
            assert!(!scroller.propagates_natural_width());
            assert!(
                scroller.hadjustment().upper() <= scroller.hadjustment().page_size() + 1.0,
                "responsive companion content must not require horizontal scrolling"
            );

            let status = widgets
                .iter()
                .find(|widget| widget.has_css_class("companion-status-chip"))
                .expect("status chip");
            assert!(status.allocated_width() > 0);
            let values = widgets
                .iter()
                .filter(|widget| widget.has_css_class("companion-row-value"))
                .collect::<Vec<_>>();
            assert_eq!(values.len(), 7);
            assert!(values.iter().all(|value| value.allocated_width() > 0));

            let action_buttons = widgets
                .iter()
                .filter(|widget| widget.has_css_class("companion-action-button"))
                .collect::<Vec<_>>();
            assert_eq!(action_buttons.len(), 24);
            assert!(
                action_buttons
                    .iter()
                    .all(|button| button.allocated_width() > 0)
            );
            let action_rows = action_buttons
                .iter()
                .filter_map(|button| button.parent())
                .map(|child| child.allocation().y())
                .collect::<HashSet<_>>();
            assert!(
                action_rows.len() > 1,
                "dense actions must wrap into vertically reachable rows"
            );
            assert!(!view.close_button.is_ancestor(&scroller));
        }
    }
}
