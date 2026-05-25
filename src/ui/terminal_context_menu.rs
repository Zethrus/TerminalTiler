use std::rc::Rc;

use adw::prelude::*;

use crate::ui::context_menu;

pub(crate) struct TerminalContextMenuInput {
    pub grab_focus: Rc<dyn Fn()>,
    pub has_selection: Rc<dyn Fn() -> bool>,
    pub can_paste: Rc<dyn Fn() -> bool>,
    pub can_reconnect: Rc<dyn Fn() -> bool>,
    pub can_open_local_shell: Rc<dyn Fn() -> bool>,
    pub copy: Rc<dyn Fn()>,
    pub paste: Rc<dyn Fn()>,
    pub reconnect: Rc<dyn Fn()>,
    pub open_local_shell: Rc<dyn Fn()>,
    pub show_transcript: Rc<dyn Fn()>,
    pub focus_command_input: Option<Rc<dyn Fn()>>,
}

pub(crate) struct TerminalContextMenuHandle {
    pub copy_button: gtk::Button,
}

pub(crate) fn install(
    parent: &(impl IsA<gtk::Widget> + Clone + 'static),
    input: TerminalContextMenuInput,
) -> TerminalContextMenuHandle {
    let popover = context_menu::popover(parent);
    let menu = context_menu::menu_box();

    let copy_button = context_menu::action_button("Copy", Some("Ctrl+Shift+C"));
    copy_button.set_sensitive((input.has_selection)());
    {
        let popover = popover.clone();
        let copy = input.copy.clone();
        copy_button.connect_clicked(move |_| {
            copy();
            popover.popdown();
        });
    }
    menu.append(&copy_button);

    let paste_button = context_menu::action_button("Paste", Some("Ctrl+Shift+V"));
    {
        let popover = popover.clone();
        let paste = input.paste.clone();
        paste_button.connect_clicked(move |_| {
            paste();
            popover.popdown();
        });
    }
    menu.append(&paste_button);

    let reconnect_button = context_menu::action_button("Reconnect", None);
    {
        let popover = popover.clone();
        let reconnect = input.reconnect.clone();
        reconnect_button.connect_clicked(move |_| {
            reconnect();
            popover.popdown();
        });
    }
    menu.append(&reconnect_button);

    let local_shell_button = context_menu::action_button("Open Local Shell", None);
    {
        let popover = popover.clone();
        let open_local_shell = input.open_local_shell.clone();
        local_shell_button.connect_clicked(move |_| {
            open_local_shell();
            popover.popdown();
        });
    }
    menu.append(&local_shell_button);

    let transcript_button = context_menu::action_button("Show Transcript", None);
    {
        let popover = popover.clone();
        let show_transcript = input.show_transcript.clone();
        transcript_button.connect_clicked(move |_| {
            popover.popdown();
            show_transcript();
        });
    }
    menu.append(&transcript_button);

    if let Some(focus_command_input) = input.focus_command_input.clone() {
        let focus_input_button = context_menu::action_button("Focus Command Input", None);
        {
            let popover = popover.clone();
            focus_input_button.connect_clicked(move |_| {
                focus_command_input();
                popover.popdown();
            });
        }
        menu.append(&focus_input_button);
    }

    popover.set_child(Some(&menu));

    let right_click = gtk::GestureClick::builder()
        .button(3)
        .propagation_phase(gtk::PropagationPhase::Capture)
        .build();
    {
        let popover = popover.clone();
        let copy_button_for_popup = copy_button.clone();
        let paste_button = paste_button.clone();
        let reconnect_button = reconnect_button.clone();
        let local_shell_button = local_shell_button.clone();
        let grab_focus = input.grab_focus.clone();
        let has_selection = input.has_selection.clone();
        let can_paste = input.can_paste.clone();
        let can_reconnect = input.can_reconnect.clone();
        let can_open_local_shell = input.can_open_local_shell.clone();
        right_click.connect_pressed(move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            grab_focus();
            copy_button_for_popup.set_sensitive(has_selection());
            paste_button.set_sensitive(can_paste());
            reconnect_button.set_sensitive(can_reconnect());
            local_shell_button.set_sensitive(can_open_local_shell());
            context_menu::popup_at(&popover, x, y);
        });
    }
    parent.add_controller(right_click);

    TerminalContextMenuHandle { copy_button }
}
