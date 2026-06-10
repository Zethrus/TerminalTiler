use adw::prelude::*;

use crate::ui::icons::{self, name as icon_name};
use crate::ui::title_chrome::TitleChrome;

pub(crate) struct AppHeaderChrome {
    pub(crate) header: adw::HeaderBar,
    pub(crate) title: TitleChrome,
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(crate) struct MainTitlebarActions {
    pub(crate) back_button: gtk::Button,
    pub(crate) fullscreen_button: gtk::Button,
    pub(crate) settings_button: gtk::Button,
    pub(crate) companion_button: Option<gtk::Button>,
    pub(crate) assets_button: gtk::Button,
}

pub(crate) fn build_app_header_chrome() -> AppHeaderChrome {
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(true)
        .show_end_title_buttons(true)
        .build();
    header.set_centering_policy(adw::CenteringPolicy::Loose);
    apply_app_headerbar_class(&header);

    let title = TitleChrome::new();
    title.root.add_css_class("app-title-handle");
    header.set_title_widget(Some(&title.root));

    AppHeaderChrome { header, title }
}

pub(crate) fn apply_app_headerbar_class(header: &adw::HeaderBar) {
    header.add_css_class("app-headerbar");
}

pub(crate) fn sync_workspace_fullscreen_chrome(
    window: &adw::ApplicationWindow,
    title_widget: &gtk::Widget,
    fullscreen_button: &gtk::Button,
    is_workspace: bool,
    enter_tooltip: &str,
    exit_tooltip: &str,
) {
    if !is_workspace {
        title_widget.set_visible(true);
        fullscreen_button.set_visible(false);
        if window.is_fullscreen() {
            window.set_fullscreened(false);
        }
        return;
    }

    let is_fullscreen = window.is_fullscreen();
    title_widget.set_visible(!is_fullscreen);
    fullscreen_button.set_visible(true);
    if is_fullscreen {
        icons::set_button_icon_label_fitted(
            fullscreen_button,
            "Exit Fullscreen",
            icon_name::RESTORE,
        );
        fullscreen_button.set_tooltip_text(Some(exit_tooltip));
    } else {
        icons::set_button_icon_label_fitted(fullscreen_button, "Fullscreen", icon_name::FULLSCREEN);
        fullscreen_button.set_tooltip_text(Some(enter_tooltip));
    }
}

pub(crate) fn build_main_titlebar_actions(
    header: &adw::HeaderBar,
    include_companion: bool,
) -> MainTitlebarActions {
    let back_button = icons::labeled_button_fitted(
        "Templates",
        icon_name::LAYOUT,
        &["flat", "titlebar-action-button"],
    );
    back_button.set_visible(false);
    header.pack_start(&back_button);

    let fullscreen_button = icons::labeled_button_fitted(
        "Fullscreen",
        icon_name::FULLSCREEN,
        &["flat", "titlebar-action-button"],
    );
    fullscreen_button.set_tooltip_text(Some("Enter fullscreen"));
    fullscreen_button.set_visible(false);
    header.pack_end(&fullscreen_button);

    let settings_button = icons::icon_button(
        icon_name::SETTINGS,
        "Open application settings",
        &["flat", "titlebar-action-button", "titlebar-icon-button"],
    );
    header.pack_end(&settings_button);

    let companion_button = include_companion.then(|| {
        let button = icons::labeled_button_fitted(
            "Account / Sync",
            icon_name::WEB,
            &["flat", "titlebar-action-button"],
        );
        header.pack_end(&button);
        button
    });

    let assets_button = icons::icon_button(
        icon_name::ASSETS,
        "Open assets manager",
        &["flat", "titlebar-action-button", "titlebar-icon-button"],
    );
    header.pack_end(&assets_button);

    MainTitlebarActions {
        back_button,
        fullscreen_button,
        settings_button,
        companion_button,
        assets_button,
    }
}

pub(crate) fn build_window_shell() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build()
}
