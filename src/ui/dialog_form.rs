//! Shared form widgets for the Kanban board dialogs (`new_task_dialog`,
//! `agent_setup_dialog`). Centralises the eyebrow field label and the premium input
//! surfaces so both dialogs stay visually consistent and the styling lives in one place.

/// An uppercase "eyebrow" label that captions a form field (TITLE, DESCRIPTION, …).
pub(crate) fn field_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Start)
        .css_classes(["eyebrow", "field-label"])
        .build()
}

/// A single-line text entry styled as a premium dialog input. Submits the dialog's
/// default widget on Enter.
pub(crate) fn text_input(placeholder: &str) -> gtk::Entry {
    gtk::Entry::builder()
        .hexpand(true)
        .placeholder_text(placeholder)
        .activates_default(true)
        .css_classes(["dialog-input"])
        .build()
}

/// A scrollable multi-line text field. Returns the scroller (append this to the layout)
/// and the inner `TextView` (read its buffer on submit).
pub(crate) fn multiline_input(min_height: i32) -> (gtk::ScrolledWindow, gtk::TextView) {
    let view = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::Word)
        .accepts_tab(false)
        .css_classes(["kanban-description-input"])
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(min_height)
        .css_classes(["kanban-description-scroller"])
        .build();
    scroller.set_child(Some(&view));
    (scroller, view)
}
