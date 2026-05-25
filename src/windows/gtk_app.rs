#[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
mod imp {
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;
    use std::process::ExitCode;
    use std::rc::Rc;
    use std::sync::mpsc;
    use std::time::Duration;

    use adw::prelude::*;
    use gtk::{gio, glib};

    use crate::extension::RuntimeOptions;
    use crate::logging;
    use crate::product;
    use crate::services::session_restore::session_for_restore_mode;
    use crate::storage::asset_store::AssetStore;
    use crate::storage::preference_store::{AppPreferences, PreferenceStore};
    use crate::storage::preset_store::PresetStore;
    use crate::storage::session_store::{SavedSession, SavedTab, SessionStore};
    use crate::ui::app_chrome::{
        build_app_header_chrome, build_main_titlebar_actions, build_window_shell,
        sync_workspace_fullscreen_chrome,
    };
    use crate::ui::appearance::{apply_theme_mode, apply_window_density};
    use crate::ui::launch_screen::{LaunchScreenActions, LaunchScreenInput};
    use crate::ui::title_chrome::{TitleChrome, apply_title_tab_state, build_title_tab_chrome};
    use crate::ui::{
        about_dialog, assets_manager, command_palette, companion_dialog, settings_dialog,
    };
    use crate::voice::VoicePackStatus;
    use crate::voice::audio::AudioCapture;
    use crate::voice::engine::{self, VoiceEngineEvent};
    use crate::voice::pack::{self, VoicePackHealth};

    const GTK_APP_ID: &str = "dev.zethrus.terminaltiler.windows.gtk";

    pub fn run() -> ExitCode {
        run_with_options(RuntimeOptions::default())
    }

    pub fn run_with_options(options: RuntimeOptions) -> ExitCode {
        logging::init();
        logging::info("windows GTK shell startup");

        let app_id = options.product.app_id.as_deref().unwrap_or(GTK_APP_ID);
        let app = adw::Application::builder().application_id(app_id).build();

        app.connect_startup(|_| {
            crate::gtk_shell::load_css_for_default_display();
            logging::info("windows GTK shell loaded canonical GTK CSS");
        });

        app.connect_activate(move |app| {
            present_launch_window(app, &options);
        });

        glib_exit_to_process_exit(app.run())
    }

    fn glib_exit_to_process_exit(code: gtk::glib::ExitCode) -> ExitCode {
        let value = code.get();
        if value == 0 {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(value)
        }
    }

    fn present_launch_window(app: &adw::Application, options: &RuntimeOptions) {
        if let Some(window) = primary_window(app) {
            window.present();
            return;
        }

        let preference_store = PreferenceStore::new();
        let preferences = preference_store.load();
        let preset_store = PresetStore::new();
        preset_store.ensure_seeded();
        let preset_outcome = preset_store.load_presets_with_status();
        let asset_store = AssetStore::new();
        asset_store.ensure_seeded();
        let asset_outcome = asset_store.load_assets_with_status();
        let workspace_assets = asset_outcome.assets.clone();
        let session_store = SessionStore::new();
        let session_outcome = session_store.load_with_status();
        let load_warning = combine_warnings(
            combine_warnings(preset_outcome.warning, asset_outcome.warning),
            session_outcome.warning,
        );

        let app_header = build_app_header_chrome();
        let header = app_header.header;
        let title = app_header.title;

        let overlay = adw::ToastOverlay::new();
        let titlebar_actions = build_main_titlebar_actions(&header, options.companion.is_some());
        let back_button = titlebar_actions.back_button;
        let fullscreen_button = titlebar_actions.fullscreen_button;
        let settings_button = titlebar_actions.settings_button;
        let companion_button = titlebar_actions.companion_button;
        let assets_button = titlebar_actions.assets_button;

        let window_shell = build_window_shell();
        window_shell.append(&header);
        window_shell.append(&overlay);

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(&options.product.app_title)
            .default_width(crate::gtk_shell::DEFAULT_WINDOW_WIDTH)
            .default_height(crate::gtk_shell::DEFAULT_WINDOW_HEIGHT)
            .content(&window_shell)
            .build();
        window.add_css_class("window-shell");
        window.add_css_class("windows-gtk-shell");
        apply_theme_mode(&window, preferences.default_theme);
        apply_window_density(&window, preferences.default_density);

        let (voice_toast_tx, voice_toast_rx) = mpsc::channel::<String>();
        {
            let overlay = overlay.clone();
            gtk::glib::timeout_add_local(Duration::from_millis(120), move || {
                while let Ok(message) = voice_toast_rx.try_recv() {
                    overlay.add_toast(adw::Toast::new(&message));
                }
                gtk::glib::ControlFlow::Continue
            });
        }

        {
            let window = window.clone();
            let overlay = overlay.clone();
            let preference_store = preference_store.clone();
            let preset_store = preset_store.clone();
            let options = options.clone();
            let voice_toast_tx = voice_toast_tx.clone();
            settings_button.connect_clicked(move |_| {
                present_settings_dialog(
                    &window,
                    &overlay,
                    preference_store.clone(),
                    preset_store.clone(),
                    options.clone(),
                    voice_toast_tx.clone(),
                );
            });
        }

        {
            let window = window.clone();
            let overlay = overlay.clone();
            let asset_store = asset_store.clone();
            assets_button.connect_clicked(move |_| {
                present_assets_manager(&window, &overlay, asset_store.clone());
            });
        }

        if let (Some(button), Some(companion)) =
            (companion_button.as_ref(), options.companion.as_ref())
        {
            let window = window.clone();
            let companion = companion.clone();
            button.connect_clicked(move |_| {
                companion_dialog::present(&window, companion.clone());
            });
        }

        {
            let window = window.clone();
            fullscreen_button.connect_clicked(move |_| {
                window.set_fullscreened(!window.is_fullscreen());
            });
        }

        {
            let window_for_notify = window.clone();
            let title_root_for_notify = title.root.clone();
            let fullscreen_for_notify = fullscreen_button.clone();
            let back_for_notify = back_button.clone();
            window.connect_fullscreened_notify(move |_| {
                sync_windows_fullscreen_chrome(
                    &window_for_notify,
                    title_root_for_notify.upcast_ref(),
                    &fullscreen_for_notify,
                    back_for_notify.is_visible(),
                );
            });
        }

        sync_windows_fullscreen_chrome(&window, title.root.upcast_ref(), &fullscreen_button, false);

        let shell_state = WindowsGtkShellState::default();
        shell_state.launch_deck_active.set(true);
        let command_palette_shortcut_controller: ShortcutControllerHandle =
            Rc::new(RefCell::new(None));
        let launch_widget_handle: Rc<RefCell<Option<gtk::Widget>>> = Rc::new(RefCell::new(None));

        let launch_preferences = preferences.clone();
        let launch_overlay = overlay.clone();
        let launch_title = title.clone();
        let launch_assets = workspace_assets.clone();
        let launch_back_button = back_button.clone();
        let launch_fullscreen_button = fullscreen_button.clone();
        let launch_window = window.clone();
        let launch_shell_state = shell_state.clone();
        let launch_widget_for_action = launch_widget_handle.clone();
        let cancel_shell_state = shell_state.clone();
        let cancel_launch_widget = launch_widget_handle.clone();
        let cancel_window = window.clone();
        let cancel_overlay = overlay.clone();
        let cancel_title = title.clone();
        let cancel_back_button = back_button.clone();
        let cancel_fullscreen_button = fullscreen_button.clone();
        let actions = LaunchScreenActions {
            on_theme_preview: Rc::new({
                let window = window.clone();
                move |theme| apply_theme_mode(&window, theme)
            }),
            on_density_preview: Rc::new({
                let window = window.clone();
                move |density| apply_window_density(&window, density)
            }),
            on_launch: Rc::new(move |preset, workspace_root| {
                if let Some(launch_widget) = launch_widget_for_action.borrow().as_ref() {
                    present_workspace_preview_from_launch(
                        &launch_window,
                        &launch_overlay,
                        &launch_title,
                        &launch_preferences,
                        &launch_back_button,
                        &launch_fullscreen_button,
                        &launch_shell_state,
                        launch_widget,
                        launch_assets.clone(),
                        preset,
                        workspace_root,
                    );
                }
            }),
            on_cancel: Rc::new({
                let app = app.clone();
                move || {
                    if cancel_shell_state.has_workspace_tabs()
                        && let Some(launch_widget) = cancel_launch_widget.borrow().as_ref()
                    {
                        let active_index = cancel_shell_state
                            .preview
                            .borrow()
                            .as_ref()
                            .map(|preview| preview.active_index())
                            .unwrap_or(0);
                        show_workspace_preview_tab(
                            &cancel_window,
                            &cancel_overlay,
                            &cancel_title,
                            launch_widget,
                            &cancel_back_button,
                            &cancel_fullscreen_button,
                            &cancel_shell_state,
                            active_index,
                        );
                    } else {
                        app.quit();
                    }
                }
            }),
            on_presets_changed: Rc::new(|| {
                logging::info("Windows GTK shell preset list changed; relaunch to refresh deck");
            }),
        };

        let launch = crate::ui::launch_screen::build(
            LaunchScreenInput {
                load_warning,
                presets: preset_outcome.presets,
                assets: asset_outcome.assets,
                default_theme: preferences.default_theme,
                default_density: preferences.default_density,
                default_restore_mode: preferences.default_restore_mode,
                preset_store,
            },
            actions,
        );
        *launch_widget_handle.borrow_mut() = Some(launch.clone());
        {
            let overlay = overlay.clone();
            let title = title.clone();
            let launch = launch.clone();
            let back_button_for_click = back_button.clone();
            let fullscreen_for_click = fullscreen_button.clone();
            let window_for_click = window.clone();
            let title_add_button = title.add_button.clone();
            let shell_state_for_launch = shell_state.clone();
            let show_launch_deck = Rc::new(move || {
                show_launch_deck_tab(
                    &window_for_click,
                    &overlay,
                    &title,
                    &launch,
                    &back_button_for_click,
                    &fullscreen_for_click,
                    &shell_state_for_launch,
                );
            });
            {
                let show_launch_deck = show_launch_deck.clone();
                back_button.connect_clicked(move |_| {
                    show_launch_deck();
                });
            }
            title_add_button.connect_clicked(move |_| {
                show_launch_deck();
            });
            let open_command_palette: Rc<dyn Fn()> = Rc::new({
                let window = window.clone();
                let overlay = overlay.clone();
                let title = title.clone();
                let launch = launch.clone();
                let back_button = back_button.clone();
                let fullscreen_button = fullscreen_button.clone();
                let shell_state = shell_state.clone();
                let preference_store = preference_store.clone();
                let preset_store = preset_store.clone();
                let asset_store = asset_store.clone();
                let options = options.clone();
                let voice_toast_tx = voice_toast_tx.clone();
                move || {
                    present_command_palette(
                        &window,
                        &overlay,
                        &title,
                        &launch,
                        &back_button,
                        &fullscreen_button,
                        &shell_state,
                        preference_store.clone(),
                        preset_store.clone(),
                        asset_store.clone(),
                        options.clone(),
                        voice_toast_tx.clone(),
                    );
                }
            });
            install_command_palette_shortcut(
                &window,
                &command_palette_shortcut_controller,
                &preferences.command_palette_shortcut,
                open_command_palette.clone(),
            );
        }
        overlay.set_child(Some(&launch));
        sync_windows_shell_title_tabs(
            &window,
            &overlay,
            &title,
            &launch,
            &back_button,
            &fullscreen_button,
            &shell_state,
        );
        window.present();

        if let Some(session) = session_outcome
            .session
            .as_ref()
            .and_then(|session| session_for_restore_mode(session, preferences.default_restore_mode))
        {
            let overlay = overlay.clone();
            let title = title.clone();
            let window = window.clone();
            let back_button = back_button.clone();
            let fullscreen_button = fullscreen_button.clone();
            let preferences = preferences.clone();
            let workspace_assets = workspace_assets.clone();
            let shell_state = shell_state.clone();
            let launch = launch.clone();
            gtk::glib::idle_add_local_once(move || {
                present_workspace_preview_from_restore(
                    &window,
                    &overlay,
                    &title,
                    &preferences,
                    &back_button,
                    &fullscreen_button,
                    &shell_state,
                    &launch,
                    workspace_assets,
                    session,
                );
            });
        }
    }

    fn present_workspace_preview_from_restore(
        window: &adw::ApplicationWindow,
        overlay: &adw::ToastOverlay,
        title: &TitleChrome,
        _preferences: &AppPreferences,
        back_button: &gtk::Button,
        fullscreen_button: &gtk::Button,
        shell_state: &WindowsGtkShellState,
        launch: &gtk::Widget,
        assets: crate::model::assets::WorkspaceAssets,
        session: SavedSession,
    ) {
        present_workspace_preview(
            window,
            overlay,
            title,
            back_button,
            fullscreen_button,
            shell_state,
            launch,
            session,
            assets,
            "restored",
        );
    }

    fn present_settings_dialog(
        window: &adw::ApplicationWindow,
        overlay: &adw::ToastOverlay,
        preference_store: PreferenceStore,
        preset_store: PresetStore,
        options: RuntimeOptions,
        voice_toast_tx: mpsc::Sender<String>,
    ) {
        let preferences = preference_store.load();
        settings_dialog::present(
            window,
            settings_dialog::SettingsDialogInput {
                default_theme: preferences.default_theme,
                default_density: preferences.default_density,
                close_to_background: preferences.close_to_background,
                workspace_fullscreen_shortcut: preferences.workspace_fullscreen_shortcut,
                workspace_density_shortcut: preferences.workspace_density_shortcut,
                workspace_zoom_in_shortcut: preferences.workspace_zoom_in_shortcut,
                workspace_zoom_out_shortcut: preferences.workspace_zoom_out_shortcut,
                command_palette_shortcut: preferences.command_palette_shortcut,
                settings_dialog_width: preferences.settings_dialog_width,
                settings_dialog_height: preferences.settings_dialog_height,
                max_reconnect_attempts: preferences.max_reconnect_attempts,
                voice: preferences.voice,
                microphone_devices: AudioCapture::enumerate_microphones().unwrap_or_default(),
                product_display_name: options.product.display_name.clone(),
                settings_title: options.product.settings_title.clone(),
                settings_summary: options.product.settings_summary.clone(),
            },
            settings_dialog::SettingsDialogActions {
                on_theme_changed: Rc::new({
                    let window = window.clone();
                    let overlay = overlay.clone();
                    let preference_store = preference_store.clone();
                    move |theme| {
                        preference_store.save_default_theme(theme);
                        apply_theme_mode(&window, theme);
                        overlay.add_toast(adw::Toast::new(&format!(
                            "Default theme set to {}",
                            theme.label()
                        )));
                    }
                }),
                on_density_changed: Rc::new({
                    let window = window.clone();
                    let overlay = overlay.clone();
                    let preference_store = preference_store.clone();
                    move |density| {
                        preference_store.save_default_density(density);
                        apply_window_density(&window, density);
                        overlay.add_toast(adw::Toast::new(&format!(
                            "Default density set to {}",
                            density.label()
                        )));
                    }
                }),
                on_close_to_background_changed: Rc::new({
                    let overlay = overlay.clone();
                    let preference_store = preference_store.clone();
                    move |close_to_background| {
                        preference_store.save_close_to_background(close_to_background);
                        let message = if close_to_background {
                            "Close-to-background preference enabled"
                        } else {
                            "Close-to-background preference disabled"
                        };
                        overlay.add_toast(adw::Toast::new(message));
                    }
                }),
                on_fullscreen_shortcut_changed: Rc::new({
                    let preference_store = preference_store.clone();
                    move |shortcut| preference_store.save_workspace_fullscreen_shortcut(&shortcut)
                }),
                on_density_shortcut_changed: Rc::new({
                    let preference_store = preference_store.clone();
                    move |shortcut| preference_store.save_workspace_density_shortcut(&shortcut)
                }),
                on_zoom_in_shortcut_changed: Rc::new({
                    let preference_store = preference_store.clone();
                    move |shortcut| preference_store.save_workspace_zoom_in_shortcut(&shortcut)
                }),
                on_zoom_out_shortcut_changed: Rc::new({
                    let preference_store = preference_store.clone();
                    move |shortcut| preference_store.save_workspace_zoom_out_shortcut(&shortcut)
                }),
                on_command_palette_shortcut_changed: Rc::new({
                    let preference_store = preference_store.clone();
                    move |shortcut| preference_store.save_command_palette_shortcut(&shortcut)
                }),
                on_max_reconnect_attempts_changed: Rc::new({
                    let preference_store = preference_store.clone();
                    move |attempts| preference_store.save_max_reconnect_attempts(attempts)
                }),
                on_voice_preferences_changed: Rc::new({
                    let preference_store = preference_store.clone();
                    move |voice| preference_store.save_voice_preferences(voice)
                }),
                on_voice_pack_install_requested: Rc::new({
                    let overlay = overlay.clone();
                    let preference_store = preference_store.clone();
                    let voice_toast_tx = voice_toast_tx.clone();
                    move || {
                        overlay
                            .add_toast(adw::Toast::new("Installing NVIDIA Parakeet voice pack…"));
                        install_windows_voice_pack(
                            preference_store.clone(),
                            voice_toast_tx.clone(),
                        );
                    }
                }),
                voice_pack_status_provider: Rc::new({
                    let preference_store = preference_store.clone();
                    move || -> VoicePackStatus { preference_store.load().voice.pack_status.clone() }
                }),
                on_voice_pack_delete_requested: Rc::new({
                    let overlay = overlay.clone();
                    let preference_store = preference_store.clone();
                    let voice_toast_tx = voice_toast_tx.clone();
                    move || {
                        overlay.add_toast(adw::Toast::new("Deleting NVIDIA Parakeet voice pack…"));
                        delete_windows_voice_pack(preference_store.clone(), voice_toast_tx.clone());
                    }
                }),
                on_voice_pack_health_check_requested: Rc::new({
                    let overlay = overlay.clone();
                    let preference_store = preference_store.clone();
                    let voice_toast_tx = voice_toast_tx.clone();
                    move || {
                        overlay.add_toast(adw::Toast::new("Checking NVIDIA Parakeet runtime…"));
                        check_windows_voice_pack_health(
                            preference_store.clone(),
                            voice_toast_tx.clone(),
                        );
                    }
                }),
                on_open_logs_folder: Rc::new({
                    let overlay = overlay.clone();
                    move || open_logs_folder(&overlay)
                }),
                on_reset_defaults: Rc::new({
                    let window = window.clone();
                    let overlay = overlay.clone();
                    let preference_store = preference_store.clone();
                    move || {
                        let defaults = AppPreferences::default();
                        preference_store.save(&defaults);
                        apply_theme_mode(&window, defaults.default_theme);
                        apply_window_density(&window, defaults.default_density);
                        overlay.add_toast(adw::Toast::new("Application defaults reset"));
                    }
                }),
                on_reset_builtin_presets: Rc::new({
                    let overlay = overlay.clone();
                    move || match preset_store.reset_builtin_presets() {
                        Ok(()) => {
                            logging::info("reset builtin saved presets to factory defaults");
                            overlay.add_toast(adw::Toast::new("Default saved presets restored"));
                        }
                        Err(error) => {
                            logging::error(format!(
                                "failed to reset builtin saved presets: {error}"
                            ));
                            overlay.add_toast(adw::Toast::new(
                                "Failed to restore default saved presets",
                            ));
                        }
                    }
                }),
                on_size_changed: Rc::new({
                    let preference_store = preference_store.clone();
                    move |width, height| preference_store.save_settings_dialog_size(width, height)
                }),
            },
        );
    }

    fn install_windows_voice_pack(
        preference_store: PreferenceStore,
        voice_toast_tx: mpsc::Sender<String>,
    ) {
        let Some(root) = pack::default_voice_pack_dir() else {
            let _ = voice_toast_tx.send("Could not resolve application data directory".into());
            return;
        };

        let mut preferences = preference_store.load();
        preferences.voice.pack_status = VoicePackStatus::Downloading { percent: 1 };
        preference_store.save(&preferences);

        std::thread::spawn(move || match pack::install_builtin_parakeet_pack(&root) {
            Ok(manifest) => {
                save_voice_pack_download_progress(&preference_store, 40);
                match pack::prepare_python_environment_with_progress(&root, &manifest, |percent| {
                    save_voice_pack_download_progress(&preference_store, percent)
                }) {
                    Ok(_) => {
                        save_voice_pack_download_progress(&preference_store, 80);
                        match verify_voice_pack_runtime(&preference_store, &root, &manifest) {
                            Ok(detail) => {
                                let mut preferences = preference_store.load();
                                preferences.voice.pack_status = VoicePackStatus::Installed {
                                    version: manifest.version.clone(),
                                };
                                preference_store.save(&preferences);
                                logging::info(format!(
                                    "installed bundled NVIDIA Parakeet voice pack id={} version={} root={} health={}",
                                    manifest.id,
                                    manifest.version,
                                    root.display(),
                                    detail
                                ));
                                let _ = voice_toast_tx
                                    .send("NVIDIA Parakeet voice pack installed".into());
                            }
                            Err(message) => {
                                save_voice_pack_error(&preference_store, &message);
                                let _ = voice_toast_tx
                                    .send("Voice pack installed, but verification failed".into());
                            }
                        }
                    }
                    Err(error) => {
                        let message = error.to_string();
                        save_voice_pack_error(&preference_store, &message);
                        logging::error(format!(
                            "failed to prepare NVIDIA Parakeet Python environment: {error:?}"
                        ));
                        let _ = voice_toast_tx
                            .send("Voice pack installed, but Python dependencies failed".into());
                    }
                }
            }
            Err(error) => {
                let message = error.to_string();
                save_voice_pack_error(&preference_store, &message);
                logging::error(format!(
                    "failed to install bundled NVIDIA Parakeet voice pack: {error:?}"
                ));
                let _ = voice_toast_tx.send("Failed to install NVIDIA Parakeet voice pack".into());
            }
        });
    }

    fn delete_windows_voice_pack(
        preference_store: PreferenceStore,
        voice_toast_tx: mpsc::Sender<String>,
    ) {
        let Some(root) = pack::default_voice_pack_dir() else {
            let _ = voice_toast_tx.send("Could not resolve application data directory".into());
            return;
        };
        let manifest = pack::builtin_parakeet_manifest();

        std::thread::spawn(move || match pack::delete_pack(&root, &manifest) {
            Ok(_) => {
                let mut preferences = preference_store.load();
                preferences.voice.pack_status = VoicePackStatus::NotInstalled;
                preference_store.save(&preferences);
                logging::info(format!(
                    "deleted NVIDIA Parakeet voice pack id={} version={} root={}",
                    manifest.id,
                    manifest.version,
                    root.display()
                ));
                let _ = voice_toast_tx.send("NVIDIA Parakeet voice pack deleted".into());
            }
            Err(error) => {
                logging::error(format!(
                    "failed to delete NVIDIA Parakeet voice pack: {error:?}"
                ));
                let _ = voice_toast_tx.send("Failed to delete NVIDIA Parakeet voice pack".into());
            }
        });
    }

    fn check_windows_voice_pack_health(
        preference_store: PreferenceStore,
        voice_toast_tx: mpsc::Sender<String>,
    ) {
        let Some(root) = pack::default_voice_pack_dir() else {
            let _ = voice_toast_tx.send("Could not resolve application data directory".into());
            return;
        };
        let manifest = pack::builtin_parakeet_manifest();

        std::thread::spawn(move || {
            let toast = match refresh_builtin_voice_pack_assets_for_runtime(&root) {
                Ok(()) => match verify_voice_pack_runtime(&preference_store, &root, &manifest) {
                    Ok(detail) => {
                        logging::info(format!(
                            "NVIDIA Parakeet runtime health check passed id={} version={} root={} detail={}",
                            manifest.id,
                            manifest.version,
                            root.display(),
                            detail
                        ));
                        "NVIDIA Parakeet runtime is healthy".to_string()
                    }
                    Err(message) => {
                        logging::error(format!(
                            "NVIDIA Parakeet runtime health check failed: {message}"
                        ));
                        message
                    }
                },
                Err(detail) => {
                    logging::error(format!(
                        "NVIDIA Parakeet voice pack refresh failed before health check: {detail}"
                    ));
                    "NVIDIA Parakeet voice pack refresh failed".to_string()
                }
            };
            let _ = voice_toast_tx.send(toast);
        });
    }

    fn verify_voice_pack_runtime(
        preference_store: &PreferenceStore,
        root: &std::path::Path,
        manifest: &pack::VoicePackManifest,
    ) -> Result<String, String> {
        match pack::health_check(root, manifest) {
            health @ VoicePackHealth::Ready { .. } => {
                let engine_mode = preference_store.load().voice.engine_mode;
                match engine::run_voice_engine_health_check(manifest, health, engine_mode) {
                    Ok(VoiceEngineEvent::Health { ok: true, detail }) => Ok(detail),
                    Ok(VoiceEngineEvent::Health { detail, .. })
                    | Ok(VoiceEngineEvent::Error(detail)) => Err(detail),
                    Ok(other) => Err(format!("inconclusive health check: {other:?}")),
                    Err(error) => Err(format!("failed to run health check: {error}")),
                }
            }
            VoicePackHealth::Missing => Err("NVIDIA Parakeet voice pack is not installed".into()),
            VoicePackHealth::Broken(message) => Err(format!(
                "NVIDIA Parakeet voice pack is incomplete: {message}"
            )),
        }
    }

    fn refresh_builtin_voice_pack_assets_for_runtime(root: &std::path::Path) -> Result<(), String> {
        match pack::refresh_builtin_parakeet_pack_assets(root) {
            Ok(Some(manifest)) => {
                logging::info(format!(
                    "refreshed bundled NVIDIA Parakeet voice pack assets id={} version={}",
                    manifest.id, manifest.version
                ));
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(error) => Err(format!("{error:?}")),
        }
    }

    fn save_voice_pack_download_progress(preference_store: &PreferenceStore, percent: u8) {
        let mut preferences = preference_store.load();
        if matches!(
            preferences.voice.pack_status,
            VoicePackStatus::Installed { .. } | VoicePackStatus::Error { .. }
        ) {
            return;
        }
        preferences.voice.pack_status = VoicePackStatus::Downloading {
            percent: percent.clamp(1, 99),
        };
        preference_store.save(&preferences);
    }

    fn save_voice_pack_error(preference_store: &PreferenceStore, message: &str) {
        let mut preferences = preference_store.load();
        preferences.voice.pack_status = VoicePackStatus::Error {
            message: message.to_string(),
        };
        preference_store.save(&preferences);
    }

    fn present_assets_manager(
        window: &adw::ApplicationWindow,
        overlay: &adw::ToastOverlay,
        asset_store: AssetStore,
    ) {
        let workspace_root = std::env::current_dir().ok();
        assets_manager::present(
            window,
            Rc::new(asset_store),
            workspace_root,
            Rc::new({
                let overlay = overlay.clone();
                move || {
                    logging::info("Windows GTK assets manager saved assets");
                    overlay.add_toast(adw::Toast::new("Assets saved"));
                }
            }),
        );
    }

    fn open_logs_folder(overlay: &adw::ToastOverlay) {
        match logging::ensure_log_directory() {
            Ok(path) => {
                let uri = gio::File::for_path(&path).uri();
                match gio::AppInfo::launch_default_for_uri(
                    uri.as_str(),
                    None::<&gio::AppLaunchContext>,
                ) {
                    Ok(()) => {
                        logging::info(format!("opened application logs folder {}", path.display()));
                        overlay.add_toast(adw::Toast::new("Opened logs folder"));
                    }
                    Err(error) => {
                        logging::error(format!(
                            "failed to open application logs folder '{}': {}",
                            path.display(),
                            error
                        ));
                        overlay.add_toast(adw::Toast::new("Failed to open logs folder"));
                    }
                }
            }
            Err(error) => {
                logging::error(format!("failed to prepare logs folder: {error}"));
                overlay.add_toast(adw::Toast::new("Could not resolve logs folder"));
            }
        }
    }

    fn primary_window(app: &adw::Application) -> Option<adw::ApplicationWindow> {
        app.windows()
            .into_iter()
            .find_map(|window| window.downcast::<adw::ApplicationWindow>().ok())
    }

    fn sync_windows_fullscreen_chrome(
        window: &adw::ApplicationWindow,
        title_widget: &gtk::Widget,
        fullscreen_button: &gtk::Button,
        is_workspace: bool,
    ) {
        sync_workspace_fullscreen_chrome(
            window,
            title_widget,
            fullscreen_button,
            is_workspace,
            "Enter fullscreen",
            "Exit fullscreen",
        );
    }

    #[derive(Clone, Default)]
    struct WindowsGtkShellState {
        preview: Rc<RefCell<Option<crate::ui::workspace_preview::SessionPreview>>>,
        launch_deck_active: Rc<Cell<bool>>,
    }

    impl WindowsGtkShellState {
        fn has_workspace_tabs(&self) -> bool {
            self.preview
                .borrow()
                .as_ref()
                .is_some_and(|preview| !preview.snapshot().tabs.is_empty())
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn show_launch_deck_tab(
        window: &adw::ApplicationWindow,
        overlay: &adw::ToastOverlay,
        title: &TitleChrome,
        launch: &gtk::Widget,
        back_button: &gtk::Button,
        fullscreen_button: &gtk::Button,
        shell_state: &WindowsGtkShellState,
    ) {
        shell_state.launch_deck_active.set(true);
        overlay.set_child(Some(launch));
        back_button.set_visible(shell_state.has_workspace_tabs());
        sync_windows_fullscreen_chrome(window, title.root.upcast_ref(), fullscreen_button, false);
        sync_windows_shell_title_tabs(
            window,
            overlay,
            title,
            launch,
            back_button,
            fullscreen_button,
            shell_state,
        );
        logging::info("Windows GTK shell selected launch deck tab");
    }

    #[allow(clippy::too_many_arguments)]
    fn show_workspace_preview_tab(
        window: &adw::ApplicationWindow,
        overlay: &adw::ToastOverlay,
        title: &TitleChrome,
        launch: &gtk::Widget,
        back_button: &gtk::Button,
        fullscreen_button: &gtk::Button,
        shell_state: &WindowsGtkShellState,
        index: usize,
    ) {
        let preview = shell_state.preview.borrow().clone();
        let Some(preview) = preview else {
            show_launch_deck_tab(
                window,
                overlay,
                title,
                launch,
                back_button,
                fullscreen_button,
                shell_state,
            );
            return;
        };

        shell_state.launch_deck_active.set(false);
        preview.select_tab(index);
        overlay.set_child(Some(&preview.widget()));
        back_button.set_visible(true);
        sync_windows_fullscreen_chrome(window, title.root.upcast_ref(), fullscreen_button, true);
        sync_windows_shell_title_tabs(
            window,
            overlay,
            title,
            launch,
            back_button,
            fullscreen_button,
            shell_state,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn sync_windows_shell_title_tabs(
        window: &adw::ApplicationWindow,
        overlay: &adw::ToastOverlay,
        title: &TitleChrome,
        launch: &gtk::Widget,
        back_button: &gtk::Button,
        fullscreen_button: &gtk::Button,
        shell_state: &WindowsGtkShellState,
    ) {
        let mut tabs = Vec::new();
        let launch_active =
            shell_state.launch_deck_active.get() || !shell_state.has_workspace_tabs();

        tabs.push(WindowsTitleTab {
            label: "Templates".into(),
            tooltip: "Workspace launch deck".into(),
            active: launch_active,
            on_select: Some(Rc::new({
                let window = window.clone();
                let overlay = overlay.clone();
                let title = title.clone();
                let launch = launch.clone();
                let back_button = back_button.clone();
                let fullscreen_button = fullscreen_button.clone();
                let shell_state = shell_state.clone();
                move || {
                    show_launch_deck_tab(
                        &window,
                        &overlay,
                        &title,
                        &launch,
                        &back_button,
                        &fullscreen_button,
                        &shell_state,
                    );
                }
            })),
            on_close: None,
        });

        if let Some(preview) = shell_state.preview.borrow().as_ref() {
            let session = preview.snapshot();
            let active_index = preview.active_index();
            for (index, tab) in session.tabs.iter().enumerate() {
                let label = tab
                    .custom_title
                    .as_deref()
                    .unwrap_or(tab.preset.name.as_str())
                    .to_string();
                let tooltip = tab.workspace_root.display().to_string();

                tabs.push(WindowsTitleTab {
                    label,
                    tooltip,
                    active: !launch_active && index == active_index,
                    on_select: Some(Rc::new({
                        let window = window.clone();
                        let overlay = overlay.clone();
                        let title = title.clone();
                        let launch = launch.clone();
                        let back_button = back_button.clone();
                        let fullscreen_button = fullscreen_button.clone();
                        let shell_state = shell_state.clone();
                        move || {
                            show_workspace_preview_tab(
                                &window,
                                &overlay,
                                &title,
                                &launch,
                                &back_button,
                                &fullscreen_button,
                                &shell_state,
                                index,
                            );
                        }
                    })),
                    on_close: Some(Rc::new({
                        let window = window.clone();
                        let overlay = overlay.clone();
                        let title = title.clone();
                        let launch = launch.clone();
                        let back_button = back_button.clone();
                        let fullscreen_button = fullscreen_button.clone();
                        let shell_state = shell_state.clone();
                        move || {
                            let preview = shell_state.preview.borrow().clone();
                            let Some(preview) = preview else {
                                return;
                            };
                            if !preview.close_tab(index) {
                                return;
                            }
                            if preview.snapshot().tabs.is_empty() {
                                *shell_state.preview.borrow_mut() = None;
                                show_launch_deck_tab(
                                    &window,
                                    &overlay,
                                    &title,
                                    &launch,
                                    &back_button,
                                    &fullscreen_button,
                                    &shell_state,
                                );
                            } else if shell_state.launch_deck_active.get() {
                                show_launch_deck_tab(
                                    &window,
                                    &overlay,
                                    &title,
                                    &launch,
                                    &back_button,
                                    &fullscreen_button,
                                    &shell_state,
                                );
                            } else {
                                show_workspace_preview_tab(
                                    &window,
                                    &overlay,
                                    &title,
                                    &launch,
                                    &back_button,
                                    &fullscreen_button,
                                    &shell_state,
                                    preview.active_index(),
                                );
                            }
                        }
                    })),
                });
            }
        }

        sync_windows_title_tabs(title, tabs);
    }

    fn present_workspace_preview_from_launch(
        window: &adw::ApplicationWindow,
        overlay: &adw::ToastOverlay,
        title: &TitleChrome,
        _preferences: &AppPreferences,
        back_button: &gtk::Button,
        fullscreen_button: &gtk::Button,
        shell_state: &WindowsGtkShellState,
        launch: &gtk::Widget,
        assets: crate::model::assets::WorkspaceAssets,
        preset: crate::model::preset::WorkspacePreset,
        workspace_root: PathBuf,
    ) {
        let saved_tab = SavedTab {
            preset,
            workspace_root,
            custom_title: None,
            terminal_zoom_steps: 0,
        };

        if let Some(preview) = shell_state.preview.borrow().as_ref().cloned() {
            preview.push_tab(saved_tab);
            shell_state.launch_deck_active.set(false);
            overlay.set_child(Some(&preview.widget()));
            back_button.set_visible(true);
            sync_windows_fullscreen_chrome(
                window,
                title.root.upcast_ref(),
                fullscreen_button,
                true,
            );
            sync_windows_shell_title_tabs(
                window,
                overlay,
                title,
                launch,
                back_button,
                fullscreen_button,
                shell_state,
            );
            let snapshot = preview.snapshot();
            let (tabs, panes) = crate::ui::workspace_preview::session_shape(&snapshot);
            logging::info(format!(
                "Windows GTK shell opened interactive GTK workspace with {tabs} tab(s) and {panes} pane(s)"
            ));
            overlay.add_toast(adw::Toast::new(
                "Workspace opened as an interactive GTK tab",
            ));
        } else {
            let session = SavedSession {
                tabs: vec![saved_tab],
                active_tab_index: 0,
            };

            present_workspace_preview(
                window,
                overlay,
                title,
                back_button,
                fullscreen_button,
                shell_state,
                launch,
                session,
                assets,
                "opened",
            );
        }
    }

    fn present_workspace_preview(
        window: &adw::ApplicationWindow,
        overlay: &adw::ToastOverlay,
        title: &TitleChrome,
        back_button: &gtk::Button,
        fullscreen_button: &gtk::Button,
        shell_state: &WindowsGtkShellState,
        launch: &gtk::Widget,
        session: SavedSession,
        assets: crate::model::assets::WorkspaceAssets,
        action: &str,
    ) {
        let (tabs, panes) = crate::ui::workspace_preview::session_shape(&session);
        let preview = crate::ui::workspace_preview::SessionPreview::with_runtime_assets(
            &session,
            false,
            assets,
            Some(Rc::new(
                crate::windows::gtk_runtime::build_tile_runtime_surface,
            )),
        );
        *shell_state.preview.borrow_mut() = Some(preview.clone());
        shell_state.launch_deck_active.set(false);
        overlay.set_child(Some(&preview.widget()));
        back_button.set_visible(true);
        sync_windows_fullscreen_chrome(window, title.root.upcast_ref(), fullscreen_button, true);
        sync_windows_shell_title_tabs(
            window,
            overlay,
            title,
            launch,
            back_button,
            fullscreen_button,
            shell_state,
        );
        logging::info(format!(
            "Windows GTK shell {action} interactive GTK workspace with {tabs} tab(s) and {panes} pane(s)"
        ));
        overlay.add_toast(adw::Toast::new(
            "Workspace opened in the shared interactive GTK shell",
        ));
    }

    struct WindowsTitleTab {
        label: String,
        tooltip: String,
        active: bool,
        on_select: Option<Rc<dyn Fn()>>,
        on_close: Option<Rc<dyn Fn()>>,
    }

    fn sync_windows_title_tabs(title: &TitleChrome, tabs: Vec<WindowsTitleTab>) {
        while let Some(child) = title.tabs_box.first_child() {
            title.tabs_box.remove(&child);
        }

        for tab in tabs {
            title.tabs_box.append(&build_windows_title_tab(tab));
        }
    }

    fn build_windows_title_tab(tab: WindowsTitleTab) -> gtk::Widget {
        let chrome = build_title_tab_chrome();
        apply_title_tab_state(
            &chrome,
            &tab.label,
            &tab.tooltip,
            tab.active,
            tab.on_close.is_some(),
        );

        if let Some(on_select) = tab.on_select {
            chrome.select_button.connect_clicked(move |_| on_select());
        }

        if let Some(on_close) = tab.on_close {
            chrome.close_button.connect_clicked(move |_| on_close());
        }

        chrome.shell.upcast()
    }

    type ShortcutControllerHandle = Rc<RefCell<Option<gtk::ShortcutController>>>;

    #[allow(clippy::too_many_arguments)]
    fn present_command_palette(
        window: &adw::ApplicationWindow,
        overlay: &adw::ToastOverlay,
        title: &TitleChrome,
        launch: &gtk::Widget,
        back_button: &gtk::Button,
        fullscreen_button: &gtk::Button,
        shell_state: &WindowsGtkShellState,
        preference_store: PreferenceStore,
        preset_store: PresetStore,
        asset_store: AssetStore,
        options: RuntimeOptions,
        voice_toast_tx: mpsc::Sender<String>,
    ) {
        let mut actions = vec![
            command_palette::PaletteAction {
                title: "Show Templates".into(),
                subtitle: "Return to the shared workspace launch deck.".into(),
                on_activate: Rc::new({
                    let window = window.clone();
                    let overlay = overlay.clone();
                    let title = title.clone();
                    let launch = launch.clone();
                    let back_button = back_button.clone();
                    let fullscreen_button = fullscreen_button.clone();
                    let shell_state = shell_state.clone();
                    move || {
                        show_launch_deck_tab(
                            &window,
                            &overlay,
                            &title,
                            &launch,
                            &back_button,
                            &fullscreen_button,
                            &shell_state,
                        );
                    }
                }),
            },
            command_palette::PaletteAction {
                title: "Open Settings".into(),
                subtitle: "Application preferences, shortcuts, theme, density, and voice.".into(),
                on_activate: Rc::new({
                    let window = window.clone();
                    let overlay = overlay.clone();
                    let preference_store = preference_store.clone();
                    let preset_store = preset_store.clone();
                    let options = options.clone();
                    let voice_toast_tx = voice_toast_tx.clone();
                    move || {
                        present_settings_dialog(
                            &window,
                            &overlay,
                            preference_store.clone(),
                            preset_store.clone(),
                            options.clone(),
                            voice_toast_tx.clone(),
                        );
                    }
                }),
            },
            command_palette::PaletteAction {
                title: "Open Assets Manager".into(),
                subtitle: "Edit global or workspace scoped assets.".into(),
                on_activate: Rc::new({
                    let window = window.clone();
                    let overlay = overlay.clone();
                    let asset_store = asset_store.clone();
                    move || present_assets_manager(&window, &overlay, asset_store.clone())
                }),
            },
            command_palette::PaletteAction {
                title: format!("About {}", product::PRODUCT_DISPLAY_NAME),
                subtitle: "Version, license, source, and open-core model.".into(),
                on_activate: Rc::new({
                    let window = window.clone();
                    let options = options.clone();
                    move || about_dialog::present(&window, &options.product)
                }),
            },
        ];

        if let Some(companion) = options.companion.clone() {
            actions.push(command_palette::PaletteAction {
                title: "Open Account / Sync".into(),
                subtitle: "Account, activation, device, and sync controls.".into(),
                on_activate: Rc::new({
                    let window = window.clone();
                    move || companion_dialog::present(&window, companion.clone())
                }),
            });
        }

        if let Some(preview) = shell_state.preview.borrow().as_ref() {
            let session = preview.snapshot();
            for (index, tab) in session.tabs.iter().enumerate() {
                let label = tab
                    .custom_title
                    .as_deref()
                    .unwrap_or(tab.preset.name.as_str());
                actions.push(command_palette::PaletteAction {
                    title: format!("Switch to {label}"),
                    subtitle: tab.workspace_root.display().to_string(),
                    on_activate: Rc::new({
                        let window = window.clone();
                        let overlay = overlay.clone();
                        let title = title.clone();
                        let launch = launch.clone();
                        let back_button = back_button.clone();
                        let fullscreen_button = fullscreen_button.clone();
                        let shell_state = shell_state.clone();
                        move || {
                            show_workspace_preview_tab(
                                &window,
                                &overlay,
                                &title,
                                &launch,
                                &back_button,
                                &fullscreen_button,
                                &shell_state,
                                index,
                            );
                        }
                    }),
                });
            }
        }

        command_palette::present(window, actions);
    }

    fn install_command_palette_shortcut(
        window: &adw::ApplicationWindow,
        controller_handle: &ShortcutControllerHandle,
        shortcut: &str,
        open_command_palette: Rc<dyn Fn()>,
    ) {
        install_shortcut_controller(
            window,
            controller_handle,
            "command_palette",
            &command_palette_shortcut_accelerators(shortcut),
            move || {
                open_command_palette();
                glib::Propagation::Stop
            },
        );
    }

    fn install_shortcut_controller<F>(
        window: &adw::ApplicationWindow,
        controller_handle: &ShortcutControllerHandle,
        shortcut_name: &str,
        accelerators: &[String],
        on_activate: F,
    ) where
        F: Fn() -> glib::Propagation + 'static,
    {
        if let Some(existing) = controller_handle.borrow_mut().take() {
            window.remove_controller(&existing);
        }

        let shortcut_controller = gtk::ShortcutController::new();
        shortcut_controller.set_scope(gtk::ShortcutScope::Global);
        shortcut_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        let on_activate = Rc::new(on_activate);
        let mut installed_triggers = Vec::new();
        let mut active_labels = Vec::new();
        for accelerator in accelerators {
            let accelerator = accelerator.trim();
            if accelerator.is_empty() || installed_triggers.iter().any(|item| item == accelerator) {
                continue;
            }
            installed_triggers.push(accelerator.to_string());

            let Some(trigger) = gtk::ShortcutTrigger::parse_string(accelerator) else {
                logging::error(format!(
                    "failed to parse {} shortcut accelerator='{}'",
                    shortcut_name, accelerator
                ));
                continue;
            };

            active_labels.push(trigger.to_str().to_string());
            let on_activate = on_activate.clone();
            let action = gtk::CallbackAction::new(move |_, _| on_activate());
            shortcut_controller.add_shortcut(gtk::Shortcut::new(Some(trigger), Some(action)));
        }

        if installed_triggers.is_empty() {
            logging::error(format!(
                "failed to install {} shortcut: no valid accelerators",
                shortcut_name,
            ));
            return;
        }

        logging::info(format!(
            "installed {} shortcut requested={:?} active={:?}",
            shortcut_name, installed_triggers, active_labels
        ));
        window.add_controller(shortcut_controller.clone());
        *controller_handle.borrow_mut() = Some(shortcut_controller);
    }

    fn command_palette_shortcut_accelerators(shortcut: &str) -> Vec<String> {
        equivalent_shortcut_accelerators(
            shortcut,
            &[
                &["<Ctrl><Shift>P", "<Primary><Shift>P", "<Control><Shift>P"],
                &["<Ctrl>P", "<Primary>P", "<Control>P"],
            ],
        )
    }

    fn equivalent_shortcut_accelerators(shortcut: &str, families: &[&[&str]]) -> Vec<String> {
        let trimmed = shortcut.trim();
        let mut accelerators = vec![trimmed.to_string()];

        if let Some(family) = families
            .iter()
            .find(|family| family.iter().any(|candidate| candidate == &trimmed))
        {
            accelerators.extend(family.iter().map(|candidate| (*candidate).to_string()));
        }

        accelerators
    }

    fn combine_warnings(first: Option<String>, second: Option<String>) -> Option<String> {
        match (first, second) {
            (Some(first), Some(second)) if !second.trim().is_empty() => {
                Some(format!("{first}\n{second}"))
            }
            (Some(first), _) => Some(first),
            (_, Some(second)) => Some(second),
            (None, None) => None,
        }
    }
}

#[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
pub use imp::{run, run_with_options};
