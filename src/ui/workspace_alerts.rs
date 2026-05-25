use std::rc::Rc;

use gtk::prelude::*;

use crate::services::alerts::{AlertEvent, AlertStore};
use crate::ui::icons::{self, name as icon_name};

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
    pub(crate) action_provider: Option<AlertActionProvider>,
}

pub(crate) fn bind_alert_list(input: WorkspaceAlertListInput) {
    let alert_store = input.alert_store;
    let alert_button = input.alert_button;
    let alert_list = input.alert_list;
    let action_provider = input.action_provider;

    let alert_store_for_refresh = alert_store.clone();
    let refresh = Rc::new(move || {
        icons::set_button_icon_label(
            &alert_button,
            &format!("Alerts ({})", alert_store_for_refresh.unread_count()),
            icon_name::ALERTS,
        );
        while let Some(child) = alert_list.first_child() {
            alert_list.remove(&child);
        }

        for alert in alert_store_for_refresh.snapshot().into_iter().rev() {
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

fn build_alert_row(
    alert: &AlertEvent,
    alert_store: &AlertStore,
    action_provider: Option<&AlertActionProvider>,
) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .css_classes(["tile-editor-row"])
        .build();
    row.append(
        &gtk::Label::builder()
            .label(&alert.title)
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["card-title"])
            .build(),
    );
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
