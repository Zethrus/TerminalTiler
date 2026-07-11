#[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
mod imp {
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;
    use std::process::ExitCode;
    use std::rc::{Rc, Weak};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use adw::prelude::*;
    use glib::value::ToValue;
    use gtk::{gdk, gio, glib};

    use crate::extension::{ProductIdentity, RuntimeOptions};
    use crate::logging;
    use crate::model::assets::RestoreLaunchMode;
    use crate::model::layout::DEFAULT_WEB_URL;
    use crate::services::agent_resume::{
        RestoreStartupOverridesByTab, restore_startup_override_for_tab_tile,
        restore_startup_overrides_for_saved_session,
    };
    use crate::services::session_restore::session_for_restore_mode;
    use crate::services::tile_navigation::TileDirection;
    use crate::storage::asset_store::AssetStore;
    use crate::storage::board_workspace_store::BoardWorkspaceStore;
    use crate::storage::preference_store::{AppPreferences, PreferenceStore};
    use crate::storage::preset_store::PresetStore;
    use crate::storage::session_store::{SavedSession, SavedTab, SessionStore};
    use crate::ui::app_chrome::{
        apply_app_headerbar_class, build_app_header_chrome, build_main_titlebar_actions,
        build_window_shell, sync_workspace_fullscreen_chrome,
    };
    use crate::ui::appearance::{apply_theme_mode, apply_window_density};
    use crate::ui::launch_screen::{LaunchScreenActions, LaunchScreenInput};
    use crate::ui::stats_dialog;
    use crate::ui::title_chrome::{TitleChrome, TitleTabInput, build_interactive_title_tab};
    use crate::ui::{
        about_dialog, assets_manager, command_palette, companion_dialog, context_menu,
        dialog_chrome, dialog_smoke, mcp_health_panel, settings_dialog, tab_rename_dialog,
        voice_hud::VoiceHud,
    };
    // Source-contract anchor: dialog_smoke, settings_dialog, tab_rename_dialog.
    use crate::voice::audio::AudioCapture;
    use crate::voice::engine::{self, VoiceEngineEvent};
    use crate::voice::pack::{self, VoicePackHealth};
    use crate::voice::{
        ParakeetTranscriber, VoiceActivationMode, VoiceEngineMode, VoicePackStatus,
    };
    use crate::windows::gtk_tray::WindowsGtkTrayController;
    use crate::windows::gtk_voice_hotkey::{WindowsGlobalHotkeyEvent, WindowsGlobalHotkeyHandle};
    use crate::windows::win32_helpers::open_path_with_shell;

    const VOICE_AUDIO_FLUSH_INTERVAL: Duration = Duration::from_millis(250);

    pub fn run() -> ExitCode {
        run_with_options(RuntimeOptions::default())
    }

    pub fn run_with_options(options: RuntimeOptions) -> ExitCode {
        logging::init();
        logging::info("windows GTK shell startup");
        let taskbar_app_user_model_id = options.product.effective_windows_app_user_model_id();
        configure_windows_taskbar_identity(taskbar_app_user_model_id);

        let app_id = options.product.effective_gtk_application_id();
        let app = adw::Application::builder().application_id(app_id).build();

        let icon_name = options.product.icon_name.clone();
        app.connect_startup(move |_| {
            crate::gtk_shell::load_css_for_default_display();
            crate::gtk_shell::configure_application_icons_for(&icon_name);
            logging::info("windows GTK shell loaded canonical GTK CSS and app icon contract");
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

    fn configure_windows_taskbar_identity(app_user_model_id: &str) {
        let app_user_model_id = app_user_model_id
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>();
        let status = unsafe {
            windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(
                app_user_model_id.as_ptr(),
            )
        };
        if status < 0 {
            logging::info(format!(
                "Windows taskbar AppUserModelID setup warning: HRESULT 0x{:08X}",
                status as u32
            ));
        }
    }

    enum WindowsVoiceTranscriberCommand {
        Start {
            manifest: pack::VoicePackManifest,
            health: VoicePackHealth,
            engine_mode: VoiceEngineMode,
            microphone_id: Option<String>,
            ui_tx: mpsc::Sender<WindowsVoiceUiEvent>,
        },
        Flush {
            ui_tx: mpsc::Sender<WindowsVoiceUiEvent>,
        },
        Stop {
            ui_tx: mpsc::Sender<WindowsVoiceUiEvent>,
        },
        Shutdown,
    }

    #[derive(Debug)]
    enum WindowsVoiceUiEvent {
        ListeningStarted,
        Final(String),
        Partial(String),
        Status(String),
        Error(String),
        GlobalHotkeyActivated,
    }

    #[derive(Clone)]
    struct WindowsVoiceTranscriberHandle {
        tx: mpsc::Sender<WindowsVoiceTranscriberCommand>,
    }

    enum WindowsVoiceGlobalHotkeyRegistration {
        Active {
            shortcut: String,
            handle: WindowsGlobalHotkeyHandle,
        },
        Unavailable {
            shortcut: String,
            last_attempt: Instant,
        },
    }

    impl WindowsVoiceGlobalHotkeyRegistration {
        fn shortcut(&self) -> &str {
            match self {
                Self::Active { shortcut, .. } | Self::Unavailable { shortcut, .. } => shortcut,
            }
        }

        fn unavailable_retry_pending(&self) -> bool {
            matches!(
                self,
                Self::Unavailable { last_attempt, .. } if last_attempt.elapsed() < Duration::from_secs(30)
            )
        }
    }

    impl WindowsVoiceTranscriberHandle {
        fn start() -> Self {
            let (tx, rx) = mpsc::channel::<WindowsVoiceTranscriberCommand>();
            std::thread::spawn(move || run_windows_voice_transcriber_worker(rx));
            Self { tx }
        }

        fn start_capture(
            &self,
            manifest: pack::VoicePackManifest,
            health: VoicePackHealth,
            engine_mode: VoiceEngineMode,
            microphone_id: Option<String>,
            ui_tx: &mpsc::Sender<WindowsVoiceUiEvent>,
        ) {
            let _ = self.tx.send(WindowsVoiceTranscriberCommand::Start {
                manifest,
                health,
                engine_mode,
                microphone_id,
                ui_tx: ui_tx.clone(),
            });
        }

        fn flush(&self, ui_tx: &mpsc::Sender<WindowsVoiceUiEvent>) {
            let _ = self.tx.send(WindowsVoiceTranscriberCommand::Flush {
                ui_tx: ui_tx.clone(),
            });
        }

        fn stop(&self, ui_tx: &mpsc::Sender<WindowsVoiceUiEvent>) {
            let _ = self.tx.send(WindowsVoiceTranscriberCommand::Stop {
                ui_tx: ui_tx.clone(),
            });
        }

        fn shutdown(&self) {
            let _ = self.tx.send(WindowsVoiceTranscriberCommand::Shutdown);
        }
    }

    fn run_windows_voice_transcriber_worker(rx: mpsc::Receiver<WindowsVoiceTranscriberCommand>) {
        let mut transcriber: Option<ParakeetTranscriber> = None;
        for command in rx {
            match command {
                WindowsVoiceTranscriberCommand::Start {
                    manifest,
                    health,
                    engine_mode,
                    microphone_id,
                    ui_tx,
                } => {
                    if let Some(previous) = transcriber.take() {
                        let _ = previous.shutdown();
                    }
                    match ParakeetTranscriber::launch(&manifest, health, engine_mode).and_then(
                        |mut transcriber| {
                            transcriber.start_capture(microphone_id.as_deref())?;
                            Ok(transcriber)
                        },
                    ) {
                        Ok(next) => {
                            transcriber = Some(next);
                            let _ = ui_tx.send(WindowsVoiceUiEvent::ListeningStarted);
                            let _ = ui_tx.send(WindowsVoiceUiEvent::Status("Listening…".into()));
                        }
                        Err(error) => {
                            let _ = ui_tx.send(WindowsVoiceUiEvent::Error(format!("{error:?}")));
                        }
                    }
                }
                WindowsVoiceTranscriberCommand::Flush { ui_tx } => {
                    let Some(transcriber) = transcriber.as_mut() else {
                        continue;
                    };
                    match transcriber.flush_captured_audio() {
                        Ok(Some(partial)) => {
                            let _ = ui_tx.send(WindowsVoiceUiEvent::Partial(partial));
                        }
                        Ok(None) => {}
                        Err(error) => {
                            let _ = ui_tx.send(WindowsVoiceUiEvent::Error(format!("{error:?}")));
                        }
                    }
                }
                WindowsVoiceTranscriberCommand::Stop { ui_tx } => {
                    let Some(mut transcriber) = transcriber.take() else {
                        let _ = ui_tx.send(WindowsVoiceUiEvent::Final(String::new()));
                        continue;
                    };
                    let partial_tx = ui_tx.clone();
                    let result = transcriber.stop_capture_and_transcribe_with_partials(|partial| {
                        let _ = partial_tx.send(WindowsVoiceUiEvent::Partial(partial));
                    });
                    let _ = transcriber.shutdown();
                    match result {
                        Ok(text) => {
                            let _ = ui_tx.send(WindowsVoiceUiEvent::Final(text));
                        }
                        Err(error) => {
                            let _ = ui_tx.send(WindowsVoiceUiEvent::Error(format!("{error:?}")));
                        }
                    }
                }
                WindowsVoiceTranscriberCommand::Shutdown => {
                    if let Some(transcriber) = transcriber.take() {
                        let _ = transcriber.shutdown();
                    }
                    break;
                }
            }
        }
    }

    fn present_launch_window(app: &adw::Application, options: &RuntimeOptions) {
        if let Some(window) = primary_window(app) {
            window.present();
            return;
        }

        let preference_store = PreferenceStore::new();
        let preferences = preference_store.load();
        let preset_store = PresetStore::new().with_catalog_provider(options.catalog.clone());
        preset_store.ensure_seeded();
        let preset_outcome = preset_store.load_presets_with_status();
        let asset_store = AssetStore::new().with_catalog_provider(options.catalog.clone());
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
        let voice_hud = VoiceHud::new();
        let content_overlay = gtk::Overlay::new();
        content_overlay.set_child(Some(&overlay));
        content_overlay.add_overlay(&voice_hud.widget());
        let titlebar_actions = build_main_titlebar_actions(&header, options.companion.is_some());
        let back_button = titlebar_actions.back_button;
        let fullscreen_button = titlebar_actions.fullscreen_button;
        let settings_button = titlebar_actions.settings_button;
        let companion_button = titlebar_actions.companion_button;
        let mcp_health_button = titlebar_actions.mcp_health_button;
        let assets_button = titlebar_actions.assets_button;

        let window_shell = build_window_shell();
        window_shell.append(&header);
        window_shell.append(&content_overlay);

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(&options.product.app_title)
            .icon_name(&options.product.icon_name)
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

        let open_assets_manager: Rc<dyn Fn()> = {
            let window = window.clone();
            let overlay = overlay.clone();
            let asset_store = asset_store.clone();
            Rc::new(move || present_assets_manager(&window, &overlay, asset_store.clone()))
        };

        {
            let open_assets_manager = open_assets_manager.clone();
            assets_button.connect_clicked(move |_| open_assets_manager());
        }
        {
            let open_assets_manager = open_assets_manager.clone();
            let action = gio::SimpleAction::new("open-assets", None);
            action.connect_activate(move |_, _| open_assets_manager());
            window.add_action(&action);
        }

        let open_companion_dialog: Option<Rc<dyn Fn()>> =
            options.companion.as_ref().map(|companion| {
                let window = window.clone();
                let companion = companion.clone();
                Rc::new(move || companion_dialog::present(&window, companion.clone()))
                    as Rc<dyn Fn()>
            });

        if let (Some(button), Some(open_companion_dialog)) =
            (companion_button.as_ref(), open_companion_dialog.as_ref())
        {
            let open_companion_dialog = open_companion_dialog.clone();
            button.connect_clicked(move |_| open_companion_dialog());
        }
        if let Some(open_companion_dialog) = open_companion_dialog.as_ref() {
            let open_companion_dialog = open_companion_dialog.clone();
            let action = gio::SimpleAction::new("open-companion", None);
            action.connect_activate(move |_, _| open_companion_dialog());
            window.add_action(&action);
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

        let shell_state = WindowsGtkShellState::new(session_store.clone(), options.product.clone());
        {
            let shell_state = shell_state.clone();
            window.connect_is_active_notify(move |window| {
                if window.is_active() {
                    shell_state.set_main_voice_target();
                }
            });
        }
        let voice_key_controller: VoiceKeyControllerHandle = Rc::new(RefCell::new(None));
        let voice_transcriber = Rc::new(WindowsVoiceTranscriberHandle::start());
        let voice_listening = Rc::new(Cell::new(false));
        let voice_starting = Rc::new(Cell::new(false));
        let voice_stopping = Rc::new(Cell::new(false));
        let voice_local_key_pressed = Rc::new(Cell::new(false));
        let voice_global_hotkey =
            Rc::new(RefCell::new(None::<WindowsVoiceGlobalHotkeyRegistration>));
        let (voice_event_tx, voice_event_rx) = mpsc::channel::<WindowsVoiceUiEvent>();
        let quit_requested = Rc::new(Cell::new(false));
        let force_quit_requested = Rc::new(Cell::new(false));
        let current_close_to_background = Rc::new(Cell::new(preferences.close_to_background));
        let open_settings_dialog_handle: VoidCallbackHandle = Rc::new(RefCell::new(None));
        shell_state.set_voice_runtime(WindowsVoiceRuntime {
            preference_store: preference_store.clone(),
            voice_transcriber: voice_transcriber.clone(),
            voice_listening: voice_listening.clone(),
            voice_starting: voice_starting.clone(),
            voice_stopping: voice_stopping.clone(),
        });
        let tray_controller = WindowsGtkTrayController::new(
            &window,
            Rc::downgrade(&open_settings_dialog_handle),
            force_quit_requested.clone(),
            options.product.tray_title.clone(),
        );
        shell_state.launch_deck_active.set(true);
        {
            let window = window.clone();
            let shell_state = shell_state.clone();
            mcp_health_button.connect_clicked(move |_| {
                let active_project_root = shell_state
                    .active_project_root()
                    .or_else(|| std::env::current_dir().ok())
                    .unwrap_or_else(|| PathBuf::from("."));
                mcp_health_panel::present_modal(
                    &window,
                    active_project_root,
                    shell_state.open_project_roots(),
                    BoardWorkspaceStore::new(),
                );
            });
        }

        {
            let shell_state = shell_state.clone();
            let voice_transcriber = voice_transcriber.clone();
            let voice_global_hotkey = voice_global_hotkey.clone();
            let quit_requested = quit_requested.clone();
            let force_quit_requested = force_quit_requested.clone();
            let current_close_to_background = current_close_to_background.clone();
            let tray_controller = tray_controller.clone();
            window.connect_close_request(move |window| {
                crate::stats_hub::flush();
                if force_quit_requested.replace(false) {
                    tray_controller.shutdown();
                    voice_global_hotkey.borrow_mut().take();
                    voice_transcriber.shutdown();
                    shutdown_windows_gtk_shell(&shell_state, "force quitting Windows GTK application");
                    return glib::Propagation::Proceed;
                }

                if !quit_requested.replace(false) && current_close_to_background.get() {
                    if tray_controller.hide_window_to_tray() {
                        return glib::Propagation::Stop;
                    }
                    logging::info("Windows GTK tray unavailable; minimizing shell to background");
                    window.minimize();
                    return glib::Propagation::Stop;
                }

                if shell_state.has_active_processes() {
                    let confirm_window = window.clone();
                    let window_for_confirm = confirm_window.clone();
                    let force_quit_requested = force_quit_requested.clone();
                    dialog_chrome::confirm_destructive_action(
                        &confirm_window,
                        "Quit Application?",
                        "One or more terminal sessions are still running. Quitting TerminalTiler now will close the application immediately even if those processes are still active.",
                        "Quit Application",
                        move || {
                            force_quit_requested.set(true);
                            window_for_confirm.close();
                        },
                    );
                    return glib::Propagation::Stop;
                }

                tray_controller.shutdown();
                voice_global_hotkey.borrow_mut().take();
                voice_transcriber.shutdown();
                shutdown_windows_gtk_shell(&shell_state, "closing Windows GTK application");
                glib::Propagation::Proceed
            });
        }
        let workspace_fullscreen_shortcut_controller: ShortcutControllerHandle =
            Rc::new(RefCell::new(None));
        let workspace_density_shortcut_controller: ShortcutControllerHandle =
            Rc::new(RefCell::new(None));
        let workspace_zoom_in_shortcut_controller: ShortcutControllerHandle =
            Rc::new(RefCell::new(None));
        let workspace_zoom_out_shortcut_controller: ShortcutControllerHandle =
            Rc::new(RefCell::new(None));
        let workspace_tile_selection_shortcut_controller: TileSelectionKeyControllerHandle =
            Rc::new(RefCell::new(None));
        let workspace_add_terminal_tile_shortcut_controller: ShortcutControllerHandle =
            Rc::new(RefCell::new(None));
        let command_palette_shortcut_controller: ShortcutControllerHandle =
            Rc::new(RefCell::new(None));
        let open_command_palette_handle: Rc<RefCell<Option<Rc<dyn Fn()>>>> =
            Rc::new(RefCell::new(None));
        let launch_widget_handle: LaunchWidgetHandle = Rc::new(RefCell::new(None));
        let refresh_launch_deck_handle: VoidCallbackHandle = Rc::new(RefCell::new(None));

        let launch_context = WindowsLaunchDeckContext {
            app: app.clone(),
            window: window.clone(),
            overlay: overlay.clone(),
            title: title.clone(),
            preference_store: preference_store.clone(),
            preset_store: preset_store.clone(),
            asset_store: asset_store.clone(),
            back_button: back_button.clone(),
            fullscreen_button: fullscreen_button.clone(),
            shell_state: shell_state.clone(),
            launch_widget_handle: launch_widget_handle.clone(),
            refresh_launch_deck_handle: refresh_launch_deck_handle.clone(),
        };
        let refresh_launch_deck_weak = Rc::downgrade(&launch_context.refresh_launch_deck_handle);
        let settings_context = WindowsSettingsDialogContext {
            window: window.clone(),
            overlay: overlay.clone(),
            title: title.clone(),
            fullscreen_button: fullscreen_button.clone(),
            shell_state: shell_state.clone(),
            current_close_to_background: current_close_to_background.clone(),
            preference_store: preference_store.clone(),
            preset_store: preset_store.clone(),
            options: options.clone(),
            voice_toast_tx: voice_toast_tx.clone(),
            voice_global_hotkey: voice_global_hotkey.clone(),
            voice_event_tx: voice_event_tx.clone(),
            workspace_fullscreen_shortcut_controller: workspace_fullscreen_shortcut_controller
                .clone(),
            workspace_density_shortcut_controller: workspace_density_shortcut_controller.clone(),
            workspace_zoom_in_shortcut_controller: workspace_zoom_in_shortcut_controller.clone(),
            workspace_zoom_out_shortcut_controller: workspace_zoom_out_shortcut_controller.clone(),
            workspace_tile_selection_shortcut_controller:
                workspace_tile_selection_shortcut_controller.clone(),
            command_palette_shortcut_controller: command_palette_shortcut_controller.clone(),
            open_command_palette_handle: open_command_palette_handle.clone(),
            refresh_launch_deck_handle: refresh_launch_deck_weak.clone(),
        };

        let launch = build_windows_launch_deck(
            &launch_context,
            &refresh_launch_deck_weak,
            load_warning,
            preset_outcome.presets,
            asset_outcome.assets,
            &preferences,
        );
        *launch_widget_handle.borrow_mut() = Some(launch.clone());
        {
            let launch_context = launch_context.clone();
            let refresh_launch_deck_weak = refresh_launch_deck_weak.clone();
            *refresh_launch_deck_handle.borrow_mut() = Some(Rc::new(move || {
                refresh_windows_launch_deck(&launch_context, &refresh_launch_deck_weak)
            }));
        }

        // The shared companion dialog activates this action after an account
        // or catalog mutation. Keep the action name identical to Linux so the
        // generic CompanionIntegration contract behaves consistently across
        // GTK shells.
        {
            let refresh_launch_deck_handle = refresh_launch_deck_weak.clone();
            let action = gio::SimpleAction::new("refresh-catalog", None);
            action.connect_activate(move |_, _| {
                request_windows_launch_deck_refresh(&refresh_launch_deck_handle);
            });
            window.add_action(&action);
        }

        // Companion implementations may also publish asynchronous entitlement
        // and catalog changes (for example activation or revocation). Drain on
        // the GTK thread, refresh the provider-backed launch deck, and surface
        // the redacted user-facing event message through the shell toast.
        if let Some(companion) = options.companion.clone() {
            let refresh_launch_deck_handle = refresh_launch_deck_weak.clone();
            let overlay = overlay.clone();
            glib::timeout_add_local(Duration::from_millis(250), move || {
                for event in companion.drain_events() {
                    if event.refresh_scope.refreshes_main_content() {
                        request_windows_launch_deck_refresh(&refresh_launch_deck_handle);
                    }
                    if let Some(message) = event.message {
                        overlay.add_toast(adw::Toast::new(&message));
                    }
                }
                glib::ControlFlow::Continue
            });
        }
        {
            let launch_overlay = overlay.clone();
            let launch_title = title.clone();
            let launch_widget_handle = launch_widget_handle.clone();
            let launch_widget_handle_for_show = launch_widget_handle.clone();
            let back_button_for_click = back_button.clone();
            let fullscreen_for_click = fullscreen_button.clone();
            let window_for_click = window.clone();
            let title_add_button = title.add_button.clone();
            let shell_state_for_launch = shell_state.clone();
            let show_launch_deck = Rc::new(move || {
                if let Some(launch_widget) =
                    launch_widget_handle_for_show.borrow().as_ref().cloned()
                {
                    show_launch_deck_tab(
                        &window_for_click,
                        &launch_overlay,
                        &launch_title,
                        &launch_widget,
                        &back_button_for_click,
                        &fullscreen_for_click,
                        &shell_state_for_launch,
                    );
                }
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
                let launch_widget_handle = launch_widget_handle.clone();
                let back_button = back_button.clone();
                let fullscreen_button = fullscreen_button.clone();
                let shell_state = shell_state.clone();
                let settings_context = settings_context.clone();
                let asset_store = asset_store.clone();
                move || {
                    let Some(launch) = launch_widget_handle.borrow().as_ref().cloned() else {
                        return;
                    };
                    present_command_palette(
                        &window,
                        &overlay,
                        &title,
                        &launch,
                        &back_button,
                        &fullscreen_button,
                        &shell_state,
                        settings_context.clone(),
                        asset_store.clone(),
                    );
                }
            });
            *open_command_palette_handle.borrow_mut() = Some(open_command_palette.clone());
            {
                let open_command_palette = open_command_palette.clone();
                let action = gio::SimpleAction::new("open-command-palette", None);
                action.connect_activate(move |_, _| open_command_palette());
                window.add_action(&action);
            }
            install_command_palette_shortcut(
                &window,
                &command_palette_shortcut_controller,
                &preferences.command_palette_shortcut,
                open_command_palette.clone(),
            );
            install_workspace_fullscreen_shortcut(
                &window,
                &workspace_fullscreen_shortcut_controller,
                &shell_state,
                &preferences.workspace_fullscreen_shortcut,
            );
            install_workspace_density_shortcut(
                &window,
                &workspace_density_shortcut_controller,
                &shell_state,
                &preferences.workspace_density_shortcut,
            );
            install_workspace_zoom_shortcut(
                &window,
                &workspace_zoom_in_shortcut_controller,
                &shell_state,
                &preferences.workspace_zoom_in_shortcut,
                1,
                "workspace_zoom_in",
            );
            install_workspace_zoom_shortcut(
                &window,
                &workspace_zoom_out_shortcut_controller,
                &shell_state,
                &preferences.workspace_zoom_out_shortcut,
                -1,
                "workspace_zoom_out",
            );
            install_workspace_tile_selection_shortcut(
                &window,
                &workspace_tile_selection_shortcut_controller,
                &shell_state,
                &preferences.workspace_tile_selection_prefix_shortcut,
            );
            install_workspace_add_terminal_tile_shortcut(
                &window,
                &workspace_add_terminal_tile_shortcut_controller,
                &shell_state,
            );
            let open_settings_dialog: Rc<dyn Fn()> = Rc::new({
                let settings_context = settings_context.clone();
                move || present_settings_dialog(settings_context.clone())
            });
            *open_settings_dialog_handle.borrow_mut() = Some(open_settings_dialog.clone());
            {
                let open_settings_dialog = open_settings_dialog.clone();
                settings_button.connect_clicked(move |_| open_settings_dialog());
            }
            {
                let open_settings_dialog = open_settings_dialog.clone();
                let action = gio::SimpleAction::new("open-settings", None);
                action.connect_activate(move |_, _| open_settings_dialog());
                window.add_action(&action);
            }
            {
                let window_for_quit_action = window.clone();
                let quit_requested = quit_requested.clone();
                let action = gio::SimpleAction::new("quit-app", None);
                action.connect_activate(move |_, _| {
                    quit_requested.set(true);
                    window_for_quit_action.close();
                });
                window.add_action(&action);
            }
        }

        install_windows_voice_hotkey_controller(
            &window,
            &voice_key_controller,
            preference_store.clone(),
            shell_state.clone(),
            voice_hud.clone(),
            overlay.clone(),
            voice_transcriber.clone(),
            voice_listening.clone(),
            voice_starting.clone(),
            voice_stopping.clone(),
            voice_local_key_pressed.clone(),
            voice_event_tx.clone(),
        );

        gtk::glib::timeout_add_seconds_local(30, || {
            crate::stats_hub::flush();
            gtk::glib::ControlFlow::Continue
        });

        sync_windows_voice_global_hotkey(
            &voice_global_hotkey,
            &preference_store.load().voice,
            &voice_event_tx,
        );
        gtk::glib::timeout_add_seconds_local(2, {
            let preference_store = preference_store.clone();
            let voice_global_hotkey = voice_global_hotkey.clone();
            let voice_event_tx = voice_event_tx.clone();
            move || {
                sync_windows_voice_global_hotkey(
                    &voice_global_hotkey,
                    &preference_store.load().voice,
                    &voice_event_tx,
                );
                gtk::glib::ControlFlow::Continue
            }
        });

        {
            let preference_store = preference_store.clone();
            let shell_state = shell_state.clone();
            let voice_hud = voice_hud.clone();
            let overlay = overlay.clone();
            let voice_transcriber = voice_transcriber.clone();
            let voice_listening = voice_listening.clone();
            let voice_starting = voice_starting.clone();
            let voice_stopping = voice_stopping.clone();
            let voice_event_tx_for_handler = voice_event_tx.clone();
            gtk::glib::timeout_add_local(Duration::from_millis(80), move || {
                while let Ok(event) = voice_event_rx.try_recv() {
                    match event {
                        WindowsVoiceUiEvent::ListeningStarted => {
                            voice_starting.set(false);
                            voice_listening.set(!voice_stopping.get());
                        }
                        WindowsVoiceUiEvent::Final(text) => {
                            voice_starting.set(false);
                            voice_listening.set(false);
                            voice_stopping.set(false);
                            if text.trim().is_empty() {
                                voice_hud.show("No speech detected", None);
                                voice_hud.hide_later();
                                continue;
                            }
                            let inserted = shell_state
                                .voice_target()
                                .map(|preview| preview.send_text_to_focused_terminal(&text))
                                .unwrap_or(false);
                            if inserted {
                                voice_hud.show("Voice inserted", Some(&text));
                                voice_hud.hide_later();
                            } else {
                                voice_hud.show("No focused terminal target", Some(&text));
                                overlay.add_toast(adw::Toast::new(
                                    "Voice text was not inserted: no focused terminal pane",
                                ));
                            }
                        }
                        WindowsVoiceUiEvent::Partial(text) => {
                            voice_hud.show("Voice partial", Some(&text));
                        }
                        WindowsVoiceUiEvent::Status(message) => {
                            if voice_stopping.get() && message == "Listening…" {
                                continue;
                            }
                            voice_hud.show(&message, None);
                        }
                        WindowsVoiceUiEvent::Error(message) => {
                            voice_starting.set(false);
                            voice_listening.set(false);
                            voice_stopping.set(false);
                            logging::error(format!(
                                "Windows GTK voice transcription failed: {message}"
                            ));
                            voice_hud.show("Voice error", Some(&message));
                            overlay.add_toast(adw::Toast::new("Voice transcription failed"));
                        }
                        WindowsVoiceUiEvent::GlobalHotkeyActivated => {
                            handle_windows_voice_global_hotkey_activation(
                                &preference_store,
                                &shell_state,
                                &voice_hud,
                                &overlay,
                                &voice_transcriber,
                                &voice_listening,
                                &voice_starting,
                                &voice_stopping,
                                &voice_event_tx_for_handler,
                            );
                        }
                    }
                }
                gtk::glib::ControlFlow::Continue
            });
        }

        {
            let voice_transcriber = voice_transcriber.clone();
            let voice_listening = voice_listening.clone();
            let voice_event_tx = voice_event_tx.clone();
            gtk::glib::timeout_add_local(VOICE_AUDIO_FLUSH_INTERVAL, move || {
                if voice_listening.get() {
                    voice_transcriber.flush(&voice_event_tx);
                }
                gtk::glib::ControlFlow::Continue
            });
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
        tray_controller.install();

        if dialog_smoke::is_enabled() {
            dialog_smoke::start(&window);
            return;
        }

        let restore_mode = preferences.default_restore_mode;
        if let Some(session) = session_outcome
            .session
            .as_ref()
            .and_then(|session| session_for_restore_mode(session, restore_mode))
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
                    restore_mode == RestoreLaunchMode::RerunStartupCommands,
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
        apply_agent_resume_overrides: bool,
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
            apply_agent_resume_overrides,
        );
    }

    fn present_settings_dialog(context: WindowsSettingsDialogContext) {
        let WindowsSettingsDialogContext {
            window,
            overlay,
            title,
            fullscreen_button,
            shell_state,
            current_close_to_background,
            preference_store,
            preset_store,
            options,
            voice_toast_tx,
            voice_global_hotkey,
            voice_event_tx,
            workspace_fullscreen_shortcut_controller,
            workspace_density_shortcut_controller,
            workspace_zoom_in_shortcut_controller,
            workspace_zoom_out_shortcut_controller,
            workspace_tile_selection_shortcut_controller,
            command_palette_shortcut_controller,
            open_command_palette_handle,
            refresh_launch_deck_handle,
        } = context;
        let preferences = preference_store.load();
        settings_dialog::present(
            &window,
            settings_dialog::SettingsDialogInput {
                default_theme: preferences.default_theme,
                default_density: preferences.default_density,
                close_to_background: preferences.close_to_background,
                workspace_fullscreen_shortcut: preferences.workspace_fullscreen_shortcut,
                workspace_density_shortcut: preferences.workspace_density_shortcut,
                workspace_zoom_in_shortcut: preferences.workspace_zoom_in_shortcut,
                workspace_zoom_out_shortcut: preferences.workspace_zoom_out_shortcut,
                workspace_tile_selection_prefix_shortcut: preferences
                    .workspace_tile_selection_prefix_shortcut,
                command_palette_shortcut: preferences.command_palette_shortcut,
                settings_dialog_width: preferences.settings_dialog_width,
                settings_dialog_height: preferences.settings_dialog_height,
                max_reconnect_attempts: preferences.max_reconnect_attempts,
                terminal_history_lines: preferences.terminal_history_lines,
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
                    let refresh_launch_deck_handle = refresh_launch_deck_handle.clone();
                    move |theme| {
                        preference_store.save_default_theme(theme);
                        apply_theme_mode(&window, theme);
                        request_windows_launch_deck_refresh(&refresh_launch_deck_handle);
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
                    let refresh_launch_deck_handle = refresh_launch_deck_handle.clone();
                    move |density| {
                        preference_store.save_default_density(density);
                        apply_window_density(&window, density);
                        request_windows_launch_deck_refresh(&refresh_launch_deck_handle);
                        overlay.add_toast(adw::Toast::new(&format!(
                            "Default density set to {}",
                            density.label()
                        )));
                    }
                }),
                on_close_to_background_changed: Rc::new({
                    let overlay = overlay.clone();
                    let preference_store = preference_store.clone();
                    let current_close_to_background = current_close_to_background.clone();
                    move |close_to_background| {
                        preference_store.save_close_to_background(close_to_background);
                        current_close_to_background.set(close_to_background);
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
                    let window = window.clone();
                    let overlay = overlay.clone();
                    let title_root = title.root.clone();
                    let fullscreen_button = fullscreen_button.clone();
                    let shell_state = shell_state.clone();
                    let controller_handle = workspace_fullscreen_shortcut_controller.clone();
                    move |shortcut| {
                        preference_store.save_workspace_fullscreen_shortcut(&shortcut);
                        install_workspace_fullscreen_shortcut(
                            &window,
                            &controller_handle,
                            &shell_state,
                            &shortcut,
                        );
                        sync_windows_fullscreen_chrome(
                            &window,
                            title_root.upcast_ref(),
                            &fullscreen_button,
                            shell_state.preview.borrow().is_some()
                                && !shell_state.launch_deck_active.get(),
                        );
                        overlay.add_toast(adw::Toast::new(&format!(
                            "Fullscreen shortcut set to {shortcut}"
                        )));
                    }
                }),
                on_density_shortcut_changed: Rc::new({
                    let preference_store = preference_store.clone();
                    let window = window.clone();
                    let overlay = overlay.clone();
                    let shell_state = shell_state.clone();
                    let controller_handle = workspace_density_shortcut_controller.clone();
                    move |shortcut| {
                        preference_store.save_workspace_density_shortcut(&shortcut);
                        install_workspace_density_shortcut(
                            &window,
                            &controller_handle,
                            &shell_state,
                            &shortcut,
                        );
                        overlay.add_toast(adw::Toast::new(&format!(
                            "Density shortcut set to {shortcut}"
                        )));
                    }
                }),
                on_zoom_in_shortcut_changed: Rc::new({
                    let preference_store = preference_store.clone();
                    let window = window.clone();
                    let overlay = overlay.clone();
                    let shell_state = shell_state.clone();
                    let controller_handle = workspace_zoom_in_shortcut_controller.clone();
                    move |shortcut| {
                        preference_store.save_workspace_zoom_in_shortcut(&shortcut);
                        install_workspace_zoom_shortcut(
                            &window,
                            &controller_handle,
                            &shell_state,
                            &shortcut,
                            1,
                            "workspace_zoom_in",
                        );
                        overlay.add_toast(adw::Toast::new(&format!(
                            "Zoom in shortcut set to {shortcut}"
                        )));
                    }
                }),
                on_zoom_out_shortcut_changed: Rc::new({
                    let preference_store = preference_store.clone();
                    let window = window.clone();
                    let overlay = overlay.clone();
                    let shell_state = shell_state.clone();
                    let controller_handle = workspace_zoom_out_shortcut_controller.clone();
                    move |shortcut| {
                        preference_store.save_workspace_zoom_out_shortcut(&shortcut);
                        install_workspace_zoom_shortcut(
                            &window,
                            &controller_handle,
                            &shell_state,
                            &shortcut,
                            -1,
                            "workspace_zoom_out",
                        );
                        overlay.add_toast(adw::Toast::new(&format!(
                            "Zoom out shortcut set to {shortcut}"
                        )));
                    }
                }),
                on_tile_selection_prefix_shortcut_changed: Rc::new({
                    let preference_store = preference_store.clone();
                    let window = window.clone();
                    let overlay = overlay.clone();
                    let shell_state = shell_state.clone();
                    let controller_handle = workspace_tile_selection_shortcut_controller.clone();
                    move |shortcut| {
                        preference_store.save_workspace_tile_selection_prefix_shortcut(&shortcut);
                        install_workspace_tile_selection_shortcut(
                            &window,
                            &controller_handle,
                            &shell_state,
                            &shortcut,
                        );
                        overlay.add_toast(adw::Toast::new(&format!(
                            "Tile selection shortcut set to {shortcut}"
                        )));
                    }
                }),
                on_command_palette_shortcut_changed: Rc::new({
                    let preference_store = preference_store.clone();
                    let window = window.clone();
                    let overlay = overlay.clone();
                    let command_palette_shortcut_controller =
                        command_palette_shortcut_controller.clone();
                    let open_command_palette_handle = open_command_palette_handle.clone();
                    move |shortcut| {
                        preference_store.save_command_palette_shortcut(&shortcut);
                        if let Some(open_command_palette) =
                            open_command_palette_handle.borrow().as_ref().cloned()
                        {
                            install_command_palette_shortcut(
                                &window,
                                &command_palette_shortcut_controller,
                                &shortcut,
                                open_command_palette,
                            );
                        }
                        overlay.add_toast(adw::Toast::new(&format!(
                            "Command palette shortcut set to {shortcut}"
                        )));
                    }
                }),
                on_max_reconnect_attempts_changed: Rc::new({
                    let preference_store = preference_store.clone();
                    move |attempts| preference_store.save_max_reconnect_attempts(attempts)
                }),
                on_terminal_history_lines_changed: Rc::new({
                    let preference_store = preference_store.clone();
                    move |lines| preference_store.save_terminal_history_lines(lines)
                }),
                on_voice_preferences_changed: Rc::new({
                    let preference_store = preference_store.clone();
                    let voice_global_hotkey = voice_global_hotkey.clone();
                    let voice_event_tx = voice_event_tx.clone();
                    move |voice| {
                        preference_store.save_voice_preferences(voice.clone());
                        sync_windows_voice_global_hotkey(
                            &voice_global_hotkey,
                            &voice,
                            &voice_event_tx,
                        );
                    }
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
                    let title_root = title.root.clone();
                    let fullscreen_button = fullscreen_button.clone();
                    let shell_state = shell_state.clone();
                    let preference_store = preference_store.clone();
                    let workspace_fullscreen_shortcut_controller =
                        workspace_fullscreen_shortcut_controller.clone();
                    let workspace_density_shortcut_controller =
                        workspace_density_shortcut_controller.clone();
                    let workspace_zoom_in_shortcut_controller =
                        workspace_zoom_in_shortcut_controller.clone();
                    let workspace_zoom_out_shortcut_controller =
                        workspace_zoom_out_shortcut_controller.clone();
                    let workspace_tile_selection_shortcut_controller =
                        workspace_tile_selection_shortcut_controller.clone();
                    let command_palette_shortcut_controller =
                        command_palette_shortcut_controller.clone();
                    let open_command_palette_handle = open_command_palette_handle.clone();
                    let refresh_launch_deck_handle = refresh_launch_deck_handle.clone();
                    let current_close_to_background = current_close_to_background.clone();
                    move || {
                        let defaults = AppPreferences::default();
                        preference_store.save(&defaults);
                        current_close_to_background.set(defaults.close_to_background);
                        apply_theme_mode(&window, defaults.default_theme);
                        apply_window_density(&window, defaults.default_density);
                        install_workspace_fullscreen_shortcut(
                            &window,
                            &workspace_fullscreen_shortcut_controller,
                            &shell_state,
                            &defaults.workspace_fullscreen_shortcut,
                        );
                        install_workspace_density_shortcut(
                            &window,
                            &workspace_density_shortcut_controller,
                            &shell_state,
                            &defaults.workspace_density_shortcut,
                        );
                        install_workspace_zoom_shortcut(
                            &window,
                            &workspace_zoom_in_shortcut_controller,
                            &shell_state,
                            &defaults.workspace_zoom_in_shortcut,
                            1,
                            "workspace_zoom_in",
                        );
                        install_workspace_zoom_shortcut(
                            &window,
                            &workspace_zoom_out_shortcut_controller,
                            &shell_state,
                            &defaults.workspace_zoom_out_shortcut,
                            -1,
                            "workspace_zoom_out",
                        );
                        install_workspace_tile_selection_shortcut(
                            &window,
                            &workspace_tile_selection_shortcut_controller,
                            &shell_state,
                            &defaults.workspace_tile_selection_prefix_shortcut,
                        );
                        sync_windows_fullscreen_chrome(
                            &window,
                            title_root.upcast_ref(),
                            &fullscreen_button,
                            shell_state.preview.borrow().is_some()
                                && !shell_state.launch_deck_active.get(),
                        );
                        if let Some(open_command_palette) =
                            open_command_palette_handle.borrow().as_ref().cloned()
                        {
                            install_command_palette_shortcut(
                                &window,
                                &command_palette_shortcut_controller,
                                &defaults.command_palette_shortcut,
                                open_command_palette,
                            );
                        }
                        request_windows_launch_deck_refresh(&refresh_launch_deck_handle);
                        overlay.add_toast(adw::Toast::new("Application defaults reset"));
                    }
                }),
                on_reset_builtin_presets: Rc::new({
                    let overlay = overlay.clone();
                    let refresh_launch_deck_handle = refresh_launch_deck_handle.clone();
                    move || match preset_store.reset_builtin_presets() {
                        Ok(()) => {
                            logging::info("reset builtin saved presets to factory defaults");
                            request_windows_launch_deck_refresh(&refresh_launch_deck_handle);
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

    fn sync_windows_voice_global_hotkey(
        registration: &Rc<RefCell<Option<WindowsVoiceGlobalHotkeyRegistration>>>,
        voice: &crate::voice::VoicePreferences,
        voice_event_tx: &mpsc::Sender<WindowsVoiceUiEvent>,
    ) {
        if !should_register_windows_voice_global_hotkey(voice) {
            registration.borrow_mut().take();
            return;
        }

        {
            let current = registration.borrow();
            if let Some(current) = current.as_ref()
                && current.shortcut() == voice.hotkey
            {
                if current.unavailable_retry_pending() {
                    return;
                }
                if matches!(current, WindowsVoiceGlobalHotkeyRegistration::Active { .. }) {
                    return;
                }
            }
        }

        registration.borrow_mut().take();
        let (global_tx, global_rx) = mpsc::channel::<WindowsGlobalHotkeyEvent>();
        match WindowsGlobalHotkeyHandle::start(voice.hotkey.clone(), global_tx) {
            Ok(handle) => {
                let shortcut = voice.hotkey.clone();
                logging::info(format!(
                    "registered Windows GTK global voice hotkey {shortcut}"
                ));
                *registration.borrow_mut() =
                    Some(WindowsVoiceGlobalHotkeyRegistration::Active { shortcut, handle });
                let ui_tx = voice_event_tx.clone();
                std::thread::spawn(move || {
                    while let Ok(event) = global_rx.recv() {
                        logging::info(format!("Windows GTK global voice hotkey event={event:?}"));
                        let _ = match event {
                            WindowsGlobalHotkeyEvent::Activated => {
                                ui_tx.send(WindowsVoiceUiEvent::GlobalHotkeyActivated)
                            }
                        };
                    }
                });
            }
            Err(error) => {
                logging::error(format!(
                    "Windows GTK global voice hotkey unavailable for '{}': {error}",
                    voice.hotkey
                ));
                *registration.borrow_mut() =
                    Some(WindowsVoiceGlobalHotkeyRegistration::Unavailable {
                        shortcut: voice.hotkey.clone(),
                        last_attempt: Instant::now(),
                    });
            }
        }
    }

    fn should_register_windows_voice_global_hotkey(voice: &crate::voice::VoicePreferences) -> bool {
        voice.enabled
            && (voice.prefer_global_hotkey
                || voice.activation_mode == VoiceActivationMode::PushToTalk)
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_windows_voice_global_hotkey_activation(
        preference_store: &PreferenceStore,
        shell_state: &WindowsGtkShellState,
        voice_hud: &VoiceHud,
        overlay: &adw::ToastOverlay,
        voice_transcriber: &Rc<WindowsVoiceTranscriberHandle>,
        voice_listening: &Rc<Cell<bool>>,
        voice_starting: &Rc<Cell<bool>>,
        voice_stopping: &Rc<Cell<bool>>,
        voice_event_tx: &mpsc::Sender<WindowsVoiceUiEvent>,
    ) {
        let voice = preference_store.load().voice;
        logging::info(format!(
            "Windows GTK global voice hotkey activated enabled={} mode={} listening={} starting={} stopping={}",
            voice.enabled,
            voice.activation_mode.label(),
            voice_listening.get(),
            voice_starting.get(),
            voice_stopping.get(),
        ));
        if !voice.enabled {
            return;
        }

        if voice_listening.get() {
            stop_windows_voice_capture(
                voice_transcriber,
                voice_listening,
                voice_stopping,
                voice_hud,
                voice_event_tx,
            );
            return;
        }

        if voice_starting.get() || voice_stopping.get() {
            return;
        }

        start_windows_voice_capture(
            preference_store,
            shell_state,
            voice_hud,
            overlay,
            voice_transcriber,
            voice_listening,
            voice_starting,
            voice_stopping,
            voice_event_tx,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn install_windows_voice_hotkey_controller(
        window: &adw::ApplicationWindow,
        controller_handle: &VoiceKeyControllerHandle,
        preference_store: PreferenceStore,
        shell_state: WindowsGtkShellState,
        voice_hud: VoiceHud,
        overlay: adw::ToastOverlay,
        voice_transcriber: Rc<WindowsVoiceTranscriberHandle>,
        voice_listening: Rc<Cell<bool>>,
        voice_starting: Rc<Cell<bool>>,
        voice_stopping: Rc<Cell<bool>>,
        voice_local_key_pressed: Rc<Cell<bool>>,
        voice_event_tx: mpsc::Sender<WindowsVoiceUiEvent>,
    ) {
        if let Some(existing) = controller_handle.borrow_mut().take() {
            window.remove_controller(&existing);
        }

        let controller = gtk::EventControllerKey::new();
        controller.set_propagation_phase(gtk::PropagationPhase::Capture);

        {
            let preference_store = preference_store.clone();
            let shell_state = shell_state.clone();
            let voice_hud = voice_hud.clone();
            let overlay = overlay.clone();
            let voice_transcriber = voice_transcriber.clone();
            let voice_listening = voice_listening.clone();
            let voice_starting = voice_starting.clone();
            let voice_stopping = voice_stopping.clone();
            let voice_local_key_pressed = voice_local_key_pressed.clone();
            let voice_event_tx = voice_event_tx.clone();
            controller.connect_key_pressed(move |_, key, _, state| {
                let preferences = preference_store.load();
                let voice = preferences.voice.clone();
                if !windows_voice_key_event_matches(&voice.hotkey, key, state) {
                    return glib::Propagation::Proceed;
                }
                if voice_local_key_pressed.replace(true) {
                    logging::info("Windows GTK voice hotkey press ignored: repeat");
                    return glib::Propagation::Stop;
                }
                logging::info("Windows GTK voice hotkey press matched");

                if !voice.enabled {
                    voice_hud.show("Voice disabled", None);
                    overlay.add_toast(adw::Toast::new("Enable voice input in Settings first"));
                    return glib::Propagation::Stop;
                }

                match voice.activation_mode {
                    VoiceActivationMode::Toggle if voice_listening.get() => {
                        stop_windows_voice_capture(
                            &voice_transcriber,
                            &voice_listening,
                            &voice_stopping,
                            &voice_hud,
                            &voice_event_tx,
                        );
                    }
                    VoiceActivationMode::Toggle | VoiceActivationMode::PushToTalk => {
                        if !voice_listening.get() && !voice_starting.get() && !voice_stopping.get()
                        {
                            start_windows_voice_capture(
                                &preference_store,
                                &shell_state,
                                &voice_hud,
                                &overlay,
                                &voice_transcriber,
                                &voice_listening,
                                &voice_starting,
                                &voice_stopping,
                                &voice_event_tx,
                            );
                        }
                    }
                }

                glib::Propagation::Stop
            });
        }

        {
            let preference_store = preference_store.clone();
            let voice_hud = voice_hud.clone();
            let voice_transcriber = voice_transcriber.clone();
            let voice_listening = voice_listening.clone();
            let voice_starting = voice_starting.clone();
            let voice_stopping = voice_stopping.clone();
            let voice_local_key_pressed = voice_local_key_pressed.clone();
            let voice_event_tx = voice_event_tx.clone();
            controller.connect_key_released(move |_, key, _, _state| {
                let preferences = preference_store.load();
                let voice = preferences.voice.clone();
                if !windows_voice_key_matches_accelerator_key(&voice.hotkey, key) {
                    return;
                }
                voice_local_key_pressed.set(false);
                if voice.activation_mode != VoiceActivationMode::PushToTalk {
                    return;
                }
                logging::info("Windows GTK voice hotkey release matched");
                if voice_starting.replace(false) && !voice_listening.get() {
                    finish_pending_windows_voice_capture(
                        &voice_transcriber,
                        &voice_stopping,
                        &voice_hud,
                        &voice_event_tx,
                    );
                } else {
                    stop_windows_voice_capture(
                        &voice_transcriber,
                        &voice_listening,
                        &voice_stopping,
                        &voice_hud,
                        &voice_event_tx,
                    );
                }
            });
        }

        window.add_controller(controller.clone());
        *controller_handle.borrow_mut() = Some(controller);
    }

    #[allow(clippy::too_many_arguments)]
    fn start_windows_voice_capture(
        preference_store: &PreferenceStore,
        shell_state: &WindowsGtkShellState,
        voice_hud: &VoiceHud,
        overlay: &adw::ToastOverlay,
        voice_transcriber: &Rc<WindowsVoiceTranscriberHandle>,
        voice_listening: &Rc<Cell<bool>>,
        voice_starting: &Rc<Cell<bool>>,
        voice_stopping: &Rc<Cell<bool>>,
        voice_event_tx: &mpsc::Sender<WindowsVoiceUiEvent>,
    ) {
        let preferences = preference_store.load();
        let voice = preferences.voice.clone();
        logging::info(format!(
            "Windows GTK voice capture start requested enabled={} mode={} listening={} starting={} stopping={}",
            voice.enabled,
            voice.activation_mode.label(),
            voice_listening.get(),
            voice_starting.get(),
            voice_stopping.get(),
        ));
        let preview = shell_state.voice_target();
        let Some(preview) = preview else {
            voice_hud.show("No workspace target", None);
            overlay.add_toast(adw::Toast::new(
                "Open a workspace and focus a terminal pane before dictating",
            ));
            return;
        };
        if !preview.focused_terminal_available() {
            voice_hud.show("No focused terminal target", None);
            overlay.add_toast(adw::Toast::new("Focus a terminal pane before dictating"));
            return;
        }

        let manifest = pack::builtin_parakeet_manifest();
        let Some(root) = pack::default_voice_pack_dir() else {
            voice_hud.show(
                "Voice pack error",
                Some("Could not resolve app data directory"),
            );
            return;
        };
        if let Err(detail) = refresh_builtin_voice_pack_assets_for_runtime(&root) {
            logging::error(format!(
                "Windows GTK voice capture blocked: could not refresh bundled voice pack assets: {detail}"
            ));
            voice_hud.show("Voice pack refresh failed", Some(&detail));
            overlay.add_toast(adw::Toast::new("Voice pack refresh failed"));
            return;
        }
        let health = pack::health_check(&root, &manifest);
        if !matches!(health, VoicePackHealth::Ready { .. }) {
            voice_hud.show(
                "Voice pack not installed",
                Some("Install NVIDIA Parakeet from Settings"),
            );
            overlay.add_toast(adw::Toast::new(
                "Install the NVIDIA Parakeet voice pack in Settings first",
            ));
            return;
        }

        voice_listening.set(false);
        voice_starting.set(true);
        voice_stopping.set(false);
        voice_hud.show("Starting voice capture…", Some("Preparing microphone"));
        voice_transcriber.start_capture(
            manifest,
            health,
            voice.engine_mode,
            voice.microphone_id,
            voice_event_tx,
        );
    }

    fn stop_windows_voice_capture(
        voice_transcriber: &Rc<WindowsVoiceTranscriberHandle>,
        voice_listening: &Rc<Cell<bool>>,
        voice_stopping: &Rc<Cell<bool>>,
        voice_hud: &VoiceHud,
        voice_event_tx: &mpsc::Sender<WindowsVoiceUiEvent>,
    ) {
        if !voice_listening.replace(false) {
            logging::info("Windows GTK voice capture stop ignored: not listening");
            return;
        }
        voice_stopping.set(true);
        voice_hud.show("Finalizing voice text…", None);
        voice_transcriber.stop(voice_event_tx);
    }

    fn finish_pending_windows_voice_capture(
        voice_transcriber: &Rc<WindowsVoiceTranscriberHandle>,
        voice_stopping: &Rc<Cell<bool>>,
        voice_hud: &VoiceHud,
        voice_event_tx: &mpsc::Sender<WindowsVoiceUiEvent>,
    ) {
        voice_stopping.set(true);
        voice_hud.show("Finalizing voice text…", None);
        voice_transcriber.stop(voice_event_tx);
    }

    fn windows_voice_key_event_matches(
        accelerator: &str,
        key: gdk::Key,
        state: gdk::ModifierType,
    ) -> bool {
        let Some((expected_key, expected_modifiers)) = gtk::accelerator_parse(accelerator) else {
            return false;
        };
        let event_modifiers = state & gtk::accelerator_get_default_mod_mask();
        key == expected_key && event_modifiers == expected_modifiers
    }

    fn windows_voice_key_matches_accelerator_key(accelerator: &str, key: gdk::Key) -> bool {
        let Some((expected_key, _)) = gtk::accelerator_parse(accelerator) else {
            return false;
        };
        key == expected_key
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
                        let message = error.user_message();
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
                let message = error.user_message();
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
            Ok(path) => match open_path_with_shell(std::ptr::null_mut(), &path) {
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
            },
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

    fn build_windows_launch_deck(
        context: &WindowsLaunchDeckContext,
        refresh_launch_deck_handle: &WeakVoidCallbackHandle,
        load_warning: Option<String>,
        presets: Vec<crate::model::preset::WorkspacePreset>,
        assets: crate::model::assets::WorkspaceAssets,
        preferences: &AppPreferences,
    ) -> gtk::Widget {
        let actions = LaunchScreenActions {
            on_theme_preview: Rc::new({
                let window = context.window.clone();
                move |theme| apply_theme_mode(&window, theme)
            }),
            on_density_preview: Rc::new({
                let window = context.window.clone();
                move |density| apply_window_density(&window, density)
            }),
            on_launch: Rc::new({
                let context = context.clone();
                let preferences = preferences.clone();
                let assets = assets.clone();
                move |preset, workspace_root| {
                    if let Some(launch_widget) =
                        context.launch_widget_handle.borrow().as_ref().cloned()
                    {
                        present_workspace_preview_from_launch(
                            &context.window,
                            &context.overlay,
                            &context.title,
                            &preferences,
                            &context.back_button,
                            &context.fullscreen_button,
                            &context.shell_state,
                            &launch_widget,
                            assets.clone(),
                            preset,
                            workspace_root,
                        );
                    }
                }
            }),
            on_launch_board: None,
            on_cancel: Rc::new({
                let context = context.clone();
                move || {
                    if context.shell_state.has_workspace_tabs()
                        && let Some(launch_widget) =
                            context.launch_widget_handle.borrow().as_ref().cloned()
                    {
                        let active_index = context
                            .shell_state
                            .preview
                            .borrow()
                            .as_ref()
                            .map(|preview| preview.active_index())
                            .unwrap_or(0);
                        show_workspace_preview_tab(
                            &context.window,
                            &context.overlay,
                            &context.title,
                            &launch_widget,
                            &context.back_button,
                            &context.fullscreen_button,
                            &context.shell_state,
                            active_index,
                        );
                    } else {
                        context.app.quit();
                    }
                }
            }),
            on_presets_changed: Rc::new({
                let refresh_launch_deck_handle = refresh_launch_deck_handle.clone();
                move || request_windows_launch_deck_refresh(&refresh_launch_deck_handle)
            }),
        };

        crate::ui::launch_screen::build(
            LaunchScreenInput {
                load_warning,
                presets,
                board_workspaces: None,
                assets,
                default_theme: preferences.default_theme,
                default_density: preferences.default_density,
                default_restore_mode: preferences.default_restore_mode,
                preset_store: context.preset_store.clone(),
                board_workspace_store: None,
            },
            actions,
        )
    }

    fn refresh_windows_launch_deck(
        context: &WindowsLaunchDeckContext,
        refresh_launch_deck_handle: &WeakVoidCallbackHandle,
    ) {
        let preset_outcome = context.preset_store.load_presets_with_status();
        let asset_outcome = context.asset_store.load_assets_with_status();
        let preferences = context.preference_store.load();
        let launch = build_windows_launch_deck(
            context,
            refresh_launch_deck_handle,
            combine_warnings(preset_outcome.warning, asset_outcome.warning),
            preset_outcome.presets,
            asset_outcome.assets,
            &preferences,
        );
        *context.launch_widget_handle.borrow_mut() = Some(launch.clone());
        if context.shell_state.launch_deck_active.get() {
            show_launch_deck_tab(
                &context.window,
                &context.overlay,
                &context.title,
                &launch,
                &context.back_button,
                &context.fullscreen_button,
                &context.shell_state,
            );
        }
        logging::info("Windows GTK shell refreshed launch deck after preset/default change");
    }

    fn request_windows_launch_deck_refresh(refresh_launch_deck_handle: &WeakVoidCallbackHandle) {
        if let Some(refresh_launch_deck_handle) = refresh_launch_deck_handle.upgrade() {
            let refresh = refresh_launch_deck_handle.borrow().as_ref().cloned();
            if let Some(refresh) = refresh {
                glib::idle_add_local_once(move || refresh());
            }
        }
    }

    fn shutdown_windows_gtk_shell(shell_state: &WindowsGtkShellState, reason: &str) {
        if shell_state.has_active_processes() {
            logging::info(format!(
                "Windows GTK shell closing with active terminal runtimes; terminating preview runtimes: {reason}",
            ));
        }
        shell_state.save_preview_session_capturing_history(reason);
        shell_state.terminate_preview_runtimes(reason);
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

    fn apply_launch_deck_profile(window: &adw::ApplicationWindow) {
        let preferences = PreferenceStore::new().load();
        apply_theme_mode(window, preferences.default_theme);
        apply_window_density(window, preferences.default_density);
    }

    fn apply_active_preview_profile(
        window: &adw::ApplicationWindow,
        preview: &crate::ui::workspace_preview::SessionPreview,
    ) {
        let session = preview.snapshot();
        let Some(tab) = session.tabs.get(preview.active_index()) else {
            return;
        };
        apply_theme_mode(window, tab.preset.theme);
        apply_window_density(window, tab.preset.density);
    }

    #[derive(Clone)]
    struct WindowsGtkShellState {
        preview: Rc<RefCell<Option<crate::ui::workspace_preview::SessionPreview>>>,
        detached_previews: Rc<RefCell<Vec<crate::ui::workspace_preview::SessionPreview>>>,
        active_voice_target: Rc<RefCell<Option<crate::ui::workspace_preview::SessionPreview>>>,
        voice_runtime: Rc<RefCell<Option<WindowsVoiceRuntime>>>,
        launch_deck_active: Rc<Cell<bool>>,
        session_store: Rc<SessionStore>,
        product: ProductIdentity,
    }

    #[derive(Clone)]
    struct WindowsVoiceRuntime {
        preference_store: PreferenceStore,
        voice_transcriber: Rc<WindowsVoiceTranscriberHandle>,
        voice_listening: Rc<Cell<bool>>,
        voice_starting: Rc<Cell<bool>>,
        voice_stopping: Rc<Cell<bool>>,
    }

    impl WindowsGtkShellState {
        fn new(session_store: SessionStore, product: ProductIdentity) -> Self {
            Self {
                preview: Rc::new(RefCell::new(None)),
                detached_previews: Rc::new(RefCell::new(Vec::new())),
                active_voice_target: Rc::new(RefCell::new(None)),
                voice_runtime: Rc::new(RefCell::new(None)),
                launch_deck_active: Rc::new(Cell::new(false)),
                session_store: Rc::new(session_store),
                product,
            }
        }

        fn voice_target(&self) -> Option<crate::ui::workspace_preview::SessionPreview> {
            if let Some(target) = self.active_voice_target.borrow().clone() {
                let is_main_preview = self
                    .preview
                    .borrow()
                    .as_ref()
                    .is_some_and(|preview| session_preview_widgets_match(preview, &target));
                if !is_main_preview || !self.launch_deck_active.get() {
                    return Some(target);
                }
            }
            if self.launch_deck_active.get() {
                None
            } else {
                self.preview.borrow().clone()
            }
        }

        fn set_voice_target(&self, preview: &crate::ui::workspace_preview::SessionPreview) {
            *self.active_voice_target.borrow_mut() = Some(preview.clone());
        }

        fn set_main_voice_target(&self) {
            *self.active_voice_target.borrow_mut() = if self.launch_deck_active.get() {
                None
            } else {
                self.preview.borrow().clone()
            };
        }

        fn clear_voice_target_if(&self, preview: &crate::ui::workspace_preview::SessionPreview) {
            let should_clear = self
                .active_voice_target
                .borrow()
                .as_ref()
                .is_some_and(|target| session_preview_widgets_match(target, preview));
            if should_clear {
                self.set_main_voice_target();
            }
        }

        fn set_voice_runtime(&self, runtime: WindowsVoiceRuntime) {
            *self.voice_runtime.borrow_mut() = Some(runtime);
        }

        fn voice_runtime(&self) -> Option<WindowsVoiceRuntime> {
            self.voice_runtime.borrow().clone()
        }

        fn register_detached_preview(
            &self,
            preview: &crate::ui::workspace_preview::SessionPreview,
        ) {
            {
                let mut detached_previews = self.detached_previews.borrow_mut();
                if detached_previews
                    .iter()
                    .any(|detached| session_preview_widgets_match(detached, preview))
                {
                    return;
                }
                detached_previews.push(preview.clone());
            }
            self.save_preview_session("Windows GTK detached workspace registered");
        }

        fn unregister_detached_preview(
            &self,
            preview: &crate::ui::workspace_preview::SessionPreview,
        ) {
            self.detached_previews
                .borrow_mut()
                .retain(|detached| !session_preview_widgets_match(detached, preview));
            self.clear_voice_target_if(preview);
            self.save_preview_session("Windows GTK detached workspace unregistered");
        }

        fn has_workspace_tabs(&self) -> bool {
            self.preview
                .borrow()
                .as_ref()
                .is_some_and(|preview| !preview.snapshot().tabs.is_empty())
        }

        fn active_project_root(&self) -> Option<PathBuf> {
            self.preview.borrow().as_ref().and_then(|preview| {
                let session = preview.snapshot();
                session
                    .tabs
                    .get(preview.active_index())
                    .map(|tab| tab.workspace_root.clone())
            })
        }

        fn open_project_roots(&self) -> Vec<PathBuf> {
            let mut roots = Vec::<PathBuf>::new();
            for preview in self.all_previews() {
                roots.extend(
                    preview
                        .snapshot()
                        .tabs
                        .into_iter()
                        .map(|tab| tab.workspace_root),
                );
            }
            roots
        }

        fn save_preview_session(&self, reason: &str) {
            if let Some(session) = self.combined_session_snapshot() {
                persist_windows_gtk_session(&self.session_store, &session, reason);
            } else {
                logging::info(format!(
                    "clearing Windows GTK saved session state reason='{reason}'"
                ));
                self.session_store.clear();
            }
        }

        fn save_preview_session_capturing_history(&self, reason: &str) {
            let line_limit = PreferenceStore::new().load().terminal_history_lines as usize;
            if let Some(session) = self.combined_session_snapshot_with_history(line_limit) {
                persist_windows_gtk_session(&self.session_store, &session, reason);
            } else {
                logging::info(format!(
                    "clearing Windows GTK saved session state reason='{reason}'"
                ));
                self.session_store.clear();
            }
        }

        fn terminate_preview_runtimes(&self, reason: &str) {
            for preview in self.all_previews() {
                preview.terminate_all(reason);
            }
        }

        fn has_active_processes(&self) -> bool {
            self.all_previews()
                .iter()
                .any(|preview| preview.has_active_processes())
        }

        fn all_previews(&self) -> Vec<crate::ui::workspace_preview::SessionPreview> {
            let mut previews = Vec::new();
            if let Some(preview) = self.preview.borrow().as_ref().cloned() {
                previews.push(preview);
            }
            previews.extend(self.detached_previews.borrow().iter().cloned());
            previews
        }

        fn combined_session_snapshot(&self) -> Option<SavedSession> {
            self.combined_session_snapshot_with(|preview| preview.snapshot())
        }

        fn combined_session_snapshot_with_history(
            &self,
            line_limit: usize,
        ) -> Option<SavedSession> {
            self.combined_session_snapshot_with(|preview| {
                preview.snapshot_with_terminal_history(line_limit)
            })
        }

        fn combined_session_snapshot_with(
            &self,
            snapshot: impl Fn(&crate::ui::workspace_preview::SessionPreview) -> SavedSession,
        ) -> Option<SavedSession> {
            let mut tabs = Vec::new();
            let mut active_tab_index = 0;
            if let Some(preview) = self.preview.borrow().as_ref() {
                let preview_snapshot = snapshot(preview);
                active_tab_index = preview_snapshot
                    .active_tab_index
                    .min(preview_snapshot.tabs.len().saturating_sub(1));
                tabs.extend(preview_snapshot.tabs);
            }
            for detached_preview in self.detached_previews.borrow().iter() {
                tabs.extend(snapshot(detached_preview).tabs);
            }
            if tabs.is_empty() {
                None
            } else {
                Some(SavedSession {
                    tabs,
                    active_tab_index,
                })
            }
        }
    }

    fn session_preview_widgets_match(
        first: &crate::ui::workspace_preview::SessionPreview,
        second: &crate::ui::workspace_preview::SessionPreview,
    ) -> bool {
        first.widget().as_ptr() == second.widget().as_ptr()
    }

    fn persist_windows_gtk_session(
        session_store: &SessionStore,
        session: &SavedSession,
        reason: &str,
    ) {
        if session.tabs.is_empty() {
            logging::info(format!(
                "clearing Windows GTK saved session state reason='{reason}'"
            ));
            session_store.clear();
        } else {
            logging::info(format!(
                "saving Windows GTK session state reason='{reason}' tabs={}",
                session.tabs.len()
            ));
            session_store.save(session);
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
        shell_state.set_main_voice_target();
        apply_launch_deck_profile(window);
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
        shell_state.set_voice_target(&preview);
        apply_active_preview_profile(window, &preview);
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
            reorder_index: None,
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
            on_rename: None,
            on_close: None,
            on_reorder: None,
            on_detach: None,
        });

        if let Some(preview) = shell_state.preview.borrow().as_ref() {
            let session = preview.snapshot();
            let active_index = preview.active_index();
            let can_detach = session.tabs.len() > 1;
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
                    reorder_index: Some(index),
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
                    on_rename: Some(Rc::new({
                        let window = window.clone();
                        let overlay = overlay.clone();
                        let title = title.clone();
                        let launch = launch.clone();
                        let back_button = back_button.clone();
                        let fullscreen_button = fullscreen_button.clone();
                        let shell_state = shell_state.clone();
                        move || {
                            present_windows_tab_rename(
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
                            close_windows_preview_tab(
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
                    on_reorder: Some(Rc::new({
                        let window = window.clone();
                        let overlay = overlay.clone();
                        let title = title.clone();
                        let launch = launch.clone();
                        let back_button = back_button.clone();
                        let fullscreen_button = fullscreen_button.clone();
                        let shell_state = shell_state.clone();
                        move |from_index, position| {
                            reorder_windows_preview_tab(
                                &window,
                                &overlay,
                                &title,
                                &launch,
                                &back_button,
                                &fullscreen_button,
                                &shell_state,
                                from_index,
                                position,
                            );
                        }
                    })),
                    on_detach: if can_detach {
                        Some(Rc::new({
                            let window = window.clone();
                            let overlay = overlay.clone();
                            let title = title.clone();
                            let launch = launch.clone();
                            let back_button = back_button.clone();
                            let fullscreen_button = fullscreen_button.clone();
                            let shell_state = shell_state.clone();
                            move || {
                                detach_windows_preview_tab(
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
                        }) as Rc<dyn Fn()>)
                    } else {
                        None
                    },
                });
            }
        }

        sync_windows_title_tabs(title, tabs);
    }

    #[allow(clippy::too_many_arguments)]
    fn close_windows_preview_tab(
        window: &adw::ApplicationWindow,
        overlay: &adw::ToastOverlay,
        title: &TitleChrome,
        launch: &gtk::Widget,
        back_button: &gtk::Button,
        fullscreen_button: &gtk::Button,
        shell_state: &WindowsGtkShellState,
        index: usize,
    ) {
        let Some(preview) = shell_state.preview.borrow().clone() else {
            return;
        };

        if preview.tab_has_active_processes(index) {
            let confirm_window = window.clone();
            let window = confirm_window.clone();
            let overlay = overlay.clone();
            let title = title.clone();
            let launch = launch.clone();
            let back_button = back_button.clone();
            let fullscreen_button = fullscreen_button.clone();
            let shell_state = shell_state.clone();
            dialog_chrome::confirm_destructive_action(
                &confirm_window,
                "Close Workspace?",
                "Running terminal sessions in this workspace will be terminated.",
                "Close",
                move || {
                    close_windows_preview_tab_now(
                        &window,
                        &overlay,
                        &title,
                        &launch,
                        &back_button,
                        &fullscreen_button,
                        &shell_state,
                        index,
                    );
                },
            );
            return;
        }

        close_windows_preview_tab_now(
            window,
            overlay,
            title,
            launch,
            back_button,
            fullscreen_button,
            shell_state,
            index,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn reorder_windows_preview_tab(
        window: &adw::ApplicationWindow,
        overlay: &adw::ToastOverlay,
        title: &TitleChrome,
        launch: &gtk::Widget,
        back_button: &gtk::Button,
        fullscreen_button: &gtk::Button,
        shell_state: &WindowsGtkShellState,
        from_index: usize,
        position: usize,
    ) {
        let Some(preview) = shell_state.preview.borrow().clone() else {
            return;
        };
        if !preview.move_tab(from_index, position) {
            return;
        }

        logging::info(format!(
            "Windows GTK shell reordered workspace tab from {from_index} to position {position}"
        ));

        if shell_state.launch_deck_active.get() {
            sync_windows_shell_title_tabs(
                window,
                overlay,
                title,
                launch,
                back_button,
                fullscreen_button,
                shell_state,
            );
        } else {
            show_workspace_preview_tab(
                window,
                overlay,
                title,
                launch,
                back_button,
                fullscreen_button,
                shell_state,
                preview.active_index(),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn detach_windows_preview_tab(
        window: &adw::ApplicationWindow,
        overlay: &adw::ToastOverlay,
        title: &TitleChrome,
        launch: &gtk::Widget,
        back_button: &gtk::Button,
        fullscreen_button: &gtk::Button,
        shell_state: &WindowsGtkShellState,
        index: usize,
    ) {
        let Some(app) = window
            .application()
            .and_then(|app| app.downcast::<adw::Application>().ok())
        else {
            return;
        };
        let Some(preview) = shell_state.preview.borrow().clone() else {
            return;
        };
        let Some(detached_preview) = preview.detach_tab_as_preview(index, None) else {
            return;
        };
        let detached_title = detached_preview
            .tab_title(0)
            .unwrap_or_else(|| "Detached Workspace".into());

        logging::info(format!(
            "Windows GTK shell detached workspace tab {index} to a new window",
        ));
        overlay.add_toast(adw::Toast::new("Workspace detached to a new window"));

        if shell_state.launch_deck_active.get() {
            sync_windows_shell_title_tabs(
                window,
                overlay,
                title,
                launch,
                back_button,
                fullscreen_button,
                shell_state,
            );
        } else {
            show_workspace_preview_tab(
                window,
                overlay,
                title,
                launch,
                back_button,
                fullscreen_button,
                shell_state,
                preview.active_index(),
            );
        }

        present_detached_windows_preview_window(
            &app,
            window,
            overlay,
            title,
            launch,
            back_button,
            fullscreen_button,
            shell_state,
            detached_preview,
            &detached_title,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn present_detached_windows_preview_window(
        app: &adw::Application,
        origin_window: &adw::ApplicationWindow,
        origin_overlay: &adw::ToastOverlay,
        origin_title: &TitleChrome,
        launch: &gtk::Widget,
        back_button: &gtk::Button,
        fullscreen_button: &gtk::Button,
        shell_state: &WindowsGtkShellState,
        detached_preview: crate::ui::workspace_preview::SessionPreview,
        detached_title: &str,
    ) {
        let header = adw::HeaderBar::new();
        header.set_show_start_title_buttons(true);
        header.set_show_end_title_buttons(true);
        apply_app_headerbar_class(&header);

        let title_label = gtk::Label::builder()
            .label(detached_title)
            .single_line_mode(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let title_shell = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(0)
            .build();
        title_shell.append(&title_label);
        header.set_title_widget(Some(&title_shell));

        let detached_fullscreen_button = crate::ui::icons::labeled_button(
            "Fullscreen",
            crate::ui::icons::name::FULLSCREEN,
            &["flat", "titlebar-action-button"],
        );
        header.pack_end(&detached_fullscreen_button);
        let reattach_button = crate::ui::icons::labeled_button(
            "Reattach",
            crate::ui::icons::name::RESTORE,
            &["flat", "titlebar-action-button"],
        );
        reattach_button.set_tooltip_text(Some("Reattach workspace to the main tab strip"));
        header.pack_end(&reattach_button);

        let detached_voice_hud = VoiceHud::new();
        let detached_workspace_overlay = gtk::Overlay::new();
        detached_workspace_overlay.set_child(Some(&detached_preview.widget()));
        detached_workspace_overlay.add_overlay(&detached_voice_hud.widget());

        let detached_overlay = adw::ToastOverlay::new();
        detached_overlay.set_child(Some(&detached_workspace_overlay));

        let detached_window_shell = build_window_shell();
        detached_window_shell.append(&header);
        detached_window_shell.append(&detached_overlay);

        let detached_window = adw::ApplicationWindow::builder()
            .application(app)
            .title(detached_title)
            .icon_name(&shell_state.product.icon_name)
            .default_width(crate::gtk_shell::DEFAULT_WINDOW_WIDTH)
            .default_height(crate::gtk_shell::DEFAULT_WINDOW_HEIGHT)
            .resizable(true)
            .content(&detached_window_shell)
            .build();
        detached_window.add_css_class("window-shell");
        detached_window.add_css_class("windows-gtk-shell");
        apply_active_preview_profile(&detached_window, &detached_preview);
        sync_windows_fullscreen_chrome(
            &detached_window,
            title_shell.upcast_ref(),
            &detached_fullscreen_button,
            true,
        );

        {
            let detached_window = detached_window.clone();
            detached_fullscreen_button.connect_clicked(move |_| {
                detached_window.set_fullscreened(!detached_window.is_fullscreen());
            });
        }
        {
            let detached_window = detached_window.clone();
            let title_shell = title_shell.clone();
            let detached_fullscreen_button = detached_fullscreen_button.clone();
            detached_window.connect_fullscreened_notify(move |window| {
                sync_windows_fullscreen_chrome(
                    window,
                    title_shell.upcast_ref(),
                    &detached_fullscreen_button,
                    true,
                );
            });
        }
        {
            let shell_state = shell_state.clone();
            let detached_preview = detached_preview.clone();
            detached_window.connect_is_active_notify(move |window| {
                if window.is_active() {
                    shell_state.set_voice_target(&detached_preview);
                    logging::info(
                        "Windows GTK detached workspace selected as voice dictation target",
                    );
                }
            });
        }
        install_detached_windows_voice_controls(
            &detached_window,
            shell_state,
            &detached_preview,
            detached_voice_hud.clone(),
            detached_overlay.clone(),
        );

        let reattaching = Rc::new(Cell::new(false));
        let do_reattach: Rc<dyn Fn()> = Rc::new({
            let detached_preview = detached_preview.clone();
            let detached_workspace_overlay = detached_workspace_overlay.clone();
            let detached_window = detached_window.clone();
            let origin_window = origin_window.clone();
            let origin_overlay = origin_overlay.clone();
            let origin_title = origin_title.clone();
            let launch = launch.clone();
            let back_button = back_button.clone();
            let fullscreen_button = fullscreen_button.clone();
            let shell_state = shell_state.clone();
            let reattaching = reattaching.clone();
            move || {
                shell_state.launch_deck_active.set(false);
                let next_index = if let Some(main_preview) = shell_state.preview.borrow().clone() {
                    let Some(detached_tab) = detached_preview.take_single_tab_for_transfer() else {
                        return;
                    };
                    main_preview.push_detached_tab(detached_tab)
                } else {
                    detached_workspace_overlay.set_child(None::<&gtk::Widget>);
                    *shell_state.preview.borrow_mut() = Some(detached_preview.clone());
                    0
                };
                show_workspace_preview_tab(
                    &origin_window,
                    &origin_overlay,
                    &origin_title,
                    &launch,
                    &back_button,
                    &fullscreen_button,
                    &shell_state,
                    next_index,
                );
                origin_window.present();
                shell_state.unregister_detached_preview(&detached_preview);
                shell_state.set_main_voice_target();
                reattaching.set(true);
                detached_window.close();
            }
        });
        {
            let do_reattach = do_reattach.clone();
            reattach_button.connect_clicked(move |_| do_reattach());
        }

        let popover = context_menu::popover(&title_shell);
        let menu = context_menu::menu_box();
        let menu_reattach_button = context_menu::action_button("Reattach", None);
        {
            let popover = popover.clone();
            let do_reattach = do_reattach.clone();
            menu_reattach_button.connect_clicked(move |_| {
                popover.popdown();
                do_reattach();
            });
        }
        menu.append(&menu_reattach_button);
        popover.set_child(Some(&menu));
        let right_click = gtk::GestureClick::builder()
            .button(3)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        {
            let popover = popover.clone();
            right_click.connect_pressed(move |gesture, _, x, y| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                context_menu::popup_at(&popover, x, y);
            });
        }
        header.add_controller(right_click);

        let title_right_click = gtk::GestureClick::builder()
            .button(3)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        {
            let popover = popover.clone();
            title_right_click.connect_pressed(move |gesture, _, x, y| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                context_menu::popup_at(&popover, x, y);
            });
        }
        title_shell.add_controller(title_right_click);

        let force_close = Rc::new(Cell::new(false));
        {
            let detached_preview = detached_preview.clone();
            let reattaching = reattaching.clone();
            let force_close_for_confirm = force_close.clone();
            let shell_state = shell_state.clone();
            detached_window.connect_close_request(move |window| {
                if reattaching.get() {
                    return glib::Propagation::Proceed;
                }
                if force_close_for_confirm.replace(false) {
                    shell_state.unregister_detached_preview(&detached_preview);
                    detached_preview.terminate_all("closing detached Windows GTK workspace");
                    return glib::Propagation::Proceed;
                }
                if detached_preview.has_active_processes() {
                    let window = window.clone();
                    let window_for_confirm = window.clone();
                    let force_close = force_close_for_confirm.clone();
                    dialog_chrome::confirm_destructive_action(
                        &window,
                        "Close Detached Workspace?",
                        "Running terminal sessions in this detached workspace will be terminated.",
                        "Close",
                        move || {
                            force_close.set(true);
                            window_for_confirm.close();
                        },
                    );
                    return glib::Propagation::Stop;
                }
                shell_state.unregister_detached_preview(&detached_preview);
                detached_preview.terminate_all("closing detached Windows GTK workspace");
                glib::Propagation::Proceed
            });
        }

        shell_state.register_detached_preview(&detached_preview);
        shell_state.set_voice_target(&detached_preview);
        detached_window.present();
    }

    fn install_detached_windows_voice_controls(
        detached_window: &adw::ApplicationWindow,
        shell_state: &WindowsGtkShellState,
        detached_preview: &crate::ui::workspace_preview::SessionPreview,
        detached_voice_hud: VoiceHud,
        detached_overlay: adw::ToastOverlay,
    ) {
        let Some(runtime) = shell_state.voice_runtime() else {
            return;
        };

        let detached_voice_key_controller: VoiceKeyControllerHandle = Rc::new(RefCell::new(None));
        let detached_voice_local_key_pressed = Rc::new(Cell::new(false));
        let (detached_voice_event_tx, detached_voice_event_rx) =
            mpsc::channel::<WindowsVoiceUiEvent>();

        install_windows_voice_hotkey_controller(
            detached_window,
            &detached_voice_key_controller,
            runtime.preference_store.clone(),
            shell_state.clone(),
            detached_voice_hud.clone(),
            detached_overlay.clone(),
            runtime.voice_transcriber.clone(),
            runtime.voice_listening.clone(),
            runtime.voice_starting.clone(),
            runtime.voice_stopping.clone(),
            detached_voice_local_key_pressed,
            detached_voice_event_tx.clone(),
        );

        {
            let detached_window_weak = detached_window.downgrade();
            let detached_preview = detached_preview.clone();
            let detached_voice_hud = detached_voice_hud.clone();
            let detached_overlay = detached_overlay.clone();
            let voice_starting = runtime.voice_starting.clone();
            let voice_listening = runtime.voice_listening.clone();
            let voice_stopping = runtime.voice_stopping.clone();
            gtk::glib::timeout_add_local(Duration::from_millis(80), move || {
                if detached_window_weak.upgrade().is_none() {
                    return gtk::glib::ControlFlow::Break;
                }
                while let Ok(event) = detached_voice_event_rx.try_recv() {
                    match event {
                        WindowsVoiceUiEvent::ListeningStarted => {
                            voice_starting.set(false);
                            voice_listening.set(!voice_stopping.get());
                        }
                        WindowsVoiceUiEvent::Final(text) => {
                            voice_starting.set(false);
                            voice_listening.set(false);
                            voice_stopping.set(false);
                            if text.trim().is_empty() {
                                detached_voice_hud.show("No speech detected", None);
                                detached_voice_hud.hide_later();
                                continue;
                            }
                            if detached_preview.send_text_to_focused_terminal(&text) {
                                detached_voice_hud.show("Voice inserted", Some(&text));
                                detached_voice_hud.hide_later();
                            } else {
                                detached_voice_hud.show("No focused terminal target", Some(&text));
                                detached_overlay.add_toast(adw::Toast::new(
                                    "Voice text was not inserted: no focused terminal pane",
                                ));
                            }
                        }
                        WindowsVoiceUiEvent::Partial(text) => {
                            detached_voice_hud.show("Voice partial", Some(&text));
                        }
                        WindowsVoiceUiEvent::Status(message) => {
                            if voice_stopping.get() && message == "Listening…" {
                                continue;
                            }
                            detached_voice_hud.show(&message, None);
                        }
                        WindowsVoiceUiEvent::Error(message) => {
                            voice_starting.set(false);
                            voice_listening.set(false);
                            voice_stopping.set(false);
                            logging::error(format!(
                                "Windows GTK detached voice transcription failed: {message}"
                            ));
                            detached_voice_hud.show("Voice error", Some(&message));
                            detached_overlay
                                .add_toast(adw::Toast::new("Voice transcription failed"));
                        }
                        WindowsVoiceUiEvent::GlobalHotkeyActivated => {}
                    }
                }
                gtk::glib::ControlFlow::Continue
            });
        }

        {
            let detached_window_weak = detached_window.downgrade();
            let voice_transcriber = runtime.voice_transcriber.clone();
            let voice_listening = runtime.voice_listening.clone();
            gtk::glib::timeout_add_local(VOICE_AUDIO_FLUSH_INTERVAL, move || {
                if detached_window_weak.upgrade().is_none() {
                    return gtk::glib::ControlFlow::Break;
                }
                if voice_listening.get() {
                    voice_transcriber.flush(&detached_voice_event_tx);
                }
                gtk::glib::ControlFlow::Continue
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn close_windows_preview_tab_now(
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
            return;
        };
        if !preview.close_tab(index) {
            return;
        }
        if preview.snapshot().tabs.is_empty() {
            *shell_state.preview.borrow_mut() = None;
            shell_state.set_main_voice_target();
            show_launch_deck_tab(
                window,
                overlay,
                title,
                launch,
                back_button,
                fullscreen_button,
                shell_state,
            );
        } else if shell_state.launch_deck_active.get() {
            show_launch_deck_tab(
                window,
                overlay,
                title,
                launch,
                back_button,
                fullscreen_button,
                shell_state,
            );
        } else {
            show_workspace_preview_tab(
                window,
                overlay,
                title,
                launch,
                back_button,
                fullscreen_button,
                shell_state,
                preview.active_index(),
            );
        }
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
            terminal_history: Vec::new(),
        };

        if let Some(preview) = shell_state.preview.borrow().as_ref().cloned() {
            preview.push_tab(saved_tab);
            shell_state.launch_deck_active.set(false);
            shell_state.set_voice_target(&preview);
            apply_active_preview_profile(window, &preview);
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
                false,
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
        apply_agent_resume_overrides: bool,
    ) {
        let (tabs, panes) = crate::ui::workspace_preview::session_shape(&session);
        let session_store = shell_state.session_store.clone();
        let restore_startup_overrides: RestoreStartupOverridesByTab =
            if apply_agent_resume_overrides {
                restore_startup_overrides_for_saved_session(&session)
            } else {
                RestoreStartupOverridesByTab::new()
            };
        let runtime_factory: crate::ui::workspace_preview::TileRuntimeFactory = Rc::new(
            move |tab_index: usize,
                  tile: &crate::model::layout::TileSpec,
                  tab: &SavedTab,
                  assets: &crate::model::assets::WorkspaceAssets| {
                crate::windows::gtk_runtime::build_tile_runtime_surface_with_restore_override(
                    tile,
                    tab,
                    assets,
                    restore_startup_override_for_tab_tile(
                        &restore_startup_overrides,
                        tab_index,
                        &tile.id,
                    )
                    .map(str::to_owned),
                )
            },
        );
        let preview =
            crate::ui::workspace_preview::SessionPreview::with_runtime_assets_and_change_handler(
                &session,
                false,
                assets,
                Some(runtime_factory),
                Some(Rc::new(move |session, reason| {
                    persist_windows_gtk_session(&session_store, &session, reason);
                })),
            );
        *shell_state.preview.borrow_mut() = Some(preview.clone());
        shell_state.launch_deck_active.set(false);
        shell_state.set_voice_target(&preview);
        apply_active_preview_profile(window, &preview);
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
        shell_state.save_preview_session(&format!("Windows GTK workspace {action}"));
        overlay.add_toast(adw::Toast::new(
            "Workspace opened in the shared interactive GTK shell",
        ));
    }

    struct WindowsTitleTab {
        label: String,
        tooltip: String,
        active: bool,
        reorder_index: Option<usize>,
        on_select: Option<Rc<dyn Fn()>>,
        on_rename: Option<Rc<dyn Fn()>>,
        on_close: Option<Rc<dyn Fn()>>,
        on_reorder: Option<Rc<dyn Fn(usize, usize)>>,
        on_detach: Option<Rc<dyn Fn()>>,
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
        let close_enabled = tab.on_close.is_some();
        let chrome = build_interactive_title_tab(TitleTabInput {
            label: tab.label,
            tooltip: tab.tooltip,
            active: tab.active,
            close_enabled,
            on_select: tab.on_select,
            on_rename: tab.on_rename,
            on_close: tab.on_close,
        });
        if let (Some(index), Some(on_reorder)) = (tab.reorder_index, tab.on_reorder) {
            install_windows_title_tab_reorder(
                &chrome.shell,
                &chrome.select_button,
                index,
                on_reorder,
            );
        }
        if let Some(on_detach) = tab.on_detach {
            install_windows_title_tab_context_menu(&chrome.shell, on_detach);
        }
        chrome.shell.upcast()
    }

    fn install_windows_title_tab_context_menu(shell: &gtk::Box, on_detach: Rc<dyn Fn()>) {
        let popover = context_menu::popover(shell);
        let menu = context_menu::menu_box();
        let detach_button = context_menu::action_button("Detach", None);
        {
            let popover = popover.clone();
            detach_button.connect_clicked(move |_| {
                popover.popdown();
                on_detach();
            });
        }
        menu.append(&detach_button);
        popover.set_child(Some(&menu));

        let right_click = gtk::GestureClick::builder()
            .button(3)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        {
            let popover = popover.clone();
            right_click.connect_pressed(move |gesture, _, x, y| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                context_menu::popup_at(&popover, x, y);
            });
        }
        shell.add_controller(right_click);
    }

    fn install_windows_title_tab_reorder(
        shell: &gtk::Box,
        select_button: &gtk::Button,
        index: usize,
        on_reorder: Rc<dyn Fn(usize, usize)>,
    ) {
        let drag_source = gtk::DragSource::builder()
            .actions(gdk::DragAction::MOVE)
            .button(1)
            .build();
        drag_source.connect_prepare(move |source, _, _| {
            suppress_windows_title_tab_drag_icon(source);
            Some(gdk::ContentProvider::for_value(&(index as u32).to_value()))
        });
        select_button.add_controller(drag_source);

        let drop_target = gtk::DropTarget::new(u32::static_type(), gdk::DragAction::MOVE);
        drop_target.set_propagation_phase(gtk::PropagationPhase::Capture);
        drop_target.connect_drop(move |target, value, x, _| {
            let Ok(from_index) = value.get::<u32>() else {
                return false;
            };
            let width = target
                .widget()
                .map(|widget| f64::from(widget.allocation().width()))
                .unwrap_or_default();
            let raw_position = index + usize::from(x >= width / 2.0);
            let position = windows_title_tab_drop_position(from_index as usize, raw_position);
            on_reorder(from_index as usize, position);
            true
        });
        shell.add_controller(drop_target);
    }

    fn windows_title_tab_drop_position(from_index: usize, raw_position: usize) -> usize {
        if from_index < raw_position {
            raw_position.saturating_sub(1)
        } else {
            raw_position
        }
    }

    fn suppress_windows_title_tab_drag_icon(source: &gtk::DragSource) {
        let empty_icon = gdk::Paintable::new_empty(1, 1);
        source.set_icon(Some(&empty_icon), 0, 0);
    }

    #[allow(clippy::too_many_arguments)]
    fn present_windows_tab_rename(
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
            return;
        };
        let Some(current_title) = preview.tab_title(index) else {
            return;
        };

        tab_rename_dialog::present(window, &current_title, {
            let window = window.clone();
            let overlay = overlay.clone();
            let title = title.clone();
            let launch = launch.clone();
            let back_button = back_button.clone();
            let fullscreen_button = fullscreen_button.clone();
            let shell_state = shell_state.clone();
            move |requested_title| {
                let preview = shell_state.preview.borrow().clone();
                let Some(preview) = preview else {
                    return;
                };
                if !preview.rename_tab(index, requested_title) {
                    return;
                }
                if shell_state.launch_deck_active.get() {
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
        });
    }

    type ShortcutControllerHandle = Rc<RefCell<Option<gtk::ShortcutController>>>;
    type VoiceKeyControllerHandle = Rc<RefCell<Option<gtk::EventControllerKey>>>;
    type TileSelectionKeyControllerHandle = Rc<RefCell<Option<gtk::EventControllerKey>>>;
    type LaunchWidgetHandle = Rc<RefCell<Option<gtk::Widget>>>;
    type VoidCallbackHandle = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
    type WeakVoidCallbackHandle = Weak<RefCell<Option<Rc<dyn Fn()>>>>;

    #[derive(Clone)]
    struct WindowsSettingsDialogContext {
        window: adw::ApplicationWindow,
        overlay: adw::ToastOverlay,
        title: TitleChrome,
        fullscreen_button: gtk::Button,
        shell_state: WindowsGtkShellState,
        current_close_to_background: Rc<Cell<bool>>,
        preference_store: PreferenceStore,
        preset_store: PresetStore,
        options: RuntimeOptions,
        voice_toast_tx: mpsc::Sender<String>,
        voice_global_hotkey: Rc<RefCell<Option<WindowsVoiceGlobalHotkeyRegistration>>>,
        voice_event_tx: mpsc::Sender<WindowsVoiceUiEvent>,
        workspace_fullscreen_shortcut_controller: ShortcutControllerHandle,
        workspace_density_shortcut_controller: ShortcutControllerHandle,
        workspace_zoom_in_shortcut_controller: ShortcutControllerHandle,
        workspace_zoom_out_shortcut_controller: ShortcutControllerHandle,
        workspace_tile_selection_shortcut_controller: TileSelectionKeyControllerHandle,
        command_palette_shortcut_controller: ShortcutControllerHandle,
        open_command_palette_handle: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
        refresh_launch_deck_handle: WeakVoidCallbackHandle,
    }

    #[derive(Clone)]
    struct WindowsLaunchDeckContext {
        app: adw::Application,
        window: adw::ApplicationWindow,
        overlay: adw::ToastOverlay,
        title: TitleChrome,
        preference_store: PreferenceStore,
        preset_store: PresetStore,
        asset_store: AssetStore,
        back_button: gtk::Button,
        fullscreen_button: gtk::Button,
        shell_state: WindowsGtkShellState,
        launch_widget_handle: LaunchWidgetHandle,
        refresh_launch_deck_handle: VoidCallbackHandle,
    }

    fn present_command_palette(
        window: &adw::ApplicationWindow,
        overlay: &adw::ToastOverlay,
        title: &TitleChrome,
        launch: &gtk::Widget,
        back_button: &gtk::Button,
        fullscreen_button: &gtk::Button,
        shell_state: &WindowsGtkShellState,
        settings_context: WindowsSettingsDialogContext,
        asset_store: AssetStore,
    ) {
        let mut actions = command_palette::app_actions(command_palette::AppActionCallbacks {
            product_display_name: settings_context.options.product.display_name.clone(),
            open_settings: Rc::new({
                let settings_context = settings_context.clone();
                move || present_settings_dialog(settings_context.clone())
            }),
            open_stats: Rc::new({
                let window = window.clone();
                move || stats_dialog::present_shared(&window)
            }),
            open_assets_manager: Rc::new({
                let window = window.clone();
                let overlay = overlay.clone();
                let asset_store = asset_store.clone();
                move || present_assets_manager(&window, &overlay, asset_store.clone())
            }),
            open_about: Rc::new({
                let window = window.clone();
                let options = settings_context.options.clone();
                move || about_dialog::present(&window, &options.product)
            }),
            open_shortcuts: Rc::new({
                let window = window.clone();
                let preference_store = settings_context.preference_store.clone();
                move || {
                    let prefs = preference_store.load();
                    crate::ui::shortcuts_dialog::present(
                        &window,
                        crate::ui::shortcuts_dialog::sections_from_summary(
                            &crate::ui::shortcuts_dialog::ShortcutSummary {
                                fullscreen: prefs.workspace_fullscreen_shortcut.clone(),
                                density: prefs.workspace_density_shortcut.clone(),
                                zoom_in: prefs.workspace_zoom_in_shortcut.clone(),
                                zoom_out: prefs.workspace_zoom_out_shortcut.clone(),
                                tile_selection_prefix: prefs
                                    .workspace_tile_selection_prefix_shortcut
                                    .clone(),
                                command_palette: prefs.command_palette_shortcut.clone(),
                                maximize: crate::ui::shortcuts_dialog::DEFAULT_MAXIMIZE_ACCEL
                                    .to_string(),
                                add_terminal_tile:
                                    crate::ui::shortcuts_dialog::DEFAULT_ADD_TERMINAL_TILE_ACCEL
                                        .to_string(),
                                open_board: "<Ctrl><Shift>K".to_string(),
                            },
                        ),
                    );
                }
            }),
            new_tab: Rc::new({
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
            open_companion: settings_context.options.companion.clone().map(|companion| {
                Rc::new({
                    let window = window.clone();
                    move || companion_dialog::present(&window, companion.clone())
                }) as Rc<dyn Fn()>
            }),
        });

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

            if !shell_state.launch_deck_active.get() {
                actions.extend(command_palette::active_tab_actions(Rc::new({
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
                        present_windows_tab_rename(
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
                })));

                let runbooks = preview
                    .runbooks()
                    .into_iter()
                    .filter(|runbook| runbook.variables.is_empty())
                    .map(|runbook| {
                        let runbook_for_action = runbook.clone();
                        let runbook_for_callback = runbook;
                        let preview = preview.clone();
                        command_palette::RunbookAction {
                            runbook: runbook_for_action,
                            on_activate: Rc::new(move || {
                                let _ = preview.run_runbook(&runbook_for_callback);
                            }),
                        }
                    })
                    .collect();

                actions.extend(command_palette::workspace_actions(
                    command_palette::WorkspaceActionCallbacks {
                        focus_next_alert: Rc::new({
                            let preview = preview.clone();
                            move || {
                                let _ = preview.focus_next_alert();
                            }
                        }),
                        // The Windows shell renders a static session preview, so
                        // there is no live pane layout to maximize.
                        toggle_maximize: Rc::new(|| {}),
                        add_terminal_tile: Rc::new({
                            let preview = preview.clone();
                            move || {
                                let _ = preview.add_terminal_tile();
                            }
                        }),
                        add_web_tile: Rc::new({
                            let preview = preview.clone();
                            move || {
                                let _ = preview.add_web_tile(DEFAULT_WEB_URL);
                            }
                        }),
                        // The Windows preview shell does not yet host Kanban board tabs.
                        open_board: Rc::new(|| {}),
                        runbooks,
                    },
                ));
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

    fn install_workspace_fullscreen_shortcut(
        window: &adw::ApplicationWindow,
        controller_handle: &ShortcutControllerHandle,
        shell_state: &WindowsGtkShellState,
        shortcut: &str,
    ) {
        let window_for_shortcut = window.clone();
        let shell_state_for_shortcut = shell_state.clone();
        install_shortcut_controller(
            window,
            controller_handle,
            "workspace_fullscreen",
            &[
                shortcut.trim().to_string(),
                AppPreferences::default().workspace_fullscreen_shortcut,
            ],
            move || {
                if shell_state_for_shortcut.preview.borrow().is_none()
                    || shell_state_for_shortcut.launch_deck_active.get()
                {
                    return glib::Propagation::Proceed;
                }

                window_for_shortcut.set_fullscreened(!window_for_shortcut.is_fullscreen());
                glib::Propagation::Stop
            },
        );
    }

    fn install_workspace_density_shortcut(
        window: &adw::ApplicationWindow,
        controller_handle: &ShortcutControllerHandle,
        shell_state: &WindowsGtkShellState,
        shortcut: &str,
    ) {
        let window_for_shortcut = window.clone();
        let shell_state_for_shortcut = shell_state.clone();
        install_shortcut_controller(
            window,
            controller_handle,
            "workspace_density",
            &[
                shortcut.trim().to_string(),
                AppPreferences::default().workspace_density_shortcut,
            ],
            move || {
                if shell_state_for_shortcut.launch_deck_active.get() {
                    return glib::Propagation::Proceed;
                }
                let preview = shell_state_for_shortcut.preview.borrow().clone();
                let Some(preview) = preview else {
                    return glib::Propagation::Proceed;
                };
                let Some(next_density) = preview.cycle_active_density() else {
                    return glib::Propagation::Proceed;
                };

                apply_window_density(&window_for_shortcut, next_density);
                logging::info(format!(
                    "Windows GTK cycled workspace density={}",
                    next_density.label()
                ));
                glib::Propagation::Stop
            },
        );
    }

    fn install_workspace_zoom_shortcut(
        window: &adw::ApplicationWindow,
        controller_handle: &ShortcutControllerHandle,
        shell_state: &WindowsGtkShellState,
        shortcut: &str,
        delta: i32,
        shortcut_name: &str,
    ) {
        let shell_state_for_shortcut = shell_state.clone();
        install_shortcut_controller(
            window,
            controller_handle,
            shortcut_name,
            &workspace_zoom_shortcut_accelerators(shortcut, delta),
            move || {
                if shell_state_for_shortcut.launch_deck_active.get() {
                    return glib::Propagation::Proceed;
                }
                let preview = shell_state_for_shortcut.preview.borrow().clone();
                let Some(preview) = preview else {
                    return glib::Propagation::Proceed;
                };
                let Some(zoom_steps) = preview.adjust_active_zoom(delta) else {
                    return glib::Propagation::Proceed;
                };

                logging::info(format!(
                    "Windows GTK adjusted workspace terminal zoom_steps={zoom_steps}"
                ));
                glib::Propagation::Stop
            },
        );
    }

    fn tile_selection_prefix_matches(
        shortcut: &str,
        key: gdk::Key,
        state: gdk::ModifierType,
    ) -> bool {
        let Some((expected_key, expected_modifiers)) = gtk::accelerator_parse(shortcut) else {
            return false;
        };
        let event_modifiers = state & gtk::accelerator_get_default_mod_mask();
        key == expected_key && event_modifiers == expected_modifiers
    }

    fn tile_selection_prefix_key_matches(shortcut: &str, key: gdk::Key) -> bool {
        let Some((expected_key, _)) = gtk::accelerator_parse(shortcut) else {
            return false;
        };
        key == expected_key
    }

    fn tile_direction_from_key(key: gdk::Key) -> Option<TileDirection> {
        match key {
            gdk::Key::Up => Some(TileDirection::Up),
            gdk::Key::Down => Some(TileDirection::Down),
            gdk::Key::Left => Some(TileDirection::Left),
            gdk::Key::Right => Some(TileDirection::Right),
            _ => None,
        }
    }

    fn install_workspace_tile_selection_shortcut(
        window: &adw::ApplicationWindow,
        controller_handle: &TileSelectionKeyControllerHandle,
        shell_state: &WindowsGtkShellState,
        shortcut: &str,
    ) {
        if let Some(existing) = controller_handle.borrow_mut().take() {
            window.remove_controller(&existing);
        }

        let shortcut = if gtk::accelerator_parse(shortcut).is_some() {
            shortcut.trim().to_string()
        } else {
            logging::error(format!(
                "failed to parse workspace_tile_selection shortcut accelerator='{}'; using default",
                shortcut
            ));
            AppPreferences::default().workspace_tile_selection_prefix_shortcut
        };

        let key_controller = gtk::EventControllerKey::new();
        key_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        let prefix_active = Rc::new(Cell::new(false));
        {
            let prefix_active = prefix_active.clone();
            let shortcut = shortcut.clone();
            let shell_state_for_shortcut = shell_state.clone();
            key_controller.connect_key_pressed(move |_, key, _, state| {
                if tile_selection_prefix_matches(&shortcut, key, state) {
                    prefix_active.set(true);
                    return glib::Propagation::Stop;
                }

                let Some(direction) = tile_direction_from_key(key) else {
                    return glib::Propagation::Proceed;
                };
                if !prefix_active.get() || shell_state_for_shortcut.launch_deck_active.get() {
                    return glib::Propagation::Proceed;
                }

                let preview = shell_state_for_shortcut.preview.borrow().clone();
                let Some(preview) = preview else {
                    return glib::Propagation::Proceed;
                };
                let _ = preview.focus_tile_in_direction(direction);
                glib::Propagation::Stop
            });
        }
        {
            let prefix_active = prefix_active.clone();
            let shortcut = shortcut.clone();
            key_controller.connect_key_released(move |_, key, _, _| {
                if tile_selection_prefix_key_matches(&shortcut, key) {
                    prefix_active.set(false);
                }
            });
        }

        logging::info(format!(
            "installed workspace_tile_selection shortcut prefix={shortcut:?}"
        ));
        window.add_controller(key_controller.clone());
        *controller_handle.borrow_mut() = Some(key_controller);
    }

    fn install_workspace_add_terminal_tile_shortcut(
        window: &adw::ApplicationWindow,
        controller_handle: &ShortcutControllerHandle,
        shell_state: &WindowsGtkShellState,
    ) {
        let shell_state_for_shortcut = shell_state.clone();
        install_shortcut_controller(
            window,
            controller_handle,
            "workspace_add_terminal_tile",
            &[crate::ui::shortcuts_dialog::DEFAULT_ADD_TERMINAL_TILE_ACCEL.to_string()],
            move || {
                if shell_state_for_shortcut.launch_deck_active.get() {
                    return glib::Propagation::Proceed;
                }
                let preview = shell_state_for_shortcut.preview.borrow().clone();
                let Some(preview) = preview else {
                    return glib::Propagation::Proceed;
                };
                if preview.add_terminal_tile() {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
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

    fn workspace_zoom_shortcut_accelerators(shortcut: &str, delta: i32) -> Vec<String> {
        if delta > 0 {
            equivalent_shortcut_accelerators(
                shortcut,
                &[
                    &["<Ctrl>plus", "<Ctrl>equal", "<Ctrl>KP_Add"],
                    &["<Control>plus", "<Control>equal", "<Control>KP_Add"],
                    &["<Primary>plus", "<Primary>equal", "<Primary>KP_Add"],
                    &["<Alt>plus", "<Alt>equal", "<Alt>KP_Add"],
                    &["<Ctrl><Alt>plus", "<Ctrl><Alt>equal", "<Ctrl><Alt>KP_Add"],
                    &[
                        "<Control><Alt>plus",
                        "<Control><Alt>equal",
                        "<Control><Alt>KP_Add",
                    ],
                ],
            )
        } else {
            equivalent_shortcut_accelerators(
                shortcut,
                &[
                    &["<Ctrl>minus", "<Ctrl>KP_Subtract"],
                    &["<Control>minus", "<Control>KP_Subtract"],
                    &["<Primary>minus", "<Primary>KP_Subtract"],
                    &["<Alt>minus", "<Alt>KP_Subtract"],
                    &["<Ctrl><Alt>minus", "<Ctrl><Alt>KP_Subtract"],
                    &["<Control><Alt>minus", "<Control><Alt>KP_Subtract"],
                ],
            )
        }
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
