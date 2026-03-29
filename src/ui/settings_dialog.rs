use std::rc::Rc;

use adw::prelude::*;

use crate::model::preset::{ApplicationDensity, ThemeMode};

#[allow(deprecated)]
pub fn present<F, G>(
    window: &adw::ApplicationWindow,
    default_theme: ThemeMode,
    default_density: ApplicationDensity,
    on_theme_changed: F,
    on_density_changed: G,
) where
    F: Fn(ThemeMode) + 'static,
    G: Fn(ApplicationDensity) + 'static,
{
    let dialog = gtk::Dialog::builder()
        .modal(true)
        .transient_for(window)
        .title("Application Settings")
        .default_width(520)
        .build();
    dialog.add_button("Close", gtk::ResponseType::Close);
    dialog.set_default_response(gtk::ResponseType::Close);

    let content = dialog.content_area();
    content.set_spacing(18);
    content.set_margin_top(20);
    content.set_margin_bottom(20);
    content.set_margin_start(20);
    content.set_margin_end(20);
    content.add_css_class("settings-dialog-content");

    let intro = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .css_classes(["config-panel", "settings-section"])
        .build();

    let title = gtk::Label::builder()
        .label("Application Settings")
        .halign(gtk::Align::Start)
        .css_classes(["section-title", "settings-title"])
        .build();
    let body = gtk::Label::builder()
        .label("Manage defaults for new launch tabs. Running workspaces keep their own preset theme, and the workspace density hotkey only changes the active workspace.")
        .halign(gtk::Align::Start)
        .wrap(true)
        .css_classes(["field-hint"])
        .build();
    intro.append(&title);
    intro.append(&body);
    content.append(&intro);

    let theme_callback = Rc::new(on_theme_changed);
    let density_callback = Rc::new(on_density_changed);

    let theme_section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel", "settings-section"])
        .build();
    content.append(&theme_section);

    let theme_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let theme_label = gtk::Label::builder()
        .label("Default Theme")
        .halign(gtk::Align::Start)
        .hexpand(true)
        .css_classes(["eyebrow"])
        .build();
    theme_row.append(&theme_label);

    let theme_strip = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .css_classes(["control-strip"])
        .build();
    for (mode, label) in [
        (ThemeMode::System, "System"),
        (ThemeMode::Light, "Light"),
        (ThemeMode::Dark, "Dark"),
    ] {
        let button = gtk::Button::with_label(label);
        button.add_css_class("flat");
        if mode == default_theme {
            button.add_css_class("is-active");
        }

        let theme_strip_ref = theme_strip.clone();
        let theme_callback = theme_callback.clone();
        button.connect_clicked(move |_| {
            theme_callback(mode);
            sync_theme_strip_active(&theme_strip_ref, mode);
        });
        theme_strip.append(&button);
    }
    theme_row.append(&theme_strip);
    theme_section.append(&theme_row);
    theme_section.append(
        &gtk::Label::builder()
            .label("Used when opening or editing launch tabs. Workspace presets still control the theme after launch.")
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint"])
            .build(),
    );

    let density_section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel", "settings-section"])
        .build();
    content.append(&density_section);

    let density_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let density_label = gtk::Label::builder()
        .label("Default Application Density")
        .halign(gtk::Align::Start)
        .hexpand(true)
        .css_classes(["eyebrow"])
        .build();
    density_row.append(&density_label);

    let density_strip = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .css_classes(["control-strip"])
        .build();
    for (density, label) in [
        (ApplicationDensity::Comfortable, "Comfortable"),
        (ApplicationDensity::Standard, "Standard"),
        (ApplicationDensity::Compact, "Compact"),
    ] {
        let button = gtk::Button::with_label(label);
        button.add_css_class("flat");
        if density == default_density {
            button.add_css_class("is-active");
        }

        let density_strip_ref = density_strip.clone();
        let density_callback = density_callback.clone();
        button.connect_clicked(move |_| {
            density_callback(density);
            sync_density_strip_active(&density_strip_ref, density);
        });
        density_strip.append(&button);
    }
    density_row.append(&density_strip);
    density_section.append(&density_row);
    density_section.append(
        &gtk::Label::builder()
            .label("Affects new launch tabs and the window shell. Use Ctrl+Shift+D inside a workspace to cycle density for only that active workspace.")
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint"])
            .build(),
    );

    let shortcuts_section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel", "settings-section"])
        .build();
    content.append(&shortcuts_section);

    shortcuts_section.append(
        &gtk::Label::builder()
            .label("Shortcuts")
            .halign(gtk::Align::Start)
            .css_classes(["eyebrow"])
            .build(),
    );
    shortcuts_section.append(&build_shortcut_row(
        "Toggle workspace fullscreen",
        "F11",
    ));
    shortcuts_section.append(&build_shortcut_row(
        "Cycle active workspace density",
        "Ctrl+Shift+D",
    ));

    dialog.connect_response(move |dialog, _| {
        dialog.close();
    });

    dialog.present();
}

fn build_shortcut_row(label: &str, keys: &str) -> gtk::Widget {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(["settings-shortcut-row"])
        .build();
    row.append(
        &gtk::Label::builder()
            .label(label)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .wrap(true)
            .css_classes(["card-meta"])
            .build(),
    );
    row.append(
        &gtk::Label::builder()
            .label(keys)
            .halign(gtk::Align::End)
            .css_classes(["status-chip", "settings-shortcut-chip"])
            .build(),
    );
    row.upcast()
}

fn sync_theme_strip_active(strip: &gtk::Box, active_theme: ThemeMode) {
    let mut child = strip.first_child();
    while let Some(widget) = child {
        let next = widget.next_sibling();
        widget.remove_css_class("is-active");
        if let Ok(button) = widget.clone().downcast::<gtk::Button>()
            && button.label().as_deref() == Some(active_theme.label())
        {
            button.add_css_class("is-active");
        }
        child = next;
    }
}

fn sync_density_strip_active(strip: &gtk::Box, active_density: ApplicationDensity) {
    let mut child = strip.first_child();
    while let Some(widget) = child {
        let next = widget.next_sibling();
        widget.remove_css_class("is-active");
        if let Ok(button) = widget.clone().downcast::<gtk::Button>()
            && button.label().as_deref() == Some(active_density.label())
        {
            button.add_css_class("is-active");
        }
        child = next;
    }
}