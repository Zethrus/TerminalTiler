use adw::prelude::*;

use crate::ui::icons::{self, name as icon_name};

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
