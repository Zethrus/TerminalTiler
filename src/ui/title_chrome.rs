use adw::prelude::*;

use crate::ui::icons::{self, name as icon_name};

pub(crate) struct TitleTabChrome {
    pub(crate) shell: gtk::Box,
    pub(crate) select_button: gtk::Button,
    pub(crate) close_button: gtk::Button,
    pub(crate) title_label: gtk::Label,
    #[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
    pub(crate) badge_label: gtk::Label,
}

#[derive(Clone)]
pub(crate) struct TitleChrome {
    pub(crate) root: gtk::Box,
    pub(crate) tabs_box: gtk::Box,
    pub(crate) add_button: gtk::Button,
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
    select_row.append(
        &gtk::Image::builder()
            .icon_name(icon_name::TERMINAL)
            .valign(gtk::Align::Center)
            .css_classes(["app-tab-icon"])
            .build(),
    );

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

    #[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
    let badge_label = {
        let badge_label = gtk::Label::builder()
            .visible(false)
            .css_classes(["app-tab-badge"])
            .build();
        select_row.append(&badge_label);
        badge_label
    };

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
        #[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
        badge_label,
    }
}
