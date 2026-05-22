#[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
mod imp {
    use std::path::PathBuf;
    use std::process::ExitCode;
    use std::rc::Rc;
    use std::sync::mpsc;
    use std::time::Duration;

    use adw::prelude::*;
    use gtk::gio;

    use crate::extension::RuntimeOptions;
    use crate::logging;
    use crate::services::session_restore::session_for_restore_mode;
    use crate::storage::asset_store::AssetStore;
    use crate::storage::preference_store::{AppPreferences, PreferenceStore};
    use crate::storage::preset_store::PresetStore;
    use crate::storage::session_store::{SavedSession, SavedTab, SessionStore};
    use crate::ui::appearance::{apply_theme_mode, apply_window_density};
    use crate::ui::icons::{self, name as icon_name};
    use crate::ui::launch_screen::{LaunchScreenActions, LaunchScreenInput};
    use crate::ui::title_chrome::{TitleChrome, build_title_tab_chrome};
    use crate::ui::{assets_manager, settings_dialog};
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
        let session_store = SessionStore::new();
        let session_outcome = session_store.load_with_status();
        let load_warning = combine_warnings(
            combine_warnings(preset_outcome.warning, asset_outcome.warning),
            session_outcome.warning,
        );

        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(true)
            .show_end_title_buttons(true)
            .build();
        header.set_centering_policy(adw::CenteringPolicy::Loose);
        header.add_css_class("app-headerbar");

        let title = TitleChrome::new();
        title.root.add_css_class("app-title-handle");
        title.add_button.set_sensitive(false);
        header.set_title_widget(Some(&title.root));

        let overlay = adw::ToastOverlay::new();
        let settings_button = icons::icon_button(
            icon_name::SETTINGS,
            "Open application settings",
            &["flat", "titlebar-action-button", "titlebar-icon-button"],
        );
        header.pack_end(&settings_button);

        let assets_button = icons::icon_button(
            icon_name::ASSETS,
            "Open assets manager",
            &["flat", "titlebar-action-button", "titlebar-icon-button"],
        );
        header.pack_end(&assets_button);

        let window_shell = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .build();
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

        sync_windows_title_tabs(
            &title,
            vec![WindowsTitleTab {
                label: "Workspace 1".into(),
                tooltip: "Launch deck".into(),
                active: true,
                on_select: None,
            }],
        );

        let launch_preferences = preferences.clone();
        let launch_overlay = overlay.clone();
        let launch_title = title.clone();
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
                present_workspace_preview_from_launch(
                    &launch_overlay,
                    &launch_title,
                    &launch_preferences,
                    preset,
                    workspace_root,
                );
            }),
            on_cancel: Rc::new({
                let app = app.clone();
                move || app.quit()
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
        overlay.set_child(Some(&launch));
        window.present();

        if let Some(session) = session_outcome
            .session
            .as_ref()
            .and_then(|session| session_for_restore_mode(session, preferences.default_restore_mode))
        {
            let overlay = overlay.clone();
            let title = title.clone();
            let preferences = preferences.clone();
            gtk::glib::idle_add_local_once(move || {
                present_workspace_preview_from_restore(&overlay, &title, &preferences, session);
            });
        }
    }

    fn present_workspace_preview_from_restore(
        overlay: &adw::ToastOverlay,
        title: &TitleChrome,
        _preferences: &AppPreferences,
        session: SavedSession,
    ) {
        present_workspace_preview(overlay, title, session, "restored");
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
                        let message = format!("{error:?}");
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
                let message = format!("{error:?}");
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

    fn present_workspace_preview_from_launch(
        overlay: &adw::ToastOverlay,
        title: &TitleChrome,
        _preferences: &AppPreferences,
        preset: crate::model::preset::WorkspacePreset,
        workspace_root: PathBuf,
    ) {
        let session = SavedSession {
            tabs: vec![SavedTab {
                preset,
                workspace_root,
                custom_title: None,
                terminal_zoom_steps: 0,
            }],
            active_tab_index: 0,
        };

        present_workspace_preview(overlay, title, session, "opened");
    }

    fn present_workspace_preview(
        overlay: &adw::ToastOverlay,
        title: &TitleChrome,
        session: SavedSession,
        action: &str,
    ) {
        let (tabs, panes) = crate::ui::workspace_preview::session_shape(&session);
        let preview = crate::ui::workspace_preview::SessionPreview::new(&session, false);
        sync_title_tabs_for_session(title, &session, &preview);
        overlay.set_child(Some(&preview.widget()));
        logging::info(format!(
            "Windows GTK shell {action} GTK workspace preview with {tabs} tab(s) and {panes} pane(s)"
        ));
        overlay.add_toast(adw::Toast::new(
            "Workspace opened in the shared GTK visual shell",
        ));
    }

    struct WindowsTitleTab {
        label: String,
        tooltip: String,
        active: bool,
        on_select: Option<Rc<dyn Fn()>>,
    }

    fn sync_title_tabs_for_session(
        title: &TitleChrome,
        session: &SavedSession,
        preview: &crate::ui::workspace_preview::SessionPreview,
    ) {
        let session = Rc::new(session.clone());
        let active_index = preview.active_index();
        let tabs = session
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let label = tab
                    .custom_title
                    .as_deref()
                    .unwrap_or(tab.preset.name.as_str())
                    .to_string();
                let tooltip = tab.workspace_root.display().to_string();
                let preview = preview.clone();
                let title = title.clone();
                let session = session.clone();
                WindowsTitleTab {
                    label,
                    tooltip,
                    active: index == active_index,
                    on_select: Some(Rc::new(move || {
                        preview.select_tab(index);
                        sync_title_tabs_for_session(&title, &session, &preview);
                    })),
                }
            })
            .collect();

        sync_windows_title_tabs(title, tabs);
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
        let shell = chrome.shell;
        shell.remove_css_class("is-inactive");
        shell.remove_css_class("is-active");
        shell.add_css_class(if tab.active {
            "is-active"
        } else {
            "is-inactive"
        });
        shell.set_tooltip_text(Some(&tab.tooltip));
        chrome.title_label.set_label(&tab.label);

        if let Some(on_select) = tab.on_select {
            chrome.select_button.connect_clicked(move |_| on_select());
        }

        chrome.close_button.set_sensitive(false);

        shell.upcast()
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
