use std::rc::Rc;

use adw::prelude::*;

use crate::ui::dialog_chrome;
use crate::ui::icons::{self, name as icon_name};

pub(crate) fn present<F>(window: &adw::ApplicationWindow, current_title: &str, on_submit: F)
where
    F: Fn(Option<String>) + 'static,
{
    let dialog = adw::Dialog::new();
    dialog.set_title("Rename Workspace");
    dialog_chrome::sync_dialog_chrome_classes(window, &dialog, "tab-rename-dialog-window");

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();

    let body = gtk::Label::builder()
        .label("Enter a new workspace tab name. Leave it blank to restore automatic naming.")
        .wrap(true)
        .halign(gtk::Align::Start)
        .build();
    let entry = gtk::Entry::builder()
        .hexpand(true)
        .text(current_title)
        .activates_default(true)
        .build();
    content.append(&body);
    content.append(&entry);

    let action_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .build();
    let cancel_button = icons::labeled_button("Cancel", icon_name::CLOSE, &["pill-button", "flat"]);
    let apply_button = icons::labeled_button(
        "Apply",
        icon_name::APPLY,
        &["pill-button", "suggested-action"],
    );
    action_row.append(&cancel_button);
    action_row.append(&apply_button);
    content.append(&action_row);
    dialog.set_child(Some(&content));
    dialog.set_default_widget(Some(&apply_button));

    let on_submit = Rc::new(on_submit);
    {
        let dialog = dialog.clone();
        cancel_button.connect_clicked(move |_| {
            dialog.close();
        });
    }
    {
        let dialog = dialog.clone();
        let entry_for_submit = entry.clone();
        let on_submit = on_submit.clone();
        apply_button.connect_clicked(move |_| {
            let requested_title = entry_for_submit.text().trim().to_string();
            if requested_title.is_empty() {
                on_submit(None);
            } else {
                on_submit(Some(requested_title));
            }
            dialog.close();
        });
    }

    dialog.present(Some(window));
    entry.grab_focus();
    entry.set_position(-1);
}
