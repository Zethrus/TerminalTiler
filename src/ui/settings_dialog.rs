use std::rc::Rc;

use adw::prelude::*;
use std::cell::Cell;

use crate::model::preset::{
    ApplicationDensity, ThemeMode, WorkspaceDensityShortcut, WorkspaceFullscreenShortcut,
};
use crate::storage::preference_store::AppPreferences;

fn sync_reset_button_state(
    reset_button: &gtk::Button,
    theme: ThemeMode,
    density: ApplicationDensity,
    fullscreen_shortcut: WorkspaceFullscreenShortcut,
    density_shortcut: WorkspaceDensityShortcut,
) {
    let defaults = AppPreferences::default();
    reset_button.set_sensitive(
        theme != defaults.default_theme
            || density != defaults.default_density
            || fullscreen_shortcut != defaults.workspace_fullscreen_shortcut
            || density_shortcut != defaults.workspace_density_shortcut,
    );
}

#[allow(deprecated)]
pub fn present<F, G, H, I, J>(
    window: &adw::ApplicationWindow,
    default_theme: ThemeMode,
    default_density: ApplicationDensity,
    workspace_fullscreen_shortcut: WorkspaceFullscreenShortcut,
    workspace_density_shortcut: WorkspaceDensityShortcut,
    on_theme_changed: F,
    on_density_changed: G,
    on_fullscreen_shortcut_changed: H,
    on_density_shortcut_changed: I,
    on_reset_defaults: J,
) where
    F: Fn(ThemeMode) + 'static,
    G: Fn(ApplicationDensity) + 'static,
    H: Fn(WorkspaceFullscreenShortcut) + 'static,
    I: Fn(WorkspaceDensityShortcut) + 'static,
    J: Fn() + 'static,
{
    let dialog = gtk::Dialog::builder()
        .modal(true)
        .transient_for(window)
        .title("Application Settings")
        .default_width(528)
        .build();
    dialog.add_button("Close", gtk::ResponseType::Close);
    dialog.set_default_response(gtk::ResponseType::Close);

    let content = dialog.content_area();
    content.set_spacing(12);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.add_css_class("settings-dialog-content");

    let current_theme = Rc::new(Cell::new(default_theme));
    let current_density = Rc::new(Cell::new(default_density));
    let current_fullscreen_shortcut = Rc::new(Cell::new(workspace_fullscreen_shortcut));
    let current_density_shortcut = Rc::new(Cell::new(workspace_density_shortcut));
    let reset_button = gtk::Button::with_label("Reset Defaults");
    reset_button.add_css_class("pill-button");
    reset_button.add_css_class("secondary-button");
    sync_reset_button_state(
        &reset_button,
        current_theme.get(),
        current_density.get(),
        current_fullscreen_shortcut.get(),
        current_density_shortcut.get(),
    );

    let intro = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
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
        .spacing(6)
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
        .css_classes(["settings-inline-note"])
        .build();
    intro_body.append(&intro_note);
    content.append(&intro);

    let theme_callback = Rc::new(on_theme_changed);
    let density_callback = Rc::new(on_density_changed);
    let fullscreen_shortcut_callback = Rc::new(on_fullscreen_shortcut_changed);
    let density_shortcut_callback = Rc::new(on_density_shortcut_changed);
    let reset_callback = Rc::new(on_reset_defaults);

    let theme_section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
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

        let current_theme = current_theme.clone();
        let theme_strip_ref = theme_strip.clone();
        let current_density = current_density.clone();
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let current_density_shortcut = current_density_shortcut.clone();
        let reset_button = reset_button.clone();
        let theme_callback = theme_callback.clone();
        button.connect_clicked(move |_| {
            if current_theme.get() != mode {
                current_theme.set(mode);
                theme_callback(mode);
                sync_theme_strip_active(&theme_strip_ref, mode);
                sync_reset_button_state(
                    &reset_button,
                    mode,
                    current_density.get(),
                    current_fullscreen_shortcut.get(),
                    current_density_shortcut.get(),
                );
            }
        });
        theme_strip.append(&button);
    }
    theme_section.append(&theme_strip);

    let density_section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["config-panel", "settings-section"])
        .build();
    content.append(&density_section);

    density_section.append(&build_section_header(
        "Default Application Density",
        "Window shell",
        "Affects new launch tabs and the window shell. Running workspaces keep their own density, and the workspace shortcut below only changes the active workspace.",
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

        let current_density = current_density.clone();
        let density_strip_ref = density_strip.clone();
        let current_theme = current_theme.clone();
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let current_density_shortcut = current_density_shortcut.clone();
        let reset_button = reset_button.clone();
        let density_callback = density_callback.clone();
        button.connect_clicked(move |_| {
            if current_density.get() != density {
                current_density.set(density);
                density_callback(density);
                sync_density_strip_active(&density_strip_ref, density);
                sync_reset_button_state(
                    &reset_button,
                    current_theme.get(),
                    density,
                    current_fullscreen_shortcut.get(),
                    current_density_shortcut.get(),
                );
            }
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
        "Apply immediately",
        "Choose default workspace shortcuts that fit your desktop environment. These take effect in the current window as soon as you change them.",
    ));
    let fullscreen_options = WorkspaceFullscreenShortcut::all();
    let fullscreen_option_labels = fullscreen_options
        .iter()
        .map(WorkspaceFullscreenShortcut::label)
        .collect::<Vec<_>>();
    let fullscreen_dropdown = gtk::DropDown::builder()
        .model(&gtk::StringList::new(&fullscreen_option_labels))
        .selected(
            fullscreen_options
                .iter()
                .position(|candidate| *candidate == workspace_fullscreen_shortcut)
                .unwrap_or(0) as u32,
        )
        .build();
    fullscreen_dropdown.add_css_class("settings-shortcut-control");
    {
        let current_theme = current_theme.clone();
        let current_density = current_density.clone();
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let current_density_shortcut = current_density_shortcut.clone();
        let reset_button = reset_button.clone();
        let callback = fullscreen_shortcut_callback.clone();
        fullscreen_dropdown.connect_selected_notify(move |dropdown| {
            let Some(shortcut) = WorkspaceFullscreenShortcut::all()
                .get(dropdown.selected() as usize)
                .copied()
            else {
                return;
            };
            if current_fullscreen_shortcut.get() != shortcut {
                current_fullscreen_shortcut.set(shortcut);
                callback(shortcut);
                sync_reset_button_state(
                    &reset_button,
                    current_theme.get(),
                    current_density.get(),
                    shortcut,
                    current_density_shortcut.get(),
                );
            }
        });
    }
    shortcuts_section.append(&build_shortcut_selector_row(
        "Toggle workspace fullscreen",
        "Available only while a workspace tab is active.",
        &fullscreen_dropdown,
    ));

    shortcuts_section.append(&gtk::Separator::builder().orientation(gtk::Orientation::Horizontal).build());
    let density_options = WorkspaceDensityShortcut::all();
    let density_option_labels = density_options
        .iter()
        .map(WorkspaceDensityShortcut::label)
        .collect::<Vec<_>>();
    let density_dropdown = gtk::DropDown::builder()
        .model(&gtk::StringList::new(&density_option_labels))
        .selected(
            density_options
                .iter()
                .position(|candidate| *candidate == workspace_density_shortcut)
                .unwrap_or(0) as u32,
        )
        .build();
    density_dropdown.add_css_class("settings-shortcut-control");
    {
        let current_theme = current_theme.clone();
        let current_density = current_density.clone();
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let current_density_shortcut = current_density_shortcut.clone();
        let reset_button = reset_button.clone();
        let callback = density_shortcut_callback.clone();
        density_dropdown.connect_selected_notify(move |dropdown| {
            let Some(shortcut) = WorkspaceDensityShortcut::all()
                .get(dropdown.selected() as usize)
                .copied()
            else {
                return;
            };
            if current_density_shortcut.get() != shortcut {
                current_density_shortcut.set(shortcut);
                callback(shortcut);
                sync_reset_button_state(
                    &reset_button,
                    current_theme.get(),
                    current_density.get(),
                    current_fullscreen_shortcut.get(),
                    shortcut,
                );
            }
        });
    }
    shortcuts_section.append(&build_shortcut_selector_row(
        "Cycle active workspace density",
        "Rotates only the current workspace without changing the saved app default.",
        &density_dropdown,
    ));

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(["settings-actions"])
        .build();
    actions.append(
        &gtk::Label::builder()
            .label("Defaults apply immediately to new launch tabs.")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .css_classes(["field-hint", "settings-footer-note"])
            .build(),
    );

    {
        let current_theme = current_theme.clone();
        let current_density = current_density.clone();
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let current_density_shortcut = current_density_shortcut.clone();
        let theme_strip = theme_strip.clone();
        let density_strip = density_strip.clone();
        let fullscreen_dropdown = fullscreen_dropdown.clone();
        let density_dropdown = density_dropdown.clone();
        let reset_button = reset_button.clone();
        let reset_button_for_signal = reset_button.clone();
        let reset_callback = reset_callback.clone();
        reset_button_for_signal.connect_clicked(move |_| {
            let defaults = AppPreferences::default();
            let changed = current_theme.get() != defaults.default_theme
                || current_density.get() != defaults.default_density
                || current_fullscreen_shortcut.get() != defaults.workspace_fullscreen_shortcut
                || current_density_shortcut.get() != defaults.workspace_density_shortcut;
            if !changed {
                return;
            }

            current_theme.set(defaults.default_theme);
            current_density.set(defaults.default_density);
            current_fullscreen_shortcut.set(defaults.workspace_fullscreen_shortcut);
            current_density_shortcut.set(defaults.workspace_density_shortcut);
            sync_theme_strip_active(&theme_strip, defaults.default_theme);
            sync_density_strip_active(&density_strip, defaults.default_density);
            fullscreen_dropdown.set_selected(
                WorkspaceFullscreenShortcut::all()
                    .iter()
                    .position(|candidate| *candidate == defaults.workspace_fullscreen_shortcut)
                    .unwrap_or(0) as u32,
            );
            density_dropdown.set_selected(
                WorkspaceDensityShortcut::all()
                    .iter()
                    .position(|candidate| *candidate == defaults.workspace_density_shortcut)
                    .unwrap_or(0) as u32,
            );
            sync_reset_button_state(
                &reset_button,
                defaults.default_theme,
                defaults.default_density,
                defaults.workspace_fullscreen_shortcut,
                defaults.workspace_density_shortcut,
            );
            reset_callback();
        });
    }
    actions.append(&reset_button);
    content.append(&actions);

    dialog.connect_response(move |dialog, _| {
        dialog.close();
    });

    dialog.present();
}

fn build_shortcut_selector_row(label: &str, note: &str, control: &impl IsA<gtk::Widget>) -> gtk::Widget {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .valign(gtk::Align::Center)
        .css_classes(["settings-shortcut-row"])
        .build();

    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
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
    row.append(control);
    row.upcast()
}
fn build_section_header(title: &str, meta: &str, body: &str) -> gtk::Widget {
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
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