use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use std::cell::{Cell, RefCell};

use crate::model::preset::{ApplicationDensity, ThemeMode};
use crate::storage::preference_store::AppPreferences;
use crate::ui::dialog_chrome;
use crate::ui::dialog_smoke;
use crate::ui::icons::{self, name as icon_name};
use crate::voice::audio::MicrophoneDevice;
use crate::voice::{VoiceActivationMode, VoiceEngineMode, VoicePackStatus, VoicePreferences};

#[derive(Clone, Debug, PartialEq, Eq)]
struct SettingsState {
    theme: ThemeMode,
    density: ApplicationDensity,
    close_to_background: bool,
    fullscreen_shortcut: String,
    density_shortcut: String,
    zoom_in_shortcut: String,
    zoom_out_shortcut: String,
    command_palette_shortcut: String,
    max_reconnect_attempts: u32,
    voice: VoicePreferences,
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
            command_palette_shortcut: defaults.command_palette_shortcut,
            max_reconnect_attempts: defaults.max_reconnect_attempts,
            voice: defaults.voice,
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
    pub command_palette_shortcut: String,
    pub settings_dialog_width: i32,
    pub settings_dialog_height: i32,
    pub max_reconnect_attempts: u32,
    pub voice: VoicePreferences,
    pub microphone_devices: Vec<MicrophoneDevice>,
    pub product_display_name: String,
    pub settings_title: String,
    pub settings_summary: String,
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
    pub on_command_palette_shortcut_changed: Rc<dyn Fn(String)>,
    pub on_max_reconnect_attempts_changed: Rc<dyn Fn(u32)>,
    pub on_voice_preferences_changed: Rc<dyn Fn(VoicePreferences)>,
    pub on_voice_pack_install_requested: Rc<dyn Fn()>,
    pub voice_pack_status_provider: Rc<dyn Fn() -> VoicePackStatus>,
    pub on_voice_pack_delete_requested: Rc<dyn Fn()>,
    pub on_voice_pack_health_check_requested: Rc<dyn Fn()>,
    pub on_open_logs_folder: Rc<dyn Fn()>,
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
    icons::set_button_icon_label(record_button, "Record", icon_name::RECORD);
    status.set_visible(false);
    status.set_label("");
}

fn set_recorder_recording(record_button: &gtk::Button, status: &gtk::Label) {
    record_button.add_css_class("is-recording");
    icons::set_button_icon_label(record_button, "Press keys...", icon_name::KEYBOARD);
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

#[derive(Clone)]
struct ShortcutRecorderRow {
    row: gtk::Widget,
    status: gtk::Label,
    capture_label: gtk::Label,
    record_button: gtk::Button,
    recording: Rc<Cell<bool>>,
}

impl ShortcutRecorderRow {
    fn cancel_recording(&self) {
        self.recording.set(false);
        set_recorder_idle(&self.record_button, &self.status);
    }

    fn sync_label(&self, shortcut: &str) {
        sync_shortcut_capture_label(&self.capture_label, shortcut);
    }
}

fn build_shortcut_recorder_row(
    label: &str,
    note: &str,
    examples: &[&str],
    current_shortcut: Rc<RefCell<String>>,
    callback: Rc<dyn Fn(String)>,
    sync_reset_button: Rc<dyn Fn()>,
) -> ShortcutRecorderRow {
    let status = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .css_classes([
            "field-hint",
            "settings-shortcut-note",
            "settings-shortcut-status",
        ])
        .visible(false)
        .build();
    let capture_label = gtk::Label::builder()
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["status-chip", "settings-shortcut-chip"])
        .build();
    sync_shortcut_capture_label(&capture_label, current_shortcut.borrow().as_str());
    let record_button = icons::labeled_button(
        "Record",
        icon_name::RECORD,
        &[
            "pill-button",
            "secondary-button",
            "settings-shortcut-record-button",
        ],
    );
    let control = build_shortcut_capture_control(&capture_label, &record_button);
    let recording = Rc::new(Cell::new(false));

    {
        let current_shortcut = current_shortcut.clone();
        let sync_reset_button = sync_reset_button.clone();
        let status = status.clone();
        let capture_label = capture_label.clone();
        let record_button_for_handler = record_button.clone();
        let recording = recording.clone();
        let callback = callback.clone();
        let key_controller = gtk::EventControllerKey::new();
        key_controller.connect_key_pressed(move |controller, key, keycode, state| {
            if !recording.get() {
                return glib::Propagation::Proceed;
            }

            let modifiers = state & gtk::accelerator_get_default_mod_mask();
            if key == gdk::Key::Escape && modifiers.is_empty() {
                recording.set(false);
                set_recorder_idle(&record_button_for_handler, &status);
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
            set_recorder_idle(&record_button_for_handler, &status);
            capture_label.set_label(&label);
            capture_label.set_tooltip_text(Some(&shortcut));
            if current_shortcut.borrow().as_str() != shortcut {
                current_shortcut.replace(shortcut.clone());
                callback(shortcut);
                sync_reset_button();
            }
            glib::Propagation::Stop
        });
        record_button.add_controller(key_controller);
    }
    {
        let status = status.clone();
        let record_button_for_handler = record_button.clone();
        let recording = recording.clone();
        record_button.connect_clicked(move |button| {
            if recording.get() {
                recording.set(false);
                set_recorder_idle(&record_button_for_handler, &status);
            } else {
                recording.set(true);
                set_recorder_recording(&record_button_for_handler, &status);
                button.grab_focus();
            }
        });
    }
    {
        let status = status.clone();
        let record_button_for_handler = record_button.clone();
        let recording = recording.clone();
        record_button.connect_notify_local(Some("has-focus"), move |button, _| {
            if recording.get() && !button.has_focus() {
                recording.set(false);
                set_recorder_idle(&record_button_for_handler, &status);
            }
        });
    }

    ShortcutRecorderRow {
        row: build_shortcut_entry_row(label, note, &control, &status, examples),
        status,
        capture_label,
        record_button,
        recording,
    }
}

fn sync_reset_button_state(reset_button: &gtk::Button, current: &SettingsState) {
    reset_button.set_sensitive(current != &SettingsState::defaults());
}

fn default_settings_dialog_size(
    window: &adw::ApplicationWindow,
    saved_width: i32,
    saved_height: i32,
) -> (i32, i32) {
    let min_width = if window.has_css_class("windows-gtk-shell") {
        640
    } else {
        200
    };
    let min_height = if window.has_css_class("windows-gtk-shell") {
        620
    } else {
        240
    };
    let width = match window.width() {
        width if width > 0 => (width - 32).min(saved_width.max(min_width)).max(min_width),
        _ => saved_width.max(min_width),
    };
    let height = match window.height() {
        height if height > 0 => (height - 48)
            .min(saved_height.max(min_height))
            .max(min_height),
        _ => saved_height.max(min_height),
    };

    (width, height)
}

fn persist_dialog_size(dialog: &adw::Dialog, on_size_changed: &Rc<dyn Fn(i32, i32)>) {
    let width = dialog.content_width();
    let height = dialog.content_height();
    if width > 0 && height > 0 {
        on_size_changed(width, height);
    }
}

fn sync_dialog_chrome_classes(window: &adw::ApplicationWindow, dialog: &adw::Dialog) {
    dialog_chrome::sync_dialog_chrome_classes(window, dialog, "settings-dialog-window");
}

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
        command_palette_shortcut,
        settings_dialog_width,
        settings_dialog_height,
        max_reconnect_attempts,
        voice,
        microphone_devices,
        product_display_name,
        settings_title,
        settings_summary,
    } = input;
    let SettingsDialogActions {
        on_theme_changed,
        on_density_changed,
        on_close_to_background_changed,
        on_fullscreen_shortcut_changed,
        on_density_shortcut_changed,
        on_zoom_in_shortcut_changed,
        on_zoom_out_shortcut_changed,
        on_command_palette_shortcut_changed,
        on_max_reconnect_attempts_changed,
        on_voice_preferences_changed,
        on_voice_pack_install_requested,
        voice_pack_status_provider,
        on_voice_pack_delete_requested,
        on_voice_pack_health_check_requested,
        on_open_logs_folder,
        on_reset_defaults,
        on_reset_builtin_presets,
        on_size_changed,
    } = actions;
    let (default_width, default_height) =
        default_settings_dialog_size(window, settings_dialog_width, settings_dialog_height);
    let dialog = adw::Dialog::new();
    dialog.set_title(&settings_title);
    dialog.set_follows_content_size(false);
    dialog.set_content_width(default_width);
    dialog.set_content_height(default_height);
    dialog_smoke::register_settings_dialog(&dialog);
    sync_dialog_chrome_classes(window, &dialog);
    let close_button = icons::labeled_button(
        "Close",
        icon_name::CLOSE,
        &["pill-button", "ghost-link-button", "settings-close-button"],
    );

    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .vexpand(true)
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .css_classes(["settings-dialog-scroller"])
        .build();
    scroller.set_has_frame(false);
    root.append(&scroller);

    let footer = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::End)
        .margin_top(12)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();
    footer.append(&close_button);
    root.append(&footer);
    dialog.set_child(Some(&root));
    dialog.set_default_widget(Some(&close_button));

    let request_close: Rc<dyn Fn()> = {
        let dialog = dialog.clone();
        Rc::new(move || {
            let dialog = dialog.clone();
            glib::idle_add_local_once(move || {
                if !dialog.close() {
                    dialog.force_close();
                }
            });
        })
    };
    if dialog_smoke::is_enabled() {
        dialog_smoke::register_settings_close(request_close.clone());
    }

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
    let current_command_palette_shortcut = Rc::new(RefCell::new(command_palette_shortcut));
    let current_max_reconnect_attempts = Rc::new(Cell::new(max_reconnect_attempts));
    let current_voice = Rc::new(RefCell::new(voice));
    let reset_button = icons::labeled_button(
        "Reset Defaults",
        icon_name::RESET,
        &["pill-button", "secondary-button", "settings-reset-button"],
    );
    let sync_reset_button: Rc<dyn Fn()> = {
        let current_theme = current_theme.clone();
        let current_density = current_density.clone();
        let current_close_to_background = current_close_to_background.clone();
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let current_density_shortcut = current_density_shortcut.clone();
        let current_zoom_in_shortcut = current_zoom_in_shortcut.clone();
        let current_zoom_out_shortcut = current_zoom_out_shortcut.clone();
        let current_command_palette_shortcut = current_command_palette_shortcut.clone();
        let current_max_reconnect_attempts = current_max_reconnect_attempts.clone();
        let current_voice = current_voice.clone();
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
                    command_palette_shortcut: current_command_palette_shortcut.borrow().clone(),
                    max_reconnect_attempts: current_max_reconnect_attempts.get(),
                    voice: current_voice.borrow().clone(),
                },
            );
        })
    };
    sync_reset_button();

    content.append(&build_settings_summary(
        &reset_button,
        &product_display_name,
        &settings_summary,
    ));

    let theme_callback = on_theme_changed;
    let density_callback = on_density_changed;
    let close_to_background_callback = on_close_to_background_changed;
    let fullscreen_shortcut_callback = on_fullscreen_shortcut_changed;
    let density_shortcut_callback = on_density_shortcut_changed;
    let zoom_in_shortcut_callback = on_zoom_in_shortcut_changed;
    let zoom_out_shortcut_callback = on_zoom_out_shortcut_changed;
    let command_palette_shortcut_callback = on_command_palette_shortcut_changed;
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

    let connection_section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["config-panel", "settings-section"])
        .build();
    content.append(&connection_section);

    connection_section.append(&build_section_header(
        "Connection",
        "Auto-reconnect",
        "Maximum number of times a pane automatically reconnects after an unexpected exit before requiring a manual restart.",
    ));

    let reconnect_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(["settings-toggle-row"])
        .build();
    let reconnect_text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    reconnect_text.append(
        &gtk::Label::builder()
            .label("Maximum auto-reconnect attempts")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .wrap(true)
            .css_classes(["settings-shortcut-title"])
            .build(),
    );
    reconnect_text.append(
        &gtk::Label::builder()
            .label(
                "After this many automatic restarts, a pane stays closed until manually restarted.",
            )
            .halign(gtk::Align::Start)
            .hexpand(true)
            .wrap(true)
            .css_classes(["field-hint", "settings-shortcut-note"])
            .build(),
    );
    reconnect_row.append(&reconnect_text);

    let reconnect_spin = gtk::SpinButton::with_range(1.0, 20.0, 1.0);
    reconnect_spin.set_value(max_reconnect_attempts as f64);
    reconnect_spin.set_valign(gtk::Align::Center);
    reconnect_spin.add_css_class("settings-spin-button");
    {
        let current_max_reconnect_attempts = current_max_reconnect_attempts.clone();
        let sync_reset_button = sync_reset_button.clone();
        let callback = on_max_reconnect_attempts_changed.clone();
        reconnect_spin.connect_value_changed(move |spin| {
            let value = spin.value() as u32;
            if current_max_reconnect_attempts.get() != value {
                current_max_reconnect_attempts.set(value);
                callback(value);
                sync_reset_button();
            }
        });
    }
    reconnect_row.append(&reconnect_spin);
    connection_section.append(&reconnect_row);

    let diagnostics_section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["config-panel", "settings-section"])
        .build();
    content.append(&diagnostics_section);

    diagnostics_section.append(&build_section_header(
        "Diagnostics",
        "Local logs",
        "Open the folder containing TerminalTiler's rolling log and current-session crash breadcrumb log.",
    ));
    diagnostics_section.append(&build_settings_action_row_with_icon(
        "Application log files",
        "Contains terminaltiler.log and terminaltiler-session.log for troubleshooting startup, runtime, and crash details.",
        "Open Logs Folder",
        icon_name::FOLDER,
        {
            let callback = on_open_logs_folder.clone();
            move || callback()
        },
    ));

    let voice_section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel", "settings-section"])
        .build();
    content.append(&voice_section);

    voice_section.append(&build_section_header(
        "Voice Input",
        "Local pack",
        "Dictate into the focused terminal pane. TerminalTiler inserts finalized transcript chunks only; partial text stays in the voice status HUD. Global hotkeys are best-effort and may be unavailable on Wayland.",
    ));

    let voice_callback = on_voice_preferences_changed.clone();
    let voice_enabled_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(["settings-toggle-row"])
        .build();
    let voice_enabled_text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    voice_enabled_text.append(
        &gtk::Label::builder()
            .label("Enable voice-to-text")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .wrap(true)
            .css_classes(["settings-shortcut-title"])
            .build(),
    );
    voice_enabled_text.append(
        &gtk::Label::builder()
            .label("Runs locally through a settings-installed voice pack. No cloud transcription is used.")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .wrap(true)
            .css_classes(["field-hint", "settings-shortcut-note"])
            .build(),
    );
    voice_enabled_row.append(&voice_enabled_text);
    let voice_enabled_switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .active(current_voice.borrow().enabled)
        .build();
    voice_enabled_switch.add_css_class("settings-toggle-switch");
    let suppress_voice_enabled_signal = Rc::new(Cell::new(false));
    {
        let current_voice = current_voice.clone();
        let sync_reset_button = sync_reset_button.clone();
        let callback = voice_callback.clone();
        let suppress_signal = suppress_voice_enabled_signal.clone();
        voice_enabled_switch.connect_active_notify(move |switch| {
            if suppress_signal.get() {
                return;
            }
            let mut next = current_voice.borrow().clone();
            if next.enabled == switch.is_active() {
                return;
            }
            next.enabled = switch.is_active();
            current_voice.replace(next.clone());
            callback(next);
            sync_reset_button();
        });
    }
    voice_enabled_row.append(&voice_enabled_switch);
    voice_section.append(&voice_enabled_row);

    let microphone_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .css_classes(["settings-toggle-row", "settings-microphone-row"])
        .build();
    let microphone_header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    let microphone_text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(3)
        .hexpand(true)
        .build();
    let microphone_title = gtk::Label::builder()
        .label("Microphone")
        .halign(gtk::Align::Start)
        .hexpand(true)
        .wrap(true)
        .css_classes(["settings-shortcut-title"])
        .build();
    microphone_text.append(&microphone_title);
    let microphone_hint = gtk::Label::builder()
        .label("Choose the input device used for voice capture. If unavailable, TerminalTiler falls back to the system default.")
        .halign(gtk::Align::Start)
        .hexpand(true)
        .wrap(true)
        .css_classes(["field-hint", "settings-shortcut-note"])
        .build();
    microphone_text.append(&microphone_hint);
    microphone_header.append(&microphone_text);
    let microphone_count = microphone_devices.len();
    let microphone_count_label = if microphone_count == 0 {
        "System fallback".to_string()
    } else if microphone_count == 1 {
        "1 input".to_string()
    } else {
        format!("{microphone_count} inputs")
    };
    microphone_header.append(
        &gtk::Label::builder()
            .label(&microphone_count_label)
            .valign(gtk::Align::Start)
            .css_classes(["settings-meta-chip", "microphone-status-chip"])
            .build(),
    );
    microphone_row.append(&microphone_header);

    let microphone_selector = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .hexpand(true)
        .css_classes(["microphone-select-shell"])
        .build();
    let microphone_combo = gtk::ComboBoxText::new();
    microphone_combo.append(Some(""), "System default");
    for microphone in &microphone_devices {
        microphone_combo.append(Some(&microphone.id), &microphone.name);
    }
    microphone_combo.set_active_id(current_voice.borrow().microphone_id.as_deref().or(Some("")));
    microphone_combo.add_css_class("surface-select-control");
    microphone_combo.add_css_class("microphone-select-control");
    microphone_combo.set_hexpand(true);
    microphone_combo.set_valign(gtk::Align::Center);
    microphone_combo.set_size_request(0, -1);
    microphone_combo.set_tooltip_text(Some(
        "Select the microphone TerminalTiler uses for local voice capture",
    ));
    {
        let current_voice = current_voice.clone();
        let sync_reset_button = sync_reset_button.clone();
        let callback = voice_callback.clone();
        microphone_combo.connect_changed(move |combo| {
            let selected = combo
                .active_id()
                .map(|value| value.to_string())
                .unwrap_or_default();
            let microphone_id = if selected.trim().is_empty() {
                None
            } else {
                Some(selected)
            };
            let mut next = current_voice.borrow().clone();
            if next.microphone_id == microphone_id {
                return;
            }
            next.microphone_id = microphone_id;
            current_voice.replace(next.clone());
            callback(next);
            sync_reset_button();
        });
    }
    microphone_selector.append(&microphone_combo);
    microphone_row.append(&microphone_selector);
    voice_section.append(&microphone_row);

    let voice_activation_strip = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .css_classes(["control-strip", "settings-choice-strip"])
        .build();
    for (mode, label) in [
        (VoiceActivationMode::PushToTalk, "Push to Talk"),
        (VoiceActivationMode::Toggle, "Toggle"),
    ] {
        let button = gtk::Button::with_label(label);
        button.add_css_class("flat");
        if mode == current_voice.borrow().activation_mode {
            button.add_css_class("is-active");
        }
        let current_voice = current_voice.clone();
        let strip = voice_activation_strip.clone();
        let sync_reset_button = sync_reset_button.clone();
        let callback = voice_callback.clone();
        button.connect_clicked(move |_| {
            let mut next = current_voice.borrow().clone();
            if next.activation_mode == mode {
                return;
            }
            next.activation_mode = mode;
            current_voice.replace(next.clone());
            sync_voice_activation_strip_active(&strip, mode);
            callback(next);
            sync_reset_button();
        });
        voice_activation_strip.append(&button);
    }
    voice_section.append(&voice_activation_strip);

    let voice_hotkey = Rc::new(RefCell::new(current_voice.borrow().hotkey.clone()));
    let voice_hotkey_recorder = build_shortcut_recorder_row(
        "Voice hotkey",
        "Push-to-talk starts on key down and flushes on key up. Toggle mode starts and stops on repeated presses.",
        &["<Ctrl><Shift>space", "<Alt>space", "F9"],
        voice_hotkey.clone(),
        Rc::new({
            let current_voice = current_voice.clone();
            let callback = voice_callback.clone();
            move |shortcut| {
                let mut next = current_voice.borrow().clone();
                next.hotkey = shortcut;
                current_voice.replace(next.clone());
                callback(next);
            }
        }),
        sync_reset_button.clone(),
    );
    voice_section.append(&voice_hotkey_recorder.row);

    let voice_engine_strip = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .css_classes(["control-strip", "settings-choice-strip"])
        .build();
    for (mode, label) in [
        (VoiceEngineMode::Auto, "Auto"),
        (VoiceEngineMode::Cuda, "CUDA"),
        (VoiceEngineMode::Cpu, "CPU"),
    ] {
        let button = gtk::Button::with_label(label);
        button.add_css_class("flat");
        if mode == current_voice.borrow().engine_mode {
            button.add_css_class("is-active");
        }
        let current_voice = current_voice.clone();
        let strip = voice_engine_strip.clone();
        let sync_reset_button = sync_reset_button.clone();
        let callback = voice_callback.clone();
        button.connect_clicked(move |_| {
            let mut next = current_voice.borrow().clone();
            if next.engine_mode == mode {
                return;
            }
            next.engine_mode = mode;
            current_voice.replace(next.clone());
            sync_voice_engine_strip_active(&strip, mode);
            callback(next);
            sync_reset_button();
        });
        voice_engine_strip.append(&button);
    }
    voice_section.append(&voice_engine_strip);

    let voice_global_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(["settings-toggle-row"])
        .build();
    let voice_global_text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    voice_global_text.append(
        &gtk::Label::builder()
            .label("Prefer global hotkey")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .wrap(true)
            .css_classes(["settings-shortcut-title"])
            .build(),
    );
    voice_global_text.append(
        &gtk::Label::builder()
            .label("Windows uses the Win32 hotkey path when available. Linux keeps an app-scoped baseline; Wayland may reject globals.")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .wrap(true)
            .css_classes(["field-hint", "settings-shortcut-note"])
            .build(),
    );
    voice_global_row.append(&voice_global_text);
    let voice_global_hotkey_switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .active(current_voice.borrow().prefer_global_hotkey)
        .build();
    voice_global_hotkey_switch.add_css_class("settings-toggle-switch");
    let suppress_voice_global_signal = Rc::new(Cell::new(false));
    {
        let current_voice = current_voice.clone();
        let sync_reset_button = sync_reset_button.clone();
        let callback = voice_callback.clone();
        let suppress_signal = suppress_voice_global_signal.clone();
        voice_global_hotkey_switch.connect_active_notify(move |switch| {
            if suppress_signal.get() {
                return;
            }
            let mut next = current_voice.borrow().clone();
            if next.prefer_global_hotkey == switch.is_active() {
                return;
            }
            next.prefer_global_hotkey = switch.is_active();
            current_voice.replace(next.clone());
            callback(next);
            sync_reset_button();
        });
    }
    voice_global_row.append(&voice_global_hotkey_switch);
    voice_section.append(&voice_global_row);

    voice_section.append(&build_voice_pack_install_row(
        current_voice.borrow().pack_status.clone(),
        voice_pack_status_provider.clone(),
        {
            let callback = on_voice_pack_install_requested.clone();
            move || callback()
        },
    ));
    voice_section.append(&build_settings_action_row(
        "Voice pack diagnostics",
        "Run a local health check for the downloaded helper and model files.",
        "Health Check",
        {
            let callback = on_voice_pack_health_check_requested.clone();
            move || callback()
        },
    ));
    voice_section.append(&build_settings_action_row(
        "Remove voice pack",
        "Deletes downloaded voice runtime/model files from application data. Settings are kept.",
        "Delete Pack",
        {
            let callback = on_voice_pack_delete_requested.clone();
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
    let fullscreen_recorder = build_shortcut_recorder_row(
        "Toggle workspace fullscreen",
        "Available only while a workspace tab is active.",
        &["F11", "<Shift>F11", "<Ctrl>F11"],
        current_fullscreen_shortcut.clone(),
        fullscreen_shortcut_callback.clone(),
        sync_reset_button.clone(),
    );
    shortcuts_section.append(&fullscreen_recorder.row);

    let density_recorder = build_shortcut_recorder_row(
        "Cycle active workspace density",
        "Rotates only the current workspace without changing the saved app default.",
        &["<Ctrl><Shift>D", "<Shift>F8", "<Alt><Super>D"],
        current_density_shortcut.clone(),
        density_shortcut_callback.clone(),
        sync_reset_button.clone(),
    );
    shortcuts_section.append(&density_recorder.row);

    let zoom_in_recorder = build_shortcut_recorder_row(
        "Zoom in terminal text",
        "Applies only to the active workspace and is restored with saved workspace sessions.",
        &["<Ctrl>plus", "<Ctrl>equal", "<Ctrl>KP_Add"],
        current_zoom_in_shortcut.clone(),
        zoom_in_shortcut_callback.clone(),
        sync_reset_button.clone(),
    );
    shortcuts_section.append(&zoom_in_recorder.row);

    let zoom_out_recorder = build_shortcut_recorder_row(
        "Zoom out terminal text",
        "Applies only to the active workspace and is restored with saved workspace sessions.",
        &["<Ctrl>minus", "<Ctrl>KP_Subtract"],
        current_zoom_out_shortcut.clone(),
        zoom_out_shortcut_callback.clone(),
        sync_reset_button.clone(),
    );
    shortcuts_section.append(&zoom_out_recorder.row);

    let command_palette_recorder = build_shortcut_recorder_row(
        "Open command palette",
        "Available in launch tabs and workspaces for fast navigation and actions.",
        &["<Ctrl><Shift>P", "<Ctrl>P", "<Super>P"],
        current_command_palette_shortcut.clone(),
        command_palette_shortcut_callback.clone(),
        sync_reset_button.clone(),
    );
    shortcuts_section.append(&command_palette_recorder.row);

    {
        let current_theme = current_theme.clone();
        let current_density = current_density.clone();
        let current_close_to_background = current_close_to_background.clone();
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let current_density_shortcut = current_density_shortcut.clone();
        let current_zoom_in_shortcut = current_zoom_in_shortcut.clone();
        let current_zoom_out_shortcut = current_zoom_out_shortcut.clone();
        let current_command_palette_shortcut = current_command_palette_shortcut.clone();
        let current_max_reconnect_attempts = current_max_reconnect_attempts.clone();
        let current_voice = current_voice.clone();
        let theme_strip = theme_strip.clone();
        let density_strip = density_strip.clone();
        let close_to_background_switch = close_to_background_switch.clone();
        let suppress_close_to_background_signal = suppress_close_to_background_signal.clone();
        let fullscreen_recorder = fullscreen_recorder.clone();
        let density_recorder = density_recorder.clone();
        let zoom_in_recorder = zoom_in_recorder.clone();
        let zoom_out_recorder = zoom_out_recorder.clone();
        let command_palette_recorder = command_palette_recorder.clone();
        let reconnect_spin = reconnect_spin.clone();
        let voice_enabled_switch = voice_enabled_switch.clone();
        let microphone_combo = microphone_combo.clone();
        let voice_global_hotkey_switch = voice_global_hotkey_switch.clone();
        let suppress_voice_enabled_signal = suppress_voice_enabled_signal.clone();
        let suppress_voice_global_signal = suppress_voice_global_signal.clone();
        let voice_activation_strip = voice_activation_strip.clone();
        let voice_engine_strip = voice_engine_strip.clone();
        let voice_hotkey = voice_hotkey.clone();
        let voice_hotkey_recorder = voice_hotkey_recorder.clone();
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
                    != defaults.workspace_zoom_out_shortcut
                || current_command_palette_shortcut.borrow().as_str()
                    != defaults.command_palette_shortcut
                || current_max_reconnect_attempts.get() != defaults.max_reconnect_attempts
                || *current_voice.borrow() != defaults.voice;
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
            current_command_palette_shortcut.replace(defaults.command_palette_shortcut.clone());
            current_max_reconnect_attempts.set(defaults.max_reconnect_attempts);
            current_voice.replace(defaults.voice.clone());
            sync_theme_strip_active(&theme_strip, defaults.default_theme);
            sync_density_strip_active(&density_strip, defaults.default_density);
            suppress_close_to_background_signal.set(true);
            close_to_background_switch.set_active(defaults.close_to_background);
            suppress_close_to_background_signal.set(false);
            fullscreen_recorder.sync_label(&defaults.workspace_fullscreen_shortcut);
            density_recorder.sync_label(&defaults.workspace_density_shortcut);
            zoom_in_recorder.sync_label(&defaults.workspace_zoom_in_shortcut);
            zoom_out_recorder.sync_label(&defaults.workspace_zoom_out_shortcut);
            command_palette_recorder.sync_label(&defaults.command_palette_shortcut);
            reconnect_spin.set_value(defaults.max_reconnect_attempts as f64);
            suppress_voice_enabled_signal.set(true);
            voice_enabled_switch.set_active(defaults.voice.enabled);
            suppress_voice_enabled_signal.set(false);
            suppress_voice_global_signal.set(true);
            voice_global_hotkey_switch.set_active(defaults.voice.prefer_global_hotkey);
            suppress_voice_global_signal.set(false);
            microphone_combo.set_active_id(defaults.voice.microphone_id.as_deref().or(Some("")));
            sync_voice_activation_strip_active(
                &voice_activation_strip,
                defaults.voice.activation_mode,
            );
            sync_voice_engine_strip_active(&voice_engine_strip, defaults.voice.engine_mode);
            voice_hotkey.replace(defaults.voice.hotkey.clone());
            voice_hotkey_recorder.sync_label(&defaults.voice.hotkey);
            fullscreen_recorder.cancel_recording();
            density_recorder.cancel_recording();
            zoom_in_recorder.cancel_recording();
            zoom_out_recorder.cancel_recording();
            command_palette_recorder.cancel_recording();
            voice_hotkey_recorder.cancel_recording();
            sync_reset_button();
            reset_callback();
        });
    }
    {
        let size_changed_callback = size_changed_callback.clone();
        dialog.connect_closed(move |dialog| {
            persist_dialog_size(dialog, &size_changed_callback);
        });
    }

    {
        let request_close = request_close.clone();
        close_button.connect_clicked(move |_| {
            request_close();
        });
    }

    dialog.present(Some(window));
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

fn sync_voice_pack_install_row(
    status: &VoicePackStatus,
    status_label: &gtk::Label,
    action_stack: &gtk::Stack,
    progress_bar: &gtk::ProgressBar,
) {
    status_label.set_label(&status.summary());
    match status {
        VoicePackStatus::Downloading { percent } => {
            let bounded_percent = (*percent).clamp(1, 99);
            progress_bar.set_fraction(f64::from(bounded_percent) / 100.0);
            progress_bar.set_text(Some(&format!("{bounded_percent}%")));
            progress_bar.pulse();
            action_stack.set_visible_child_name("progress");
        }
        _ => {
            progress_bar.set_fraction(0.0);
            progress_bar.set_text(None);
            action_stack.set_visible_child_name("button");
        }
    }
}

fn build_voice_pack_install_row<F>(
    initial_status: VoicePackStatus,
    status_provider: Rc<dyn Fn() -> VoicePackStatus>,
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
            .label("Voice pack")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .wrap(true)
            .css_classes(["settings-shortcut-title"])
            .build(),
    );
    let status_label = gtk::Label::builder()
        .label(initial_status.summary())
        .halign(gtk::Align::Start)
        .hexpand(true)
        .wrap(true)
        .css_classes(["field-hint", "settings-shortcut-note"])
        .build();
    text.append(&status_label);
    shell.append(&text);

    let button = icons::labeled_button(
        "Install / Reinstall",
        icon_name::EDIT,
        &["pill-button", "secondary-button"],
    );
    button.connect_clicked(move |_| on_click());

    let progress_bar = gtk::ProgressBar::builder()
        .width_request(150)
        .valign(gtk::Align::Center)
        .show_text(true)
        .css_classes(["voice-pack-progress"])
        .build();
    progress_bar.set_pulse_step(0.05);

    let action_stack = gtk::Stack::builder()
        .valign(gtk::Align::Center)
        .hexpand(false)
        .build();
    action_stack.add_named(&button, Some("button"));
    action_stack.add_named(&progress_bar, Some("progress"));
    shell.append(&action_stack);

    sync_voice_pack_install_row(&initial_status, &status_label, &action_stack, &progress_bar);

    let status_label_weak = status_label.downgrade();
    let action_stack_weak = action_stack.downgrade();
    let progress_bar_weak = progress_bar.downgrade();
    glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        let Some(status_label) = status_label_weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let Some(action_stack) = action_stack_weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let Some(progress_bar) = progress_bar_weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let status = status_provider();
        sync_voice_pack_install_row(&status, &status_label, &action_stack, &progress_bar);
        glib::ControlFlow::Continue
    });

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
    build_settings_action_row_with_icon(title, note, button_label, icon_name::RESET, on_click)
}

fn build_settings_action_row_with_icon<F>(
    title: &str,
    note: &str,
    button_label: &str,
    button_icon: &str,
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

    let button = icons::labeled_button(
        button_label,
        button_icon,
        &["pill-button", "secondary-button"],
    );
    button.connect_clicked(move |_| on_click());
    shell.append(&button);

    shell.upcast()
}

fn build_settings_summary(
    reset_button: &gtk::Button,
    product_display_name: &str,
    settings_summary: &str,
) -> gtk::Widget {
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
    let settings_icon = gtk::Image::from_icon_name("preferences-system-symbolic");
    settings_icon.set_valign(gtk::Align::Center);
    icon.append(&settings_icon);
    shell.append(&icon);

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .css_classes(["settings-summary-body"])
        .build();
    body.append(
        &gtk::Label::builder()
            .label(product_display_name)
            .halign(gtk::Align::Start)
            .css_classes(["section-title", "settings-title", "settings-summary-title"])
            .build(),
    );
    body.append(
        &gtk::Label::builder()
            .label(settings_summary)
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
    actions.append(&build_meta_chip("MIT core"));
    actions.append(&build_meta_chip("Public source"));
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

fn sync_voice_activation_strip_active(strip: &gtk::Box, active_mode: VoiceActivationMode) {
    let mut child = strip.first_child();
    while let Some(widget) = child {
        let next = widget.next_sibling();
        widget.remove_css_class("is-active");
        if let Ok(button) = widget.clone().downcast::<gtk::Button>()
            && button.label().as_deref() == Some(active_mode.label())
        {
            button.add_css_class("is-active");
        }
        child = next;
    }
}

fn sync_voice_engine_strip_active(strip: &gtk::Box, active_mode: VoiceEngineMode) {
    let mut child = strip.first_child();
    while let Some(widget) = child {
        let next = widget.next_sibling();
        widget.remove_css_class("is-active");
        if let Ok(button) = widget.clone().downcast::<gtk::Button>()
            && button.label().as_deref() == Some(active_mode.label())
        {
            button.add_css_class("is-active");
        }
        child = next;
    }
}
