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
        if parent.as_ref().has_css_class(class_name) {
            dialog.add_css_class(class_name);
        }
    }
}
