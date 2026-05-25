use std::rc::Rc;

use gtk::{gio, prelude::*};

use crate::{logging, ui::context_menu};

pub(crate) struct WebContextMenuInput {
    pub reload: Rc<dyn Fn()>,
    pub current_url: Rc<dyn Fn() -> Option<String>>,
    pub open_error_context: &'static str,
}

pub(crate) fn build(
    parent: &(impl IsA<gtk::Widget> + Clone + 'static),
    input: WebContextMenuInput,
) -> gtk::Popover {
    let popover = context_menu::popover(parent);
    let menu = context_menu::menu_box();

    let reload_button = context_menu::action_button("Reload", Some("F5"));
    {
        let reload = input.reload.clone();
        let popover = popover.clone();
        reload_button.connect_clicked(move |_| {
            reload();
            popover.popdown();
        });
    }
    menu.append(&reload_button);

    let copy_url_button = context_menu::action_button("Copy URL", None);
    {
        let current_url = input.current_url.clone();
        let popover = popover.clone();
        let parent = parent.clone();
        copy_url_button.connect_clicked(move |_| {
            if let Some(url) = current_url()
                && !url.trim().is_empty()
            {
                parent.display().clipboard().set_text(&url);
            }
            popover.popdown();
        });
    }
    menu.append(&copy_url_button);

    let open_external_button = context_menu::action_button("Open in Browser", None);
    {
        let current_url = input.current_url.clone();
        let popover = popover.clone();
        let open_error_context = input.open_error_context;
        open_external_button.connect_clicked(move |_| {
            if let Some(url) = current_url()
                && !url.trim().is_empty()
                && let Err(error) =
                    gio::AppInfo::launch_default_for_uri(&url, None::<&gio::AppLaunchContext>)
            {
                logging::error(format!(
                    "{open_error_context} open failed for '{url}': {error}"
                ));
            }
            popover.popdown();
        });
    }
    menu.append(&open_external_button);

    popover.set_child(Some(&menu));
    popover
}

pub(crate) fn install_right_click(
    parent: &(impl IsA<gtk::Widget> + Clone + 'static),
    input: WebContextMenuInput,
) -> gtk::Popover {
    let popover = build(parent, input);
    let right_click = gtk::GestureClick::builder()
        .button(3)
        .propagation_phase(gtk::PropagationPhase::Capture)
        .build();
    {
        let parent = parent.clone();
        let popover = popover.clone();
        right_click.connect_pressed(move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            parent.grab_focus();
            context_menu::popup_at(&popover, x, y);
        });
    }
    parent.add_controller(right_click);
    popover
}
