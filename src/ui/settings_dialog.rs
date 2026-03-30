use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use std::cell::{Cell, RefCell};

use crate::model::preset::{ApplicationDensity, ThemeMode};
use crate::storage::preference_store::AppPreferences;

fn shortcut_display_label(shortcut: &str) -> String {
    gtk::accelerator_parse(shortcut)
        .map(|(key, modifiers)| gtk::accelerator_get_label(key, modifiers).to_string())
        .unwrap_or_else(|| shortcut.to_string())
}

fn sync_shortcut_capture_label(label: &gtk::Label, shortcut: &str) {
    label.set_label(&shortcut_display_label(shortcut));
    label.set_tooltip_text(Some(shortcut));
}

fn set_recorder_idle(record_button: &gtk::Button, status: &gtk::Label) {
    record_button.remove_css_class("is-recording");
    record_button.set_label("Record");
    status.set_visible(false);
    status.set_label("");
}

fn set_recorder_recording(record_button: &gtk::Button, status: &gtk::Label) {
    record_button.add_css_class("is-recording");
    record_button.set_label("Press keys...");
    status.set_label("Listening for a shortcut. Press Esc to cancel.");
    status.set_visible(true);
}

fn fallback_shortcut_key_from_keycode(
    display: &gdk::Display,
    keycode: u32,
) -> Option<(gdk::Key, gdk::ModifierType)> {
    display.map_keycode(keycode).and_then(|entries| {
        entries
            .into_iter()
            .filter_map(|(keymap_key, mapped_key)| {
                let priority = match mapped_key.name().as_deref() {
                    Some(name) if name.starts_with("KP_") => 0,
                    _ if mapped_key.to_unicode().is_some() => 1,
                    _ => 2,
                };
                let consumed = if keymap_key.level() > 0 {
                    gdk::ModifierType::SHIFT_MASK
                } else {
                    gdk::ModifierType::empty()
                };
                Some((priority, keymap_key.level(), mapped_key, consumed))
            })
            .min_by_key(|(priority, level, _, _)| (*priority, *level))
            .map(|(_, _, mapped_key, consumed)| (mapped_key, consumed))
    })
}

fn normalize_captured_shortcut(
    controller: &gtk::EventControllerKey,
    key: gdk::Key,
    keycode: u32,
    state: gdk::ModifierType,
) -> Option<(String, String)> {
    let default_modifiers = state & gtk::accelerator_get_default_mod_mask();
    let mut normalized_key = key;
    let mut consumed_modifiers = gdk::ModifierType::empty();

    if let Some(display) = gdk::Display::default() {
        if let Some((translated_key, _, _, consumed)) =
            display.translate_key(keycode, state, controller.group() as i32)
        {
            normalized_key = translated_key;
            consumed_modifiers = consumed & gtk::accelerator_get_default_mod_mask();
        }

        if matches!(normalized_key.name().as_deref(), Some("ClearGrab"))
            && let Some((mapped_key, mapped_consumed)) =
                fallback_shortcut_key_from_keycode(&display, keycode)
        {
            normalized_key = mapped_key;
            consumed_modifiers = mapped_consumed;
        }
    }

    let modifiers = default_modifiers & !consumed_modifiers;
    if !gtk::accelerator_valid(normalized_key, modifiers) {
        return None;
    }

    let shortcut = gtk::accelerator_name(normalized_key, modifiers).to_string();
    let label = gtk::accelerator_get_label(normalized_key, modifiers).to_string();
    Some((shortcut, label))
}

fn sync_reset_button_state(
    reset_button: &gtk::Button,
    theme: ThemeMode,
    density: ApplicationDensity,
    fullscreen_shortcut: &str,
    density_shortcut: &str,
    zoom_in_shortcut: &str,
    zoom_out_shortcut: &str,
) {
    let defaults = AppPreferences::default();
    reset_button.set_sensitive(
        theme != defaults.default_theme
            || density != defaults.default_density
            || fullscreen_shortcut != defaults.workspace_fullscreen_shortcut
            || density_shortcut != defaults.workspace_density_shortcut
            || zoom_in_shortcut != defaults.workspace_zoom_in_shortcut
            || zoom_out_shortcut != defaults.workspace_zoom_out_shortcut,
    );
}

fn default_settings_dialog_size(
    window: &adw::ApplicationWindow,
    saved_width: i32,
    saved_height: i32,
) -> (i32, i32) {
    let width = match window.width() {
        width if width > 0 => (width - 32).min(saved_width).max(200),
        _ => saved_width.max(200),
    };
    let height = match window.height() {
        height if height > 0 => (height - 48).min(saved_height).max(240),
        _ => saved_height.max(240),
    };

    (width, height)
}

fn persist_dialog_size(dialog: &gtk::Dialog, on_size_changed: &Rc<dyn Fn(i32, i32)>) {
    let width = dialog.width();
    let height = dialog.height();
    if width > 0 && height > 0 {
        on_size_changed(width, height);
    }
}

#[allow(deprecated)]
pub fn present<F, G, H, I, J, K, L, M>(
    window: &adw::ApplicationWindow,
    default_theme: ThemeMode,
    default_density: ApplicationDensity,
    workspace_fullscreen_shortcut: String,
    workspace_density_shortcut: String,
    workspace_zoom_in_shortcut: String,
    workspace_zoom_out_shortcut: String,
    settings_dialog_width: i32,
    settings_dialog_height: i32,
    on_theme_changed: F,
    on_density_changed: G,
    on_fullscreen_shortcut_changed: H,
    on_density_shortcut_changed: I,
    on_zoom_in_shortcut_changed: J,
    on_zoom_out_shortcut_changed: K,
    on_reset_defaults: L,
    on_size_changed: M,
) where
    F: Fn(ThemeMode) + 'static,
    G: Fn(ApplicationDensity) + 'static,
    H: Fn(String) + 'static,
    I: Fn(String) + 'static,
    J: Fn(String) + 'static,
    K: Fn(String) + 'static,
    L: Fn() + 'static,
    M: Fn(i32, i32) + 'static,
{
    let (default_width, default_height) =
        default_settings_dialog_size(window, settings_dialog_width, settings_dialog_height);
    let dialog = gtk::Dialog::builder()
        .modal(true)
        .transient_for(window)
        .title("Application Settings")
        .default_width(default_width)
        .default_height(default_height)
        .resizable(true)
        .build();
    dialog.add_button("Close", gtk::ResponseType::Close);
    dialog.set_default_response(gtk::ResponseType::Close);

    let content_area = dialog.content_area();
    content_area.set_vexpand(true);
    let scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .css_classes(["settings-dialog-scroller"])
        .build();
    scroller.set_has_frame(false);
    content_area.append(&scroller);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    scroller.set_child(Some(&content));
    content.set_spacing(12);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.add_css_class("settings-dialog-content");

    let current_theme = Rc::new(Cell::new(default_theme));
    let current_density = Rc::new(Cell::new(default_density));
    let current_fullscreen_shortcut = Rc::new(RefCell::new(workspace_fullscreen_shortcut));
    let current_density_shortcut = Rc::new(RefCell::new(workspace_density_shortcut));
    let current_zoom_in_shortcut = Rc::new(RefCell::new(workspace_zoom_in_shortcut));
    let current_zoom_out_shortcut = Rc::new(RefCell::new(workspace_zoom_out_shortcut));
    let reset_button = gtk::Button::with_label("Reset Defaults");
    reset_button.add_css_class("pill-button");
    reset_button.add_css_class("secondary-button");
    sync_reset_button_state(
        &reset_button,
        current_theme.get(),
        current_density.get(),
        current_fullscreen_shortcut.borrow().as_str(),
        current_density_shortcut.borrow().as_str(),
        current_zoom_in_shortcut.borrow().as_str(),
        current_zoom_out_shortcut.borrow().as_str(),
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
        .label("Set defaults for new launch tabs and keep a few high-value controls close at hand. Running workspaces keep their own preset theme, and workspace shortcuts only affect the active workspace.")
        .halign(gtk::Align::Start)
        .wrap(true)
        .css_classes(["field-hint", "settings-copy"])
        .build();
    intro_body.append(&body);

    let intro_note = gtk::Label::builder()
        .label("Launch defaults are immediate. Running workspace zoom is session-scoped and restored with saved workspaces.")
        .halign(gtk::Align::Start)
        .css_classes(["settings-inline-note"])
        .build();
    intro_body.append(&intro_note);
    content.append(&intro);

    let theme_callback = Rc::new(on_theme_changed);
    let density_callback = Rc::new(on_density_changed);
    let fullscreen_shortcut_callback = Rc::new(on_fullscreen_shortcut_changed);
    let density_shortcut_callback = Rc::new(on_density_shortcut_changed);
    let zoom_in_shortcut_callback = Rc::new(on_zoom_in_shortcut_changed);
    let zoom_out_shortcut_callback = Rc::new(on_zoom_out_shortcut_changed);
    let reset_callback = Rc::new(on_reset_defaults);
    let size_changed_callback: Rc<dyn Fn(i32, i32)> = Rc::new(on_size_changed);

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
        let current_zoom_in_shortcut = current_zoom_in_shortcut.clone();
        let current_zoom_out_shortcut = current_zoom_out_shortcut.clone();
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
                    current_fullscreen_shortcut.borrow().as_str(),
                    current_density_shortcut.borrow().as_str(),
                    current_zoom_in_shortcut.borrow().as_str(),
                    current_zoom_out_shortcut.borrow().as_str(),
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
        let current_zoom_in_shortcut = current_zoom_in_shortcut.clone();
        let current_zoom_out_shortcut = current_zoom_out_shortcut.clone();
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
                    current_fullscreen_shortcut.borrow().as_str(),
                    current_density_shortcut.borrow().as_str(),
                    current_zoom_in_shortcut.borrow().as_str(),
                    current_zoom_out_shortcut.borrow().as_str(),
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
        "Choose default workspace shortcuts that fit your desktop environment. Click Record, then press the shortcut you want. Changes take effect in the current window immediately.",
    ));
    let fullscreen_status = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .css_classes([
            "field-hint",
            "settings-shortcut-note",
            "settings-shortcut-status",
        ])
        .visible(false)
        .build();
    let fullscreen_capture_label = gtk::Label::builder()
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["status-chip", "settings-shortcut-chip"])
        .build();
    sync_shortcut_capture_label(
        &fullscreen_capture_label,
        current_fullscreen_shortcut.borrow().as_str(),
    );
    let fullscreen_record_button = gtk::Button::with_label("Record");
    fullscreen_record_button.add_css_class("pill-button");
    fullscreen_record_button.add_css_class("secondary-button");
    fullscreen_record_button.add_css_class("settings-shortcut-record-button");
    let fullscreen_control =
        build_shortcut_capture_control(&fullscreen_capture_label, &fullscreen_record_button);
    let fullscreen_recording = Rc::new(Cell::new(false));
    {
        let current_theme = current_theme.clone();
        let current_density = current_density.clone();
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let current_density_shortcut = current_density_shortcut.clone();
        let current_zoom_in_shortcut = current_zoom_in_shortcut.clone();
        let current_zoom_out_shortcut = current_zoom_out_shortcut.clone();
        let reset_button = reset_button.clone();
        let status = fullscreen_status.clone();
        let capture_label = fullscreen_capture_label.clone();
        let record_button = fullscreen_record_button.clone();
        let recording = fullscreen_recording.clone();
        let callback = fullscreen_shortcut_callback.clone();
        let key_controller = gtk::EventControllerKey::new();
        key_controller.connect_key_pressed(move |controller, key, keycode, state| {
            if !recording.get() {
                return glib::Propagation::Proceed;
            }

            let modifiers = state & gtk::accelerator_get_default_mod_mask();
            if key == gdk::Key::Escape && modifiers.is_empty() {
                recording.set(false);
                set_recorder_idle(&record_button, &status);
                return glib::Propagation::Stop;
            }

            let Some((shortcut, label)) =
                normalize_captured_shortcut(controller, key, keycode, state)
            else {
                status.set_label(
                    "That key cannot be used alone. Try a function key or add modifiers.",
                );
                status.set_visible(true);
                return glib::Propagation::Stop;
            };

            recording.set(false);
            set_recorder_idle(&record_button, &status);
            capture_label.set_label(&label);
            capture_label.set_tooltip_text(Some(&shortcut));
            if current_fullscreen_shortcut.borrow().as_str() != shortcut {
                current_fullscreen_shortcut.replace(shortcut.clone());
                callback(shortcut);
                sync_reset_button_state(
                    &reset_button,
                    current_theme.get(),
                    current_density.get(),
                    current_fullscreen_shortcut.borrow().as_str(),
                    current_density_shortcut.borrow().as_str(),
                    current_zoom_in_shortcut.borrow().as_str(),
                    current_zoom_out_shortcut.borrow().as_str(),
                );
            }
            glib::Propagation::Stop
        });
        fullscreen_record_button.add_controller(key_controller);
    }
    {
        let status = fullscreen_status.clone();
        let record_button = fullscreen_record_button.clone();
        let recording = fullscreen_recording.clone();
        fullscreen_record_button.connect_clicked(move |button| {
            if recording.get() {
                recording.set(false);
                set_recorder_idle(&record_button, &status);
            } else {
                recording.set(true);
                set_recorder_recording(&record_button, &status);
                button.grab_focus();
            }
        });
    }
    {
        let status = fullscreen_status.clone();
        let record_button = fullscreen_record_button.clone();
        let recording = fullscreen_recording.clone();
        fullscreen_record_button.connect_notify_local(Some("has-focus"), move |button, _| {
            if recording.get() && !button.has_focus() {
                recording.set(false);
                set_recorder_idle(&record_button, &status);
            }
        });
    }
    shortcuts_section.append(&build_shortcut_entry_row(
        "Toggle workspace fullscreen",
        "Available only while a workspace tab is active.",
        &fullscreen_control,
        &fullscreen_status,
        &["F11", "<Shift>F11", "<Ctrl>F11"],
    ));

    shortcuts_section.append(
        &gtk::Separator::builder()
            .orientation(gtk::Orientation::Horizontal)
            .build(),
    );
    let density_status = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .css_classes([
            "field-hint",
            "settings-shortcut-note",
            "settings-shortcut-status",
        ])
        .visible(false)
        .build();
    let density_capture_label = gtk::Label::builder()
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["status-chip", "settings-shortcut-chip"])
        .build();
    sync_shortcut_capture_label(
        &density_capture_label,
        current_density_shortcut.borrow().as_str(),
    );
    let density_record_button = gtk::Button::with_label("Record");
    density_record_button.add_css_class("pill-button");
    density_record_button.add_css_class("secondary-button");
    density_record_button.add_css_class("settings-shortcut-record-button");
    let density_control =
        build_shortcut_capture_control(&density_capture_label, &density_record_button);
    let density_recording = Rc::new(Cell::new(false));
    {
        let current_theme = current_theme.clone();
        let current_density = current_density.clone();
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let current_density_shortcut = current_density_shortcut.clone();
        let current_zoom_in_shortcut = current_zoom_in_shortcut.clone();
        let current_zoom_out_shortcut = current_zoom_out_shortcut.clone();
        let reset_button = reset_button.clone();
        let status = density_status.clone();
        let capture_label = density_capture_label.clone();
        let record_button = density_record_button.clone();
        let recording = density_recording.clone();
        let callback = density_shortcut_callback.clone();
        let key_controller = gtk::EventControllerKey::new();
        key_controller.connect_key_pressed(move |controller, key, keycode, state| {
            if !recording.get() {
                return glib::Propagation::Proceed;
            }

            let modifiers = state & gtk::accelerator_get_default_mod_mask();
            if key == gdk::Key::Escape && modifiers.is_empty() {
                recording.set(false);
                set_recorder_idle(&record_button, &status);
                return glib::Propagation::Stop;
            }

            let Some((shortcut, label)) =
                normalize_captured_shortcut(controller, key, keycode, state)
            else {
                status.set_label(
                    "That key cannot be used alone. Try a function key or add modifiers.",
                );
                status.set_visible(true);
                return glib::Propagation::Stop;
            };

            recording.set(false);
            set_recorder_idle(&record_button, &status);
            capture_label.set_label(&label);
            capture_label.set_tooltip_text(Some(&shortcut));
            if current_density_shortcut.borrow().as_str() != shortcut {
                current_density_shortcut.replace(shortcut.clone());
                callback(shortcut);
                sync_reset_button_state(
                    &reset_button,
                    current_theme.get(),
                    current_density.get(),
                    current_fullscreen_shortcut.borrow().as_str(),
                    current_density_shortcut.borrow().as_str(),
                    current_zoom_in_shortcut.borrow().as_str(),
                    current_zoom_out_shortcut.borrow().as_str(),
                );
            }
            glib::Propagation::Stop
        });
        density_record_button.add_controller(key_controller);
    }
    {
        let status = density_status.clone();
        let record_button = density_record_button.clone();
        let recording = density_recording.clone();
        density_record_button.connect_clicked(move |button| {
            if recording.get() {
                recording.set(false);
                set_recorder_idle(&record_button, &status);
            } else {
                recording.set(true);
                set_recorder_recording(&record_button, &status);
                button.grab_focus();
            }
        });
    }
    {
        let status = density_status.clone();
        let record_button = density_record_button.clone();
        let recording = density_recording.clone();
        density_record_button.connect_notify_local(Some("has-focus"), move |button, _| {
            if recording.get() && !button.has_focus() {
                recording.set(false);
                set_recorder_idle(&record_button, &status);
            }
        });
    }
    shortcuts_section.append(&build_shortcut_entry_row(
        "Cycle active workspace density",
        "Rotates only the current workspace without changing the saved app default.",
        &density_control,
        &density_status,
        &["<Ctrl><Shift>D", "<Shift>F8", "<Alt><Super>D"],
    ));

    shortcuts_section.append(
        &gtk::Separator::builder()
            .orientation(gtk::Orientation::Horizontal)
            .build(),
    );
    let zoom_in_status = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .css_classes([
            "field-hint",
            "settings-shortcut-note",
            "settings-shortcut-status",
        ])
        .visible(false)
        .build();
    let zoom_in_capture_label = gtk::Label::builder()
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["status-chip", "settings-shortcut-chip"])
        .build();
    sync_shortcut_capture_label(
        &zoom_in_capture_label,
        current_zoom_in_shortcut.borrow().as_str(),
    );
    let zoom_in_record_button = gtk::Button::with_label("Record");
    zoom_in_record_button.add_css_class("pill-button");
    zoom_in_record_button.add_css_class("secondary-button");
    zoom_in_record_button.add_css_class("settings-shortcut-record-button");
    let zoom_in_control =
        build_shortcut_capture_control(&zoom_in_capture_label, &zoom_in_record_button);
    let zoom_in_recording = Rc::new(Cell::new(false));
    {
        let current_theme = current_theme.clone();
        let current_density = current_density.clone();
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let current_density_shortcut = current_density_shortcut.clone();
        let current_zoom_in_shortcut = current_zoom_in_shortcut.clone();
        let current_zoom_out_shortcut = current_zoom_out_shortcut.clone();
        let reset_button = reset_button.clone();
        let status = zoom_in_status.clone();
        let capture_label = zoom_in_capture_label.clone();
        let record_button = zoom_in_record_button.clone();
        let recording = zoom_in_recording.clone();
        let callback = zoom_in_shortcut_callback.clone();
        let key_controller = gtk::EventControllerKey::new();
        key_controller.connect_key_pressed(move |controller, key, keycode, state| {
            if !recording.get() {
                return glib::Propagation::Proceed;
            }

            let modifiers = state & gtk::accelerator_get_default_mod_mask();
            if key == gdk::Key::Escape && modifiers.is_empty() {
                recording.set(false);
                set_recorder_idle(&record_button, &status);
                return glib::Propagation::Stop;
            }

            let Some((shortcut, label)) =
                normalize_captured_shortcut(controller, key, keycode, state)
            else {
                status.set_label(
                    "That key cannot be used alone. Try a function key or add modifiers.",
                );
                status.set_visible(true);
                return glib::Propagation::Stop;
            };

            recording.set(false);
            set_recorder_idle(&record_button, &status);
            capture_label.set_label(&label);
            capture_label.set_tooltip_text(Some(&shortcut));
            if current_zoom_in_shortcut.borrow().as_str() != shortcut {
                current_zoom_in_shortcut.replace(shortcut.clone());
                callback(shortcut);
                sync_reset_button_state(
                    &reset_button,
                    current_theme.get(),
                    current_density.get(),
                    current_fullscreen_shortcut.borrow().as_str(),
                    current_density_shortcut.borrow().as_str(),
                    current_zoom_in_shortcut.borrow().as_str(),
                    current_zoom_out_shortcut.borrow().as_str(),
                );
            }
            glib::Propagation::Stop
        });
        zoom_in_record_button.add_controller(key_controller);
    }
    {
        let status = zoom_in_status.clone();
        let record_button = zoom_in_record_button.clone();
        let recording = zoom_in_recording.clone();
        zoom_in_record_button.connect_clicked(move |button| {
            if recording.get() {
                recording.set(false);
                set_recorder_idle(&record_button, &status);
            } else {
                recording.set(true);
                set_recorder_recording(&record_button, &status);
                button.grab_focus();
            }
        });
    }
    {
        let status = zoom_in_status.clone();
        let record_button = zoom_in_record_button.clone();
        let recording = zoom_in_recording.clone();
        zoom_in_record_button.connect_notify_local(Some("has-focus"), move |button, _| {
            if recording.get() && !button.has_focus() {
                recording.set(false);
                set_recorder_idle(&record_button, &status);
            }
        });
    }
    shortcuts_section.append(&build_shortcut_entry_row(
        "Zoom in terminal text",
        "Applies only to the active workspace and is restored with saved workspace sessions.",
        &zoom_in_control,
        &zoom_in_status,
        &["<Ctrl>plus", "<Ctrl>equal", "<Ctrl>KP_Add"],
    ));

    shortcuts_section.append(
        &gtk::Separator::builder()
            .orientation(gtk::Orientation::Horizontal)
            .build(),
    );
    let zoom_out_status = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .css_classes([
            "field-hint",
            "settings-shortcut-note",
            "settings-shortcut-status",
        ])
        .visible(false)
        .build();
    let zoom_out_capture_label = gtk::Label::builder()
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["status-chip", "settings-shortcut-chip"])
        .build();
    sync_shortcut_capture_label(
        &zoom_out_capture_label,
        current_zoom_out_shortcut.borrow().as_str(),
    );
    let zoom_out_record_button = gtk::Button::with_label("Record");
    zoom_out_record_button.add_css_class("pill-button");
    zoom_out_record_button.add_css_class("secondary-button");
    zoom_out_record_button.add_css_class("settings-shortcut-record-button");
    let zoom_out_control =
        build_shortcut_capture_control(&zoom_out_capture_label, &zoom_out_record_button);
    let zoom_out_recording = Rc::new(Cell::new(false));
    {
        let current_theme = current_theme.clone();
        let current_density = current_density.clone();
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let current_density_shortcut = current_density_shortcut.clone();
        let current_zoom_in_shortcut = current_zoom_in_shortcut.clone();
        let current_zoom_out_shortcut = current_zoom_out_shortcut.clone();
        let reset_button = reset_button.clone();
        let status = zoom_out_status.clone();
        let capture_label = zoom_out_capture_label.clone();
        let record_button = zoom_out_record_button.clone();
        let recording = zoom_out_recording.clone();
        let callback = zoom_out_shortcut_callback.clone();
        let key_controller = gtk::EventControllerKey::new();
        key_controller.connect_key_pressed(move |controller, key, keycode, state| {
            if !recording.get() {
                return glib::Propagation::Proceed;
            }

            let modifiers = state & gtk::accelerator_get_default_mod_mask();
            if key == gdk::Key::Escape && modifiers.is_empty() {
                recording.set(false);
                set_recorder_idle(&record_button, &status);
                return glib::Propagation::Stop;
            }

            let Some((shortcut, label)) =
                normalize_captured_shortcut(controller, key, keycode, state)
            else {
                status.set_label(
                    "That key cannot be used alone. Try a function key or add modifiers.",
                );
                status.set_visible(true);
                return glib::Propagation::Stop;
            };

            recording.set(false);
            set_recorder_idle(&record_button, &status);
            capture_label.set_label(&label);
            capture_label.set_tooltip_text(Some(&shortcut));
            if current_zoom_out_shortcut.borrow().as_str() != shortcut {
                current_zoom_out_shortcut.replace(shortcut.clone());
                callback(shortcut);
                sync_reset_button_state(
                    &reset_button,
                    current_theme.get(),
                    current_density.get(),
                    current_fullscreen_shortcut.borrow().as_str(),
                    current_density_shortcut.borrow().as_str(),
                    current_zoom_in_shortcut.borrow().as_str(),
                    current_zoom_out_shortcut.borrow().as_str(),
                );
            }
            glib::Propagation::Stop
        });
        zoom_out_record_button.add_controller(key_controller);
    }
    {
        let status = zoom_out_status.clone();
        let record_button = zoom_out_record_button.clone();
        let recording = zoom_out_recording.clone();
        zoom_out_record_button.connect_clicked(move |button| {
            if recording.get() {
                recording.set(false);
                set_recorder_idle(&record_button, &status);
            } else {
                recording.set(true);
                set_recorder_recording(&record_button, &status);
                button.grab_focus();
            }
        });
    }
    {
        let status = zoom_out_status.clone();
        let record_button = zoom_out_record_button.clone();
        let recording = zoom_out_recording.clone();
        zoom_out_record_button.connect_notify_local(Some("has-focus"), move |button, _| {
            if recording.get() && !button.has_focus() {
                recording.set(false);
                set_recorder_idle(&record_button, &status);
            }
        });
    }
    shortcuts_section.append(&build_shortcut_entry_row(
        "Zoom out terminal text",
        "Applies only to the active workspace and is restored with saved workspace sessions.",
        &zoom_out_control,
        &zoom_out_status,
        &["<Ctrl>minus", "<Ctrl>KP_Subtract"],
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
        let current_zoom_in_shortcut = current_zoom_in_shortcut.clone();
        let current_zoom_out_shortcut = current_zoom_out_shortcut.clone();
        let theme_strip = theme_strip.clone();
        let density_strip = density_strip.clone();
        let fullscreen_capture_label = fullscreen_capture_label.clone();
        let density_capture_label = density_capture_label.clone();
        let zoom_in_capture_label = zoom_in_capture_label.clone();
        let zoom_out_capture_label = zoom_out_capture_label.clone();
        let fullscreen_record_button = fullscreen_record_button.clone();
        let density_record_button = density_record_button.clone();
        let zoom_in_record_button = zoom_in_record_button.clone();
        let zoom_out_record_button = zoom_out_record_button.clone();
        let fullscreen_status = fullscreen_status.clone();
        let density_status = density_status.clone();
        let zoom_in_status = zoom_in_status.clone();
        let zoom_out_status = zoom_out_status.clone();
        let fullscreen_recording = fullscreen_recording.clone();
        let density_recording = density_recording.clone();
        let zoom_in_recording = zoom_in_recording.clone();
        let zoom_out_recording = zoom_out_recording.clone();
        let reset_button = reset_button.clone();
        let reset_button_for_signal = reset_button.clone();
        let reset_callback = reset_callback.clone();
        reset_button_for_signal.connect_clicked(move |_| {
            let defaults = AppPreferences::default();
            let changed = current_theme.get() != defaults.default_theme
                || current_density.get() != defaults.default_density
                || current_fullscreen_shortcut.borrow().as_str()
                    != defaults.workspace_fullscreen_shortcut
                || current_density_shortcut.borrow().as_str()
                    != defaults.workspace_density_shortcut
                || current_zoom_in_shortcut.borrow().as_str()
                    != defaults.workspace_zoom_in_shortcut
                || current_zoom_out_shortcut.borrow().as_str()
                    != defaults.workspace_zoom_out_shortcut;
            if !changed {
                return;
            }

            current_theme.set(defaults.default_theme);
            current_density.set(defaults.default_density);
            current_fullscreen_shortcut.replace(defaults.workspace_fullscreen_shortcut.clone());
            current_density_shortcut.replace(defaults.workspace_density_shortcut.clone());
            current_zoom_in_shortcut.replace(defaults.workspace_zoom_in_shortcut.clone());
            current_zoom_out_shortcut.replace(defaults.workspace_zoom_out_shortcut.clone());
            sync_theme_strip_active(&theme_strip, defaults.default_theme);
            sync_density_strip_active(&density_strip, defaults.default_density);
            sync_shortcut_capture_label(
                &fullscreen_capture_label,
                &defaults.workspace_fullscreen_shortcut,
            );
            sync_shortcut_capture_label(
                &density_capture_label,
                &defaults.workspace_density_shortcut,
            );
            sync_shortcut_capture_label(
                &zoom_in_capture_label,
                &defaults.workspace_zoom_in_shortcut,
            );
            sync_shortcut_capture_label(
                &zoom_out_capture_label,
                &defaults.workspace_zoom_out_shortcut,
            );
            fullscreen_recording.set(false);
            density_recording.set(false);
            zoom_in_recording.set(false);
            zoom_out_recording.set(false);
            set_recorder_idle(&fullscreen_record_button, &fullscreen_status);
            set_recorder_idle(&density_record_button, &density_status);
            set_recorder_idle(&zoom_in_record_button, &zoom_in_status);
            set_recorder_idle(&zoom_out_record_button, &zoom_out_status);
            sync_reset_button_state(
                &reset_button,
                defaults.default_theme,
                defaults.default_density,
                &defaults.workspace_fullscreen_shortcut,
                &defaults.workspace_density_shortcut,
                &defaults.workspace_zoom_in_shortcut,
                &defaults.workspace_zoom_out_shortcut,
            );
            reset_callback();
        });
    }
    actions.append(&reset_button);
    content.append(&actions);

    {
        let size_changed_callback = size_changed_callback.clone();
        dialog.connect_response(move |dialog, _| {
            persist_dialog_size(dialog, &size_changed_callback);
            dialog.close();
        });
    }

    {
        let size_changed_callback = size_changed_callback.clone();
        dialog.connect_close_request(move |dialog| {
            persist_dialog_size(dialog, &size_changed_callback);
            glib::Propagation::Proceed
        });
    }

    dialog.present();
}

fn build_shortcut_capture_control(
    value_label: &impl IsA<gtk::Widget>,
    record_button: &impl IsA<gtk::Widget>,
) -> gtk::Widget {
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .css_classes(["settings-shortcut-control-shell"])
        .build();
    shell.append(value_label);
    shell.append(record_button);
    shell.upcast()
}

fn build_shortcut_entry_row(
    label: &str,
    note: &str,
    control: &impl IsA<gtk::Widget>,
    status: &gtk::Label,
    examples: &[&str],
) -> gtk::Widget {
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();

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

    let trailing = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .css_classes(["settings-shortcut-trailing"])
        .build();
    trailing.append(control);
    trailing.append(&build_shortcut_help_button(label, examples));
    row.append(&trailing);

    shell.append(&row);
    shell.append(status);
    shell.upcast()
}

fn build_shortcut_help_button(title: &str, examples: &[&str]) -> gtk::Widget {
    let button = gtk::MenuButton::new();
    button.set_icon_name("dialog-question-symbolic");
    button.set_tooltip_text(Some("Show shortcut syntax examples"));
    button.set_valign(gtk::Align::Center);
    button.add_css_class("flat");
    button.add_css_class("circular");
    button.add_css_class("settings-help-button");

    let popover = gtk::Popover::new();
    popover.add_css_class("settings-help-popover");

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(10)
        .margin_end(10)
        .build();
    body.append(
        &gtk::Label::builder()
            .label(title)
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["settings-help-title"])
            .build(),
    );
    body.append(
        &gtk::Label::builder()
            .label("Click Record, then press the shortcut you want to use. Press Esc while recording to cancel.")
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint", "settings-help-copy"])
            .build(),
    );

    let examples_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .css_classes(["settings-help-examples"])
        .build();
    examples_box.append(
        &gtk::Label::builder()
            .label("Examples")
            .halign(gtk::Align::Start)
            .css_classes(["eyebrow", "settings-help-eyebrow"])
            .build(),
    );
    for example in examples {
        examples_box.append(
            &gtk::Label::builder()
                .label(*example)
                .halign(gtk::Align::Start)
                .selectable(true)
                .css_classes(["settings-help-example"])
                .build(),
        );
    }
    body.append(&examples_box);

    popover.set_child(Some(&body));
    button.set_popover(Some(&popover));
    button.upcast()
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
