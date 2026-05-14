use gtk::{gdk, prelude::*};

pub(crate) fn popover(parent: &impl IsA<gtk::Widget>) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.add_css_class("terminal-context-popover");
    popover.set_autohide(true);
    popover.set_has_arrow(true);
    popover.set_position(gtk::PositionType::Bottom);
    popover.set_parent(parent);
    popover
}

pub(crate) fn menu_box() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(4)
        .margin_end(4)
        .css_classes(["terminal-context-menu"])
        .build()
}

pub(crate) fn action_button(label: &str, shortcut: Option<&str>) -> gtk::Button {
    let button = gtk::Button::builder()
        .focus_on_click(false)
        .css_classes(["flat", "terminal-context-action"])
        .build();

    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .hexpand(true)
        .build();
    row.append(
        &gtk::Label::builder()
            .label(label)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .css_classes(["terminal-context-label"])
            .build(),
    );

    if let Some(shortcut) = shortcut {
        row.append(
            &gtk::Label::builder()
                .label(shortcut)
                .halign(gtk::Align::End)
                .css_classes(["terminal-context-shortcut"])
                .build(),
        );
    }

    button.set_child(Some(&row));
    button
}

pub(crate) fn popup_at(popover: &gtk::Popover, x: f64, y: f64) {
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        x.round() as i32,
        y.round() as i32,
        1,
        1,
    )));
    popover.popup();
}
