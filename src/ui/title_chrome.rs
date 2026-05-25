use adw::prelude::*;
use std::rc::Rc;

use crate::ui::icons::{self, name as icon_name};

#[derive(Clone)]
pub(crate) struct TitleTabChrome {
    pub(crate) shell: gtk::Box,
    pub(crate) select_button: gtk::Button,
    pub(crate) close_button: gtk::Button,
    pub(crate) title_label: gtk::Label,
}

#[derive(Clone)]
pub(crate) struct TitleChrome {
    pub(crate) root: gtk::Box,
    pub(crate) tabs_box: gtk::Box,
    pub(crate) add_button: gtk::Button,
}

pub(crate) struct TitleTabInput {
    pub(crate) label: String,
    pub(crate) tooltip: String,
    pub(crate) active: bool,
    pub(crate) close_enabled: bool,
    pub(crate) on_select: Option<Rc<dyn Fn()>>,
    pub(crate) on_rename: Option<Rc<dyn Fn()>>,
    pub(crate) on_close: Option<Rc<dyn Fn()>>,
}

impl TitleChrome {
    pub(crate) fn new() -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .halign(gtk::Align::Center)
            .build();

        let tabs_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .halign(gtk::Align::Center)
            .build();
        tabs_box.add_css_class("app-tab-strip");

        let add_button = icons::icon_button(
            icon_name::ADD,
            "New workspace tab",
            &["flat", "app-tab-add"],
        );
        root.append(&tabs_box);
        root.append(&add_button);

        Self {
            root,
            tabs_box,
            add_button,
        }
    }
}

pub(crate) fn build_title_tab_chrome() -> TitleTabChrome {
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .css_classes(["app-tab-shell", "is-inactive"])
        .build();

    let select_button = gtk::Button::builder()
        .css_classes(["app-tab-select"])
        .hexpand(true)
        .focus_on_click(false)
        .build();

    let select_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .build();
    // Resolve through icons::image so the custom "hover:" scheme loads the bundled
    // SVG instead of GTK falling back to the missing-icon (exclamation) glyph.
    let tab_icon = icons::image(icon_name::TERMINAL);
    tab_icon.set_valign(gtk::Align::Center);
    tab_icon.set_pixel_size(14);
    tab_icon.add_css_class("app-tab-icon");
    select_row.append(&tab_icon);

    let title_label = gtk::Label::builder()
        .xalign(0.0)
        .hexpand(true)
        .single_line_mode(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .width_chars(14)
        .max_width_chars(14)
        .css_classes(["app-tab-title"])
        .build();
    select_row.append(&title_label);

    select_button.set_child(Some(&select_row));
    shell.append(&select_button);

    let close_button =
        icons::icon_button(icon_name::CLOSE, "Close tab", &["flat", "app-tab-close"]);
    shell.append(&close_button);

    TitleTabChrome {
        shell,
        select_button,
        close_button,
        title_label,
    }
}

pub(crate) fn apply_title_tab_state(
    chrome: &TitleTabChrome,
    label: &str,
    tooltip: &str,
    active: bool,
    close_enabled: bool,
) {
    chrome.shell.set_tooltip_text(Some(tooltip));
    chrome.shell.remove_css_class("is-inactive");
    chrome.shell.remove_css_class("is-active");
    chrome
        .shell
        .add_css_class(if active { "is-active" } else { "is-inactive" });
    chrome.title_label.set_label(label);
    chrome.close_button.set_sensitive(close_enabled);
}

pub(crate) fn build_interactive_title_tab(input: TitleTabInput) -> TitleTabChrome {
    let chrome = build_title_tab_chrome();
    apply_title_tab_state(
        &chrome,
        &input.label,
        &input.tooltip,
        input.active,
        input.close_enabled,
    );

    if let Some(on_select) = input.on_select {
        chrome.select_button.connect_clicked(move |_| on_select());
    }

    if let Some(on_rename) = input.on_rename {
        let rename_click = gtk::GestureClick::builder()
            .button(1)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        rename_click.connect_pressed(move |gesture, n_press, _, _| {
            if n_press != 2 {
                return;
            }
            gesture.set_state(gtk::EventSequenceState::Claimed);
            on_rename();
        });
        chrome.select_button.add_controller(rename_click);
    }

    chrome.close_button.set_focus_on_click(false);
    if let Some(on_close) = input.on_close {
        let on_middle_close = on_close.clone();
        chrome.close_button.connect_clicked(move |_| on_close());

        let middle_close = gtk::GestureClick::builder()
            .button(2)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        middle_close.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            on_middle_close();
        });
        chrome.shell.add_controller(middle_close);
    }

    chrome
}
