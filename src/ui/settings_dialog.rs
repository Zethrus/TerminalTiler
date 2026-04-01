use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use std::cell::{Cell, RefCell};

use crate::model::preset::{ApplicationDensity, ThemeMode};
use crate::storage::preference_store::AppPreferences;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SettingsState {
    theme: ThemeMode,
    density: ApplicationDensity,
    close_to_background: bool,
    fullscreen_shortcut: String,
    density_shortcut: String,
    zoom_in_shortcut: String,
    zoom_out_shortcut: String,
}

impl SettingsState {
    fn defaults() -> Self {
        let defaults = AppPreferences::default();
        Self {
            theme: defaults.default_theme,
            density: defaults.default_density,
            close_to_background: defaults.close_to_background,
            fullscreen_shortcut: defaults.workspace_fullscreen_shortcut,
            density_shortcut: defaults.workspace_density_shortcut,
            zoom_in_shortcut: defaults.workspace_zoom_in_shortcut,
            zoom_out_shortcut: defaults.workspace_zoom_out_shortcut,
        }
    }
}

pub struct SettingsDialogInput {
    pub default_theme: ThemeMode,
    pub default_density: ApplicationDensity,
    pub close_to_background: bool,
    pub workspace_fullscreen_shortcut: String,
    pub workspace_density_shortcut: String,
    pub workspace_zoom_in_shortcut: String,
    pub workspace_zoom_out_shortcut: String,
    pub settings_dialog_width: i32,
    pub settings_dialog_height: i32,
}

#[derive(Clone)]
pub struct SettingsDialogActions {
    pub on_theme_changed: Rc<dyn Fn(ThemeMode)>,
    pub on_density_changed: Rc<dyn Fn(ApplicationDensity)>,
    pub on_close_to_background_changed: Rc<dyn Fn(bool)>,
    pub on_fullscreen_shortcut_changed: Rc<dyn Fn(String)>,
    pub on_density_shortcut_changed: Rc<dyn Fn(String)>,
    pub on_zoom_in_shortcut_changed: Rc<dyn Fn(String)>,
    pub on_zoom_out_shortcut_changed: Rc<dyn Fn(String)>,
    pub on_reset_defaults: Rc<dyn Fn()>,
    pub on_reset_builtin_presets: Rc<dyn Fn()>,
    pub on_size_changed: Rc<dyn Fn(i32, i32)>,
}

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
        let mapped_entries = entries
            .into_iter()
            .map(|(keymap_key, mapped_key)| {
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
                (priority, keymap_key.level(), mapped_key, consumed)
            })
            .collect::<Vec<_>>();
        mapped_entries
            .into_iter()
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

fn sync_reset_button_state(reset_button: &gtk::Button, current: &SettingsState) {
    reset_button.set_sensitive(current != &SettingsState::defaults());
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

fn sync_dialog_chrome_classes(window: &adw::ApplicationWindow, dialog: &gtk::Dialog) {
    dialog.add_css_class("settings-dialog-window");
    for class_name in [
        "theme-light",
        "theme-dark",
        "profile-comfortable",
        "profile-standard",
        "profile-compact",
    ] {
        dialog.remove_css_class(class_name);
        if window.has_css_class(class_name) {
            dialog.add_css_class(class_name);
        }
    }
}

#[allow(deprecated)]
pub fn present(
    window: &adw::ApplicationWindow,
    input: SettingsDialogInput,
    actions: SettingsDialogActions,
) {
    let SettingsDialogInput {
        default_theme,
        default_density,
        close_to_background,
        workspace_fullscreen_shortcut,
        workspace_density_shortcut,
        workspace_zoom_in_shortcut,
        workspace_zoom_out_shortcut,
        settings_dialog_width,
        settings_dialog_height,
    } = input;
    let SettingsDialogActions {
        on_theme_changed,
        on_density_changed,
        on_close_to_background_changed,
        on_fullscreen_shortcut_changed,
        on_density_shortcut_changed,
        on_zoom_in_shortcut_changed,
        on_zoom_out_shortcut_changed,
        on_reset_defaults,
        on_reset_builtin_presets,
        on_size_changed,
    } = actions;
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
    sync_dialog_chrome_classes(window, &dialog);
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
    let current_close_to_background = Rc::new(Cell::new(close_to_background));
    let current_fullscreen_shortcut = Rc::new(RefCell::new(workspace_fullscreen_shortcut));
    let current_density_shortcut = Rc::new(RefCell::new(workspace_density_shortcut));
    let current_zoom_in_shortcut = Rc::new(RefCell::new(workspace_zoom_in_shortcut));
    let current_zoom_out_shortcut = Rc::new(RefCell::new(workspace_zoom_out_shortcut));
    let reset_button = gtk::Button::with_label("Reset Defaults");
    reset_button.add_css_class("pill-button");
    reset_button.add_css_class("secondary-button");
    reset_button.add_css_class("settings-reset-button");
    let sync_reset_button: Rc<dyn Fn()> = {
        let current_theme = current_theme.clone();
        let current_density = current_density.clone();
        let current_close_to_background = current_close_to_background.clone();
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let current_density_shortcut = current_density_shortcut.clone();
        let current_zoom_in_shortcut = current_zoom_in_shortcut.clone();
        let current_zoom_out_shortcut = current_zoom_out_shortcut.clone();
        let reset_button = reset_button.clone();
        Rc::new(move || {
            sync_reset_button_state(
                &reset_button,
                &SettingsState {
                    theme: current_theme.get(),
                    density: current_density.get(),
                    close_to_background: current_close_to_background.get(),
                    fullscreen_shortcut: current_fullscreen_shortcut.borrow().clone(),
                    density_shortcut: current_density_shortcut.borrow().clone(),
                    zoom_in_shortcut: current_zoom_in_shortcut.borrow().clone(),
                    zoom_out_shortcut: current_zoom_out_shortcut.borrow().clone(),
                },
            );
        })
    };
    sync_reset_button();

    content.append(&build_settings_summary(&reset_button));

    let theme_callback = on_theme_changed;
    let density_callback = on_density_changed;
    let close_to_background_callback = on_close_to_background_changed;
    let fullscreen_shortcut_callback = on_fullscreen_shortcut_changed;
    let density_shortcut_callback = on_density_shortcut_changed;
    let zoom_in_shortcut_callback = on_zoom_in_shortcut_changed;
    let zoom_out_shortcut_callback = on_zoom_out_shortcut_changed;
    let reset_callback = on_reset_defaults;
    let size_changed_callback = on_size_changed;

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
        let sync_reset_button = sync_reset_button.clone();
        let theme_callback = theme_callback.clone();
        button.connect_clicked(move |_| {
            if current_theme.get() != mode {
                current_theme.set(mode);
                theme_callback(mode);
                sync_theme_strip_active(&theme_strip_ref, mode);
                sync_reset_button();
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
        let sync_reset_button = sync_reset_button.clone();
        let density_callback = density_callback.clone();
        button.connect_clicked(move |_| {
            if current_density.get() != density {
                current_density.set(density);
                density_callback(density);
                sync_density_strip_active(&density_strip_ref, density);
                sync_reset_button();
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

    let background_section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["config-panel", "settings-section"])
        .build();
    content.append(&background_section);

    background_section.append(&build_section_header(
        "Background Behavior",
        "Tray fallback aware",
        "When enabled, closing the window hides TerminalTiler to the system tray instead of quitting. If no tray watcher is available, close falls back to the normal quit path.",
    ));

    let background_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(["settings-toggle-row"])
        .build();
    let background_text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    background_text.append(
        &gtk::Label::builder()
            .label("Close button hides to background")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .wrap(true)
            .css_classes(["settings-shortcut-title"])
            .build(),
    );
    background_text.append(
        &gtk::Label::builder()
            .label("Tray menu provides Show / Restore, Open Settings, and Quit while the window is hidden.")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .wrap(true)
            .css_classes(["field-hint", "settings-shortcut-note"])
            .build(),
    );
    background_row.append(&background_text);

    let close_to_background_switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .active(close_to_background)
        .build();
    close_to_background_switch.add_css_class("settings-toggle-switch");
    let suppress_close_to_background_signal = Rc::new(Cell::new(false));
    {
        let current_close_to_background = current_close_to_background.clone();
        let sync_reset_button = sync_reset_button.clone();
        let callback = close_to_background_callback.clone();
        let suppress_signal = suppress_close_to_background_signal.clone();
        close_to_background_switch.connect_active_notify(move |switch| {
            if suppress_signal.get() {
                return;
            }

            let is_active = switch.is_active();
            if current_close_to_background.get() == is_active {
                return;
            }

            current_close_to_background.set(is_active);
            callback(is_active);
            sync_reset_button();
        });
    }
    background_row.append(&close_to_background_switch);
    background_section.append(&background_row);

    let presets_section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["config-panel", "settings-section"])
        .build();
    content.append(&presets_section);

    presets_section.append(&build_section_header(
        "Saved Presets",
        "Factory restore",
        "Restore the shipped saved presets after deleting or editing them. User-created presets are kept exactly as they are.",
    ));
    presets_section.append(&build_settings_action_row(
        "Reset default saved presets",
        "Replaces the built-in saved presets with the original shipped versions and leaves every user preset untouched.",
        "Reset Default Saved Presets",
        {
            let callback = on_reset_builtin_presets.clone();
            move || callback()
        },
    ));

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
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let sync_reset_button = sync_reset_button.clone();
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
                sync_reset_button();
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
        let current_density_shortcut = current_density_shortcut.clone();
        let sync_reset_button = sync_reset_button.clone();
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
                sync_reset_button();
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
        let current_zoom_in_shortcut = current_zoom_in_shortcut.clone();
        let sync_reset_button = sync_reset_button.clone();
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
                sync_reset_button();
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
        let current_zoom_out_shortcut = current_zoom_out_shortcut.clone();
        let sync_reset_button = sync_reset_button.clone();
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
                sync_reset_button();
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

    {
        let current_theme = current_theme.clone();
        let current_density = current_density.clone();
        let current_close_to_background = current_close_to_background.clone();
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let current_density_shortcut = current_density_shortcut.clone();
        let current_zoom_in_shortcut = current_zoom_in_shortcut.clone();
        let current_zoom_out_shortcut = current_zoom_out_shortcut.clone();
        let theme_strip = theme_strip.clone();
        let density_strip = density_strip.clone();
        let close_to_background_switch = close_to_background_switch.clone();
        let suppress_close_to_background_signal = suppress_close_to_background_signal.clone();
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
                || current_close_to_background.get() != defaults.close_to_background
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
            current_close_to_background.set(defaults.close_to_background);
            current_fullscreen_shortcut.replace(defaults.workspace_fullscreen_shortcut.clone());
            current_density_shortcut.replace(defaults.workspace_density_shortcut.clone());
            current_zoom_in_shortcut.replace(defaults.workspace_zoom_in_shortcut.clone());
            current_zoom_out_shortcut.replace(defaults.workspace_zoom_out_shortcut.clone());
            sync_theme_strip_active(&theme_strip, defaults.default_theme);
            sync_density_strip_active(&density_strip, defaults.default_density);
            suppress_close_to_background_signal.set(true);
            close_to_background_switch.set_active(defaults.close_to_background);
            suppress_close_to_background_signal.set(false);
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
            sync_reset_button();
            reset_callback();
        });
    }
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
        .css_classes([
            "settings-shortcut-control-shell",
            "settings-shortcut-controls",
        ])
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
        .spacing(10)
        .css_classes(["settings-shortcut-card"])
        .build();

    let top = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
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
    top.append(&text);
    top.append(&build_shortcut_help_button(label, examples));
    shell.append(&top);

    let controls = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .css_classes(["settings-shortcut-trailing"])
        .build();
    controls.append(control);
    shell.append(&controls);

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
        .css_classes(["settings-section-header"])
        .build();

    let top = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .css_classes(["settings-section-top"])
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
            .css_classes(["field-hint", "settings-copy", "settings-section-copy"])
            .build(),
    );

    shell.upcast()
}

fn build_settings_action_row<F>(
    title: &str,
    note: &str,
    button_label: &str,
    on_click: F,
) -> gtk::Widget
where
    F: Fn() + 'static,
{
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(["settings-toggle-row"])
        .build();

    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    text.append(
        &gtk::Label::builder()
            .label(title)
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
    shell.append(&text);

    let button = gtk::Button::with_label(button_label);
    button.add_css_class("pill-button");
    button.add_css_class("secondary-button");
    button.connect_clicked(move |_| on_click());
    shell.append(&button);

    shell.upcast()
}

fn build_settings_summary(reset_button: &gtk::Button) -> gtk::Widget {
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(14)
        .css_classes(["config-panel", "settings-section", "settings-summary"])
        .build();

    let icon = gtk::Box::builder()
        .width_request(40)
        .height_request(40)
        .valign(gtk::Align::Start)
        .css_classes(["settings-summary-icon"])
        .build();
    icon.append(&gtk::Image::from_icon_name("preferences-system-symbolic"));
    shell.append(&icon);

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .css_classes(["settings-summary-body"])
        .build();
    body.append(
        &gtk::Label::builder()
            .label("Application Settings")
            .halign(gtk::Align::Start)
            .css_classes(["section-title", "settings-title", "settings-summary-title"])
            .build(),
    );
    body.append(
        &gtk::Label::builder()
            .label("Set launch defaults, tray behavior, and workspace shortcuts in one place. Changes apply immediately, while workspace zoom stays session-scoped.")
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint", "settings-copy", "settings-summary-copy"])
            .build(),
    );
    shell.append(&body);

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .valign(gtk::Align::Center)
        .css_classes(["settings-summary-actions"])
        .build();
    actions.append(&build_meta_chip("Saved automatically"));
    actions.append(&build_meta_chip("Defaults live"));
    actions.append(reset_button);
    shell.append(&actions);

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
