use std::rc::Rc;

use adw::prelude::*;

use crate::ui::dialog_chrome;
use crate::ui::icons::{self, name as icon_name};

pub(crate) fn build(
    parent: &impl IsA<gtk::Widget>,
    reconnect: Rc<dyn Fn()>,
    open_local_shell: Rc<dyn Fn()>,
) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.add_css_class("terminal-recovery-popover");
    popover.set_autohide(true);
    popover.set_has_arrow(true);
    popover.set_position(gtk::PositionType::Bottom);
    popover.set_parent(parent);
    dialog_chrome::sync_popover_chrome_classes(
        parent,
        &popover,
        "terminal-recovery-popover-window",
    );

    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(10)
        .margin_end(10)
        .build();
    shell.append(
        &gtk::Label::builder()
            .label("Session ended")
            .halign(gtk::Align::Start)
            .css_classes(["card-title"])
            .build(),
    );
    shell.append(
        &gtk::Label::builder()
            .label("Reconnect the configured session or open a local shell in this pane.")
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint"])
            .build(),
    );

    let reconnect_button = icons::labeled_button(
        "Reconnect Session",
        icon_name::RECOVER,
        &["flat", "surface-button"],
    );
    reconnect_button.set_focus_on_click(false);
    {
        let popover = popover.clone();
        reconnect_button.connect_clicked(move |_| {
            reconnect();
            popover.popdown();
        });
    }
    shell.append(&reconnect_button);

    let local_shell_button = icons::labeled_button(
        "Open Local Shell",
        icon_name::TERMINAL,
        &["flat", "surface-button"],
    );
    local_shell_button.set_focus_on_click(false);
    {
        let popover = popover.clone();
        local_shell_button.connect_clicked(move |_| {
            open_local_shell();
            popover.popdown();
        });
    }
    shell.append(&local_shell_button);

    popover.set_child(Some(&shell));
    popover
}
