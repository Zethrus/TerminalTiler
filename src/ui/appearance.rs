use adw::prelude::*;

use crate::model::preset::{ApplicationDensity, ThemeMode};

pub(crate) fn apply_theme_mode(window: &adw::ApplicationWindow, theme: ThemeMode) {
    let manager = adw::StyleManager::default();
    manager.set_color_scheme(match theme {
        ThemeMode::System => adw::ColorScheme::Default,
        ThemeMode::Light => adw::ColorScheme::ForceLight,
        ThemeMode::Dark => adw::ColorScheme::ForceDark,
    });

    window.remove_css_class("theme-light");
    window.remove_css_class("theme-dark");
    window.add_css_class(if manager.is_dark() {
        "theme-dark"
    } else {
        "theme-light"
    });
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn apply_window_density(window: &adw::ApplicationWindow, density: ApplicationDensity) {
    apply_optional_window_density(window, Some(density));
}

pub(crate) fn apply_optional_window_density(
    window: &adw::ApplicationWindow,
    density: Option<ApplicationDensity>,
) {
    window.remove_css_class("profile-comfortable");
    window.remove_css_class("profile-standard");
    window.remove_css_class("profile-compact");

    if let Some(density) = density {
        window.add_css_class(density.css_class());
    }
}

pub(crate) fn resolved_theme_uses_dark_palette(theme: ThemeMode) -> bool {
    match theme {
        ThemeMode::System => adw::StyleManager::default().is_dark(),
        ThemeMode::Light => false,
        ThemeMode::Dark => true,
    }
}

pub(crate) fn window_uses_dark_theme(window: &adw::ApplicationWindow) -> bool {
    window.has_css_class("theme-dark")
}
