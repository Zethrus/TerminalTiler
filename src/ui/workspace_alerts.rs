use std::rc::Rc;

use gtk::prelude::*;

use crate::services::alerts::{AlertEvent, AlertSeverity, AlertStore};
use crate::ui::icons::{self, name as icon_name};

fn severity_class(severity: AlertSeverity) -> &'static str {
    match severity {
        AlertSeverity::Info => "severity-info",
        AlertSeverity::Warning => "severity-warning",
        AlertSeverity::Error => "severity-error",
    }
}

pub(crate) type AlertActionProvider = Rc<dyn Fn(&AlertEvent, &AlertStore) -> Vec<AlertRowAction>>;

pub(crate) struct AlertRowAction {
    pub(crate) label: &'static str,
    pub(crate) icon_name: &'static str,
    pub(crate) on_activate: Rc<dyn Fn()>,
}

pub(crate) struct WorkspaceAlertListInput {
    pub(crate) alert_store: AlertStore,
    pub(crate) alert_button: gtk::Button,
    pub(crate) alert_list: gtk::Box,
    pub(crate) unread_badge: gtk::Label,
    pub(crate) action_provider: Option<AlertActionProvider>,
}

pub(crate) fn bind_alert_list(input: WorkspaceAlertListInput) {
    let alert_store = input.alert_store;
    let alert_button = input.alert_button;
    let alert_list = input.alert_list;
    let unread_badge = input.unread_badge;
    let action_provider = input.action_provider;

    let alert_store_for_refresh = alert_store.clone();
    let refresh = Rc::new(move || {
        let unread = alert_store_for_refresh.unread_count();
        icons::set_button_icon_label(
            &alert_button,
            &format!("Alerts ({unread})"),
            icon_name::ALERTS,
        );
        unread_badge.set_text(&unread.to_string());
        unread_badge.set_visible(unread > 0);

        while let Some(child) = alert_list.first_child() {
            alert_list.remove(&child);
        }

        let alerts = alert_store_for_refresh.snapshot();
        if alerts.is_empty() {
            alert_list.append(&build_empty_state());
            return;
        }

        for alert in alerts.into_iter().rev() {
            alert_list.append(&build_alert_row(
                &alert,
                &alert_store_for_refresh,
                action_provider.as_ref(),
            ));
        }
    });
    alert_store.subscribe(refresh.clone());
    refresh();
}

fn build_empty_state() -> gtk::Box {
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .css_classes(["alert-empty-state"])
        .build();
    container.append(
        &gtk::Label::builder()
            .label("You're all caught up")
            .css_classes(["alert-empty-title"])
            .build(),
    );
    container.append(
        &gtk::Label::builder()
            .label("No alerts to review right now.")
            .css_classes(["alert-empty-body"])
            .build(),
    );
    container
}

fn build_alert_row(
    alert: &AlertEvent,
    alert_store: &AlertStore,
    action_provider: Option<&AlertActionProvider>,
) -> gtk::Box {
    let severity = severity_class(alert.severity);
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .css_classes(["alert-row", severity])
        .build();
    if alert.unread {
        row.add_css_class("is-unread");
    }

    let title_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let dot = gtk::Box::builder()
        .valign(gtk::Align::Center)
        .css_classes(["alert-severity-dot", severity])
        .build();
    title_row.append(&dot);
    title_row.append(
        &gtk::Label::builder()
            .label(&alert.title)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .wrap(true)
            .css_classes(["card-title"])
            .build(),
    );
    row.append(&title_row);
    row.append(
        &gtk::Label::builder()
            .label(if alert.detail.trim().is_empty() {
                "No detail available."
            } else {
                alert.detail.as_str()
            })
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint"])
            .build(),
    );

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .margin_top(2)
        .build();
    if let Some(action_provider) = action_provider {
        for action in action_provider(alert, alert_store) {
            let button =
                icons::labeled_button(action.label, action.icon_name, &["flat", "surface-button"]);
            let on_activate = action.on_activate;
            button.connect_clicked(move |_| on_activate());
            actions.append(&button);
        }
    }

    let mark_read_button = icons::labeled_button(
        if alert.unread { "Mark Read" } else { "Read" },
        icon_name::APPLY,
        &["flat", "surface-button"],
    );
    mark_read_button.set_sensitive(alert.unread);
    let alert_store = alert_store.clone();
    let alert_id = alert.id;
    mark_read_button.connect_clicked(move |_| {
        alert_store.mark_read(alert_id);
    });
    actions.append(&mark_read_button);
    row.append(&actions);

    row
}
