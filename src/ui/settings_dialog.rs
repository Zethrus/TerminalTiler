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
        .default_width(560)
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
        .orientation(gtk::Orientation::Horizontal)
        .spacing(14)
        .css_classes(["config-panel", "settings-section", "settings-hero"])
        .build();

    let intro_icon = gtk::Box::builder()
        .width_request(42)
        .height_request(42)
        .valign(gtk::Align::Start)
        .css_classes(["settings-hero-icon"])
        .build();
    intro_icon.append(&gtk::Image::from_icon_name("preferences-system-symbolic"));
    intro.append(&intro_icon);

    let intro_body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .hexpand(true)
        .build();
    intro.append(&intro_body);

    let intro_top = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();
    let title = gtk::Label::builder()
        .label("Application Settings")
        .halign(gtk::Align::Start)
        .hexpand(true)
        .css_classes(["section-title", "settings-title"])
        .build();
    intro_top.append(&title);
    intro_top.append(&build_meta_chip("Saved automatically"));
    intro_body.append(&intro_top);

    let body = gtk::Label::builder()
        .label("Set defaults for new launch tabs and keep a few high-value controls close at hand. Running workspaces keep their own preset theme, and the density hotkey only changes the active workspace.")
        .halign(gtk::Align::Start)
        .wrap(true)
        .css_classes(["field-hint", "settings-copy"])
        .build();
    intro_body.append(&body);

    let intro_note = gtk::Label::builder()
        .label("Launch defaults are immediate. Workspace presets still take over after a workspace starts.")
        .halign(gtk::Align::Start)
        .wrap(true)
        .css_classes(["settings-inline-note"])
        .build();
    intro_body.append(&intro_note);
    content.append(&intro);

    let theme_callback = Rc::new(on_theme_changed);
    let density_callback = Rc::new(on_density_changed);

    let theme_section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel", "settings-section"])
        .build();
    content.append(&theme_section);

    theme_section.append(&build_section_header(
        "Default Theme",
        "New launch tabs",
        "Used when opening or editing launch tabs. Workspace presets still control the theme after launch.",
    ));

    let theme_strip = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .css_classes(["control-strip", "settings-choice-strip"])
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
    theme_section.append(&theme_strip);

    let density_section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel", "settings-section"])
        .build();
    content.append(&density_section);

    density_section.append(&build_section_header(
        "Default Application Density",
        "Window shell",
        "Affects new launch tabs and the window shell. Use Ctrl+Shift+D inside a workspace to cycle density for only that active workspace.",
    ));

    let density_strip = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .css_classes(["control-strip", "settings-choice-strip"])
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
    density_section.append(&density_strip);
    density_section.append(
        &gtk::Label::builder()
            .label("Workspace quick toggle: Comfortable -> Standard -> Compact")
            .halign(gtk::Align::Start)
            .css_classes(["settings-inline-note"])
            .build(),
    );

    let shortcuts_section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel", "settings-section"])
        .build();
    content.append(&shortcuts_section);

    shortcuts_section.append(&build_section_header(
        "Shortcuts",
        "Available now",
        "These shortcuts do not require opening Settings and are meant to keep workspace adjustments fast.",
    ));
    shortcuts_section.append(&build_shortcut_row(
        "Toggle workspace fullscreen",
        "F11",
        "Available only while a workspace tab is active.",
    ));
    shortcuts_section.append(&gtk::Separator::builder().orientation(gtk::Orientation::Horizontal).build());
    shortcuts_section.append(&build_shortcut_row(
        "Cycle active workspace density",
        "Ctrl+Shift+D",
        "Rotates only the current workspace without changing the saved app default.",
    ));

    dialog.connect_response(move |dialog, _| {
        dialog.close();
    });

    dialog.present();
}

fn build_shortcut_row(label: &str, keys: &str, note: &str) -> gtk::Widget {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .valign(gtk::Align::Center)
        .css_classes(["settings-shortcut-row"])
        .build();

    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .build();
    text.append(
        &gtk::Label::builder()
            .label(label)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .wrap(true)
            .css_classes(["settings-shortcut-title"])
            .build(),
    );
    text.append(
        &gtk::Label::builder()
            .label(note)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .wrap(true)
            .css_classes(["field-hint", "settings-shortcut-note"])
            .build(),
    );
    row.append(&text);

    row.append(
        &gtk::Label::builder()
            .label(keys)
            .halign(gtk::Align::End)
            .valign(gtk::Align::Center)
            .css_classes(["status-chip", "settings-shortcut-chip"])
            .build(),
    );
    row.upcast()
}

fn build_section_header(title: &str, meta: &str, body: &str) -> gtk::Widget {
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();

    let top = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();
    top.append(
        &gtk::Label::builder()
            .label(title)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .css_classes(["eyebrow", "settings-section-heading"])
            .build(),
    );
    top.append(&build_meta_chip(meta));
    shell.append(&top);

    shell.append(
        &gtk::Label::builder()
            .label(body)
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint", "settings-copy"])
            .build(),
    );

    shell.upcast()
}

fn build_meta_chip(label: &str) -> gtk::Widget {
    gtk::Label::builder()
        .label(label)
        .halign(gtk::Align::End)
        .css_classes(["status-chip", "settings-meta-chip"])
        .build()
        .upcast()
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