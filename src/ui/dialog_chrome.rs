use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::pango;

use crate::ui::icons::{self, name as icon_name};

const PARITY_DIALOG_CLASS: &str = "parity-dialog-window";
const PREMIUM_MODAL_CLASS: &str = "premium-modal";
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

/// Accent tint for the premium modal icon chip.
pub(crate) enum ModalAccent {
    Danger,
    Amber,
}

/// Maps modal actions onto the shared button role contract.
pub(crate) enum ModalActionRole {
    Primary,
    Secondary,
    Ghost,
    Destructive,
}

impl ModalActionRole {
    fn css_class(&self) -> &'static str {
        match self {
            ModalActionRole::Primary => "primary-cta-button",
            ModalActionRole::Secondary => "secondary-button",
            ModalActionRole::Ghost => "ghost-link-button",
            ModalActionRole::Destructive => "destructive-button",
        }
    }
}

/// Shared premium alert/confirm modal: icon chip + eyebrow + heading + body
/// composition over `adw::Dialog`, with once-guarded actions and dismissal so
/// every alert surface keeps the same look and close semantics.
pub(crate) struct PremiumModal {
    dialog: adw::Dialog,
    surface_class: String,
    header: gtk::Box,
    header_text: gtk::Box,
    content: gtk::Box,
    actions: gtk::Box,
    action_taken: Rc<Cell<bool>>,
}

impl PremiumModal {
    pub(crate) fn new(surface_class: &str, heading: &str) -> Self {
        let dialog = adw::Dialog::new();
        dialog.set_title(heading);
        dialog.set_content_width(400);
        dialog.add_css_class(PREMIUM_MODAL_CLASS);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(16)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .css_classes(["premium-modal-content"])
            .build();

        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .css_classes(["premium-modal-header"])
            .build();

        let header_text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();

        let heading_label = gtk::Label::builder()
            .label(heading)
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(pango::WrapMode::WordChar)
            .css_classes(["premium-modal-heading"])
            .build();
        header_text.append(&heading_label);
        header.append(&header_text);
        content.append(&header);

        let actions = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .homogeneous(true)
            .halign(gtk::Align::End)
            .css_classes(["premium-modal-actions"])
            .build();

        Self {
            dialog,
            surface_class: surface_class.to_owned(),
            header,
            header_text,
            content,
            actions,
            action_taken: Rc::new(Cell::new(false)),
        }
    }

    pub(crate) fn content_width(self, width: i32) -> Self {
        self.dialog.set_content_width(width);
        self
    }

    pub(crate) fn eyebrow(self, text: &str) -> Self {
        let eyebrow = gtk::Label::builder()
            .label(text)
            .xalign(0.0)
            .css_classes(["eyebrow", "premium-modal-eyebrow"])
            .build();
        self.header_text
            .insert_child_after(&eyebrow, gtk::Widget::NONE);
        self
    }

    pub(crate) fn icon(self, icon_name: &str, accent: ModalAccent) -> Self {
        let chip = gtk::Box::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .css_classes(["premium-modal-icon-chip"])
            .build();
        chip.add_css_class(match accent {
            ModalAccent::Danger => "accent-danger",
            ModalAccent::Amber => "accent-amber",
        });
        let icon = icons::image(icon_name);
        icon.set_pixel_size(15);
        icon.set_halign(gtk::Align::Center);
        icon.set_valign(gtk::Align::Center);
        icon.set_hexpand(true);
        icon.set_vexpand(true);
        chip.append(&icon);
        self.header.insert_child_after(&chip, gtk::Widget::NONE);
        self
    }

    pub(crate) fn meta_chip(self, text: &str) -> Self {
        let chip = gtk::Label::builder()
            .label(text)
            .halign(gtk::Align::Start)
            .css_classes(["status-chip", "premium-modal-chip"])
            .build();
        self.header_text.append(&chip);
        self
    }

    pub(crate) fn body(self, text: &str) -> Self {
        let body = gtk::Label::builder()
            .label(text)
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(pango::WrapMode::WordChar)
            .css_classes(["premium-modal-body"])
            .build();
        self.content.append(&body);
        self
    }

    pub(crate) fn warning(self, text: &str) -> Self {
        let warning = gtk::Label::builder()
            .label(text)
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(pango::WrapMode::WordChar)
            .css_classes(["premium-modal-warning"])
            .build();
        self.content.append(&warning);
        self
    }

    /// Add arbitrary, caller-owned content while retaining the shared modal
    /// chrome. Long-running flows use this for progress UI without teaching
    /// the generic dialog builder about a product-specific operation.
    pub(crate) fn custom_content(self, widget: &impl IsA<gtk::Widget>) -> Self {
        self.content.append(widget);
        self
    }

    /// Add a caller-owned action whose lifetime and close behavior are
    /// externally controlled. This is useful for cancellable work where the
    /// button must remain visible until the worker acknowledges cancellation.
    pub(crate) fn external_action(self, button: &gtk::Button) -> Self {
        button.add_css_class("premium-modal-action");
        button.set_hexpand(true);
        button.set_halign(gtk::Align::Fill);
        self.actions.append(button);
        self
    }

    /// Disable incidental Escape/close dismissal for atomic operation phases.
    pub(crate) fn dismissible(self, dismissible: bool) -> Self {
        self.dialog.set_can_close(dismissible);
        self
    }

    pub(crate) fn entry(&self, initial: &str) -> gtk::Entry {
        let entry = gtk::Entry::builder()
            .hexpand(true)
            .text(initial)
            .activates_default(true)
            .css_classes(["dialog-input"])
            .build();
        self.content.append(&entry);
        entry
    }

    pub(crate) fn stacked_actions(self) -> Self {
        self.actions.set_orientation(gtk::Orientation::Vertical);
        self.actions.set_homogeneous(false);
        self.actions.set_halign(gtk::Align::Fill);
        self
    }

    pub(crate) fn action(
        self,
        label: &str,
        role: ModalActionRole,
        is_default: bool,
        handler: impl Fn() + 'static,
    ) -> Self {
        let button = gtk::Button::with_label(label);
        button.add_css_class("premium-modal-action");
        button.add_css_class(role.css_class());
        button.set_hexpand(true);
        button.set_halign(gtk::Align::Fill);

        let action_taken = self.action_taken.clone();
        let dialog = self.dialog.clone();
        button.connect_clicked(move |_| {
            if !action_taken.replace(true) {
                handler();
            }
            dialog.close();
        });

        if is_default {
            self.dialog.set_default_widget(Some(&button));
        }
        self.actions.append(&button);
        self
    }

    pub(crate) fn on_dismiss(self, handler: impl Fn() + 'static) -> Self {
        let action_taken = self.action_taken.clone();
        self.dialog.connect_closed(move |_| {
            if !action_taken.replace(true) {
                handler();
            }
        });
        self
    }

    pub(crate) fn present(self, parent: Option<&impl IsA<gtk::Widget>>) {
        self.content.append(&self.actions);
        self.dialog.set_child(Some(&self.content));
        match parent {
            Some(parent) => {
                sync_dialog_chrome_classes(parent, &self.dialog, &self.surface_class);
                self.dialog.present(Some(parent.as_ref()));
            }
            None => {
                self.dialog.add_css_class(PARITY_DIALOG_CLASS);
                self.dialog.add_css_class(&self.surface_class);
                self.dialog.present(gtk::Widget::NONE);
            }
        }
    }

    /// Present the modal and retain a handle for a later programmatic close.
    /// Long-running operations use this to replace progress UI with the final
    /// result instead of stacking another dialog above it.
    pub(crate) fn present_with_handle(self, parent: Option<&impl IsA<gtk::Widget>>) -> adw::Dialog {
        let dialog = self.dialog.clone();
        self.present(parent);
        dialog
    }
}

/// Destructive confirmation that reports both outcomes; Esc/close counts as
/// declining exactly once.
pub(crate) fn confirm_destructive_choice<F>(
    parent: Option<&impl IsA<gtk::Widget>>,
    surface_class: &str,
    heading: &str,
    body: &str,
    confirm_label: &str,
    on_response: F,
) where
    F: Fn(bool) + 'static,
{
    let on_response: Rc<dyn Fn(bool)> = Rc::new(on_response);
    let on_cancel = on_response.clone();
    let on_confirm = on_response.clone();
    let on_close = on_response;
    PremiumModal::new(surface_class, heading)
        .icon(icon_name::DIALOG_WARNING, ModalAccent::Danger)
        .body(body)
        .action("Cancel", ModalActionRole::Secondary, true, move || {
            on_cancel(false)
        })
        .action(
            confirm_label,
            ModalActionRole::Destructive,
            false,
            move || on_confirm(true),
        )
        .on_dismiss(move || on_close(false))
        .present(parent);
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
    confirm_destructive_choice(
        Some(window),
        "destructive-confirm-dialog",
        heading,
        body,
        confirm_label,
        move |confirmed| {
            if confirmed {
                on_confirm();
            }
        },
    );
}

/// Informational notice with a single acknowledging action.
pub(crate) fn present_notice(
    parent: &impl IsA<gtk::Widget>,
    surface_class: &str,
    heading: &str,
    body: &str,
) {
    PremiumModal::new(surface_class, heading)
        .icon(icon_name::DIALOG_INFO, ModalAccent::Amber)
        .body(body)
        .action("OK", ModalActionRole::Secondary, true, || {})
        .present(Some(parent));
}

fn source_has_chrome_class(source: &gtk::Widget, class_name: &str) -> bool {
    source.has_css_class(class_name)
        || source
            .root()
            .is_some_and(|root| root.has_css_class(class_name))
}
