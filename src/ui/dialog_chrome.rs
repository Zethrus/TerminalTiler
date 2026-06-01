use adw::prelude::*;

const PARITY_DIALOG_CLASS: &str = "parity-dialog-window";
const INHERITED_CHROME_CLASSES: &[&str] = &[
    "theme-light",
    "theme-dark",
    "profile-comfortable",
    "profile-standard",
    "profile-compact",
    "windows-gtk-shell",
];

pub(crate) fn sync_dialog_chrome_classes(
    parent: &impl IsA<gtk::Widget>,
    dialog: &impl IsA<gtk::Widget>,
    surface_class: &str,
) {
    let dialog = dialog.as_ref();
    dialog.add_css_class(PARITY_DIALOG_CLASS);
    dialog.add_css_class(surface_class);
    for class_name in INHERITED_CHROME_CLASSES {
        dialog.remove_css_class(class_name);
        if source_has_chrome_class(parent.as_ref(), class_name) {
            dialog.add_css_class(class_name);
        }
    }
}

pub(crate) fn sync_popover_chrome_classes(
    parent: &impl IsA<gtk::Widget>,
    popover: &gtk::Popover,
    surface_class: &str,
) {
    sync_dialog_chrome_classes(parent, popover, surface_class);
}

pub(crate) fn confirm_destructive_action<F>(
    window: &adw::ApplicationWindow,
    heading: &str,
    body: &str,
    confirm_label: &str,
    on_confirm: F,
) where
    F: Fn() + 'static,
{
    let dialog = adw::MessageDialog::builder()
        .modal(true)
        .transient_for(window)
        .heading(heading)
        .body(body)
        .build();
    sync_dialog_chrome_classes(window, &dialog, "destructive-confirm-dialog");

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("confirm", confirm_label);
    dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    dialog.connect_response(None, move |dialog, response| {
        if response == "confirm" {
            on_confirm();
        }
        dialog.close();
    });

    dialog.present();
}

fn source_has_chrome_class(source: &gtk::Widget, class_name: &str) -> bool {
    source.has_css_class(class_name)
        || source
            .root()
            .is_some_and(|root| root.has_css_class(class_name))
}
