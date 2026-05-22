use adw::prelude::*;

use crate::ui::title_chrome::TitleChrome;

pub(crate) struct AppHeaderChrome {
    pub(crate) header: adw::HeaderBar,
    pub(crate) title: TitleChrome,
}

pub(crate) fn build_app_header_chrome() -> AppHeaderChrome {
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(true)
        .show_end_title_buttons(true)
        .build();
    header.set_centering_policy(adw::CenteringPolicy::Loose);
    header.add_css_class("app-headerbar");

    let title = TitleChrome::new();
    title.root.add_css_class("app-title-handle");
    header.set_title_widget(Some(&title.root));

    AppHeaderChrome { header, title }
}

pub(crate) fn build_window_shell() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build()
}
