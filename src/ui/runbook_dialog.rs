use std::rc::Rc;

use adw::prelude::*;

use crate::model::assets::{Runbook, RunbookConfirmPolicy, TemplateVariableValues};
use crate::ui::dialog_chrome;
use crate::ui::icons::{self, name as icon_name};

pub(crate) fn present(
    button: &gtk::Button,
    runbook: &Runbook,
    execute: Rc<dyn Fn(TemplateVariableValues)>,
) {
    if runbook.variables.is_empty() && runbook.confirm_policy == RunbookConfirmPolicy::Never {
        execute(TemplateVariableValues::new());
        return;
    }

    let Some(window) = button
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    else {
        return;
    };

    let dialog = adw::Dialog::new();
    dialog.set_title(&format!("Run {}", runbook.name));
    dialog_chrome::sync_dialog_chrome_classes(&window, &dialog, "runbook-dialog-window");

    let area = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();
    area.append(
        &gtk::Label::builder()
            .label(runbook_summary(runbook))
            .wrap(true)
            .halign(gtk::Align::Start)
            .build(),
    );

    let entries = runbook
        .variables
        .iter()
        .map(|variable| {
            let entry = gtk::Entry::builder()
                .placeholder_text(&variable.label)
                .text(variable.default_value.clone().unwrap_or_default())
                .activates_default(true)
                .build();
            area.append(
                &gtk::Label::builder()
                    .label(&variable.label)
                    .halign(gtk::Align::Start)
                    .build(),
            );
            area.append(&entry);
            (variable.id.clone(), entry)
        })
        .collect::<Vec<_>>();

    let preview = runbook
        .steps
        .iter()
        .map(|step| step.command.clone())
        .collect::<Vec<_>>()
        .join("\n");
    area.append(
        &gtk::Label::builder()
            .label(format!("Preview:\n{preview}"))
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint"])
            .build(),
    );

    let action_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .build();
    let cancel_button = icons::labeled_button("Cancel", icon_name::CLOSE, &["pill-button", "flat"]);
    let run_button =
        icons::labeled_button("Run", icon_name::RUN, &["pill-button", "suggested-action"]);
    action_row.append(&cancel_button);
    action_row.append(&run_button);
    area.append(&action_row);
    dialog.set_child(Some(&area));
    dialog.set_default_widget(Some(&run_button));

    {
        let dialog = dialog.clone();
        cancel_button.connect_clicked(move |_| {
            dialog.close();
        });
    }
    {
        let dialog = dialog.clone();
        run_button.connect_clicked(move |_| {
            let variables = entries
                .iter()
                .map(|(id, entry)| (id.clone(), entry.text().to_string()))
                .collect::<TemplateVariableValues>();
            execute(variables);
            dialog.close();
        });
    }

    dialog.present(Some(&window));
}

fn runbook_summary(runbook: &Runbook) -> String {
    let metadata = format!(
        "Target: {}  •  Steps: {}  •  {}",
        runbook.target.label(),
        runbook.steps.len(),
        runbook.confirm_policy.label()
    );

    if runbook.description.trim().is_empty() {
        metadata
    } else {
        format!("{}\n{metadata}", runbook.description)
    }
}
