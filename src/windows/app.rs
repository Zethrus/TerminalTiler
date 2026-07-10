use std::process::ExitCode;

#[cfg(target_os = "windows")]
mod imp {
    use super::ExitCode;
    use std::mem;
    use std::path::PathBuf;
    use std::ptr;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use std::thread;

    use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject, HDC, UpdateWindow};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Controls::{PBM_SETPOS, PBM_SETRANGE32, PBS_SMOOTH};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
        SetCurrentProcessExplicitAppUserModelID, Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, BM_GETCHECK, BM_SETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_PUSHBUTTON,
        CB_ADDSTRING, CB_GETCURSEL, CB_RESETCONTENT, CB_SETCURSEL, CBN_SELCHANGE, CBS_DROPDOWNLIST,
        CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW,
        DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, EN_CHANGE, ES_AUTOHSCROLL,
        ES_AUTOVSCROLL, ES_LEFT, ES_MULTILINE, ES_PASSWORD, ES_READONLY, GWLP_USERDATA,
        GetClientRect, GetCursorPos, GetDlgItem, GetMessageW, GetWindowLongPtrW, HMENU, IDC_ARROW,
        IDI_APPLICATION, IDOK, LB_ADDSTRING, LB_ERR, LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL,
        LBN_DBLCLK, LBN_SELCHANGE, LBS_NOTIFY, LoadCursorW, LoadIconW, MB_ICONWARNING, MB_OK,
        MB_OKCANCEL, MF_STRING, MSG, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW,
        SW_HIDE, SW_SHOW, SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetWindowLongPtrW,
        SetWindowPos, SetWindowTextW, ShowWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
        TranslateMessage, WM_APP, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_CTLCOLOREDIT,
        WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONUP,
        WM_NCCREATE, WM_NCDESTROY, WM_RBUTTONUP, WM_SETFONT, WM_SIZE, WNDCLASSW, WS_BORDER,
        WS_CHILD, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
    };

    use crate::extension::{
        CompanionAction, CompanionActionInput, CompanionIntegration, CompanionPanelSnapshot,
        ProductInfo, RuntimeOptions,
    };
    use crate::logging;
    use crate::model::assets::{ProjectSuggestion, RestoreLaunchMode, WorkspaceAssets};
    use crate::model::layout::{
        LayoutNode, LayoutTemplate, TileKind, builtin_templates, generate_layout,
    };
    use crate::model::preset::{
        ApplicationDensity, ThemeMode, WorkspacePreset, is_builtin_preset_id,
    };
    use crate::platform::{home_dir, resolve_workspace_root};
    use crate::services::agent_resume::{
        RestoreStartupOverridesByTab, restore_startup_overrides_for_saved_session,
    };
    use crate::services::project_suggestions::detect_project_suggestions;
    use crate::services::session_restore::{
        RestoreStartupAction, session_for_restore_mode, session_for_startup_action,
    };
    use crate::services::tile_draft::{apply_project_suggestion, resize_layout};
    use crate::storage::asset_store::AssetStore;
    use crate::storage::preference_store::{AppPreferences, PreferenceStore};
    use crate::storage::preset_store::PresetStore;
    use crate::storage::session_store::{SavedSession, SessionStore};
    use crate::voice::audio::AudioCapture;
    use crate::voice::engine::{self, VoiceEngineEvent};
    use crate::voice::pack::{self, VoicePackHealth};
    use crate::voice::{VoiceActivationMode, VoiceEngineMode, VoicePackStatus};
    use crate::windows::win32_helpers::{
        create_child_window_with_ex_style as create_child_window, open_path_with_shell,
        read_window_text, wide,
    };
    use crate::windows::workspace;
    use crate::windows::wsl::{self, WindowsRuntime};
    use crate::windows::{
        assets_manager, command_palette, launcher_editor, restore_prompt, shortcut_capture, theme,
    };

    const WINDOW_CLASS: &str = "TerminalTilerWindowsShell";
    const SETTINGS_WINDOW_CLASS: &str = "TerminalTilerWindowsSettings";
    const COMPANION_PROMPT_CLASS: &str = "TerminalTilerWindowsCompanionPrompt";
    const ID_STATUS: isize = 1001;
    const ID_REFRESH: isize = 1002;
    const ID_LAUNCH: isize = 1003;
    const ID_QUIT: isize = 1004;
    const ID_WORKSPACE_PATH: isize = 1005;
    const ID_LAUNCH_NAME: isize = 1006;
    const ID_PRESET_LIST: isize = 1007;
    const ID_LAUNCH_PRESET: isize = 1008;
    const ID_SAVE_PRESET: isize = 1009;
    const ID_LABEL_PATH: isize = 1010;
    const ID_LABEL_NAME: isize = 1011;
    const ID_LABEL_PRESETS: isize = 1012;
    const ID_SETTINGS: isize = 1013;
    const ID_UPDATE_PRESET: isize = 1014;
    const ID_DELETE_PRESET: isize = 1015;
    const ID_LABEL_TEMPLATES: isize = 1016;
    const ID_TEMPLATE_LIST: isize = 1017;
    const ID_LABEL_TILE_COUNT: isize = 1018;
    const ID_TILE_COUNT: isize = 1019;
    const ID_LABEL_SELECTION_SUMMARY: isize = 1020;
    const ID_SELECTION_SUMMARY: isize = 1021;
    const ID_LABEL_SUGGESTIONS: isize = 1022;
    const ID_SUGGESTION_LIST: isize = 1023;
    const ID_APPLY_SUGGESTION: isize = 1024;
    const ID_ASSETS_MANAGER: isize = 1025;
    const ID_COMMAND_PALETTE: isize = 1026;
    const ID_LABEL_THEME: isize = 1027;
    const ID_THEME_COMBO: isize = 1028;
    const ID_LABEL_LAUNCH_DENSITY: isize = 1029;
    const ID_LAUNCH_DENSITY_COMBO: isize = 1030;
    const ID_EDIT_TILES: isize = 1031;
    const ID_COMPANION: isize = 1032;
    const ID_PROMPT_EDIT: isize = 1901;
    const ID_PROMPT_OK: isize = 1902;
    const ID_PROMPT_CANCEL: isize = 1903;
    const ID_PROMPT_LABEL: isize = 1904;
    const ID_SETTINGS_THEME_LIST: isize = 2001;
    const ID_SETTINGS_DENSITY_LIST: isize = 2002;
    const ID_SETTINGS_CLOSE_BACKGROUND: isize = 2003;
    const ID_SETTINGS_WSL_DISTRO: isize = 2004;
    const ID_SETTINGS_RUNTIME_STATUS: isize = 2005;
    const ID_SETTINGS_RESET: isize = 2007;
    const ID_SETTINGS_CLOSE: isize = 2008;
    const ID_SETTINGS_PROBE: isize = 2009;
    const ID_SETTINGS_LABEL_THEME: isize = 2010;
    const ID_SETTINGS_LABEL_DENSITY: isize = 2011;
    const ID_SETTINGS_LABEL_DISTRO: isize = 2012;
    const ID_SETTINGS_LABEL_RUNTIME: isize = 2013;
    const ID_SETTINGS_LABEL_SHORTCUTS: isize = 2014;
    const ID_SETTINGS_SHORTCUT_STATUS: isize = 2015;
    const ID_SETTINGS_FULLSCREEN_SHORTCUT: isize = 2016;
    const ID_SETTINGS_FULLSCREEN_RECORD: isize = 2017;
    const ID_SETTINGS_DENSITY_SHORTCUT: isize = 2018;
    const ID_SETTINGS_DENSITY_RECORD: isize = 2019;
    const ID_SETTINGS_ZOOM_IN_SHORTCUT: isize = 2020;
    const ID_SETTINGS_ZOOM_IN_RECORD: isize = 2021;
    const ID_SETTINGS_ZOOM_OUT_SHORTCUT: isize = 2022;
    const ID_SETTINGS_ZOOM_OUT_RECORD: isize = 2023;
    const ID_SETTINGS_COMMAND_PALETTE_SHORTCUT: isize = 2024;
    const ID_SETTINGS_COMMAND_PALETTE_RECORD: isize = 2025;
    const ID_SETTINGS_LABEL_FULLSCREEN_SHORTCUT: isize = 2026;
    const ID_SETTINGS_LABEL_DENSITY_SHORTCUT: isize = 2027;
    const ID_SETTINGS_LABEL_ZOOM_IN_SHORTCUT: isize = 2028;
    const ID_SETTINGS_LABEL_ZOOM_OUT_SHORTCUT: isize = 2029;
    const ID_SETTINGS_LABEL_COMMAND_PALETTE_SHORTCUT: isize = 2030;
    const ID_SETTINGS_NOTE_FULLSCREEN_SHORTCUT: isize = 2031;
    const ID_SETTINGS_NOTE_DENSITY_SHORTCUT: isize = 2032;
    const ID_SETTINGS_NOTE_ZOOM_IN_SHORTCUT: isize = 2033;
    const ID_SETTINGS_NOTE_ZOOM_OUT_SHORTCUT: isize = 2034;
    const ID_SETTINGS_NOTE_COMMAND_PALETTE_SHORTCUT: isize = 2035;
    const ID_SETTINGS_HELP_FULLSCREEN_SHORTCUT: isize = 2036;
    const ID_SETTINGS_HELP_DENSITY_SHORTCUT: isize = 2037;
    const ID_SETTINGS_HELP_ZOOM_IN_SHORTCUT: isize = 2038;
    const ID_SETTINGS_HELP_ZOOM_OUT_SHORTCUT: isize = 2039;
    const ID_SETTINGS_HELP_COMMAND_PALETTE_SHORTCUT: isize = 2040;
    const ID_SETTINGS_SUMMARY_TITLE: isize = 2041;
    const ID_SETTINGS_SUMMARY_COPY: isize = 2042;
    const ID_SETTINGS_META_AUTOSAVE: isize = 2043;
    const ID_SETTINGS_META_LIVE: isize = 2044;
    const ID_SETTINGS_RESET_BUILTIN_PRESETS: isize = 2045;
    const ID_SETTINGS_LABEL_VOICE: isize = 2046;
    const ID_SETTINGS_VOICE_ENABLED: isize = 2047;
    const ID_SETTINGS_LABEL_VOICE_ACTIVATION: isize = 2048;
    const ID_SETTINGS_VOICE_ACTIVATION: isize = 2049;
    const ID_SETTINGS_LABEL_VOICE_MICROPHONE: isize = 2061;
    const ID_SETTINGS_VOICE_MICROPHONE: isize = 2062;
    const ID_SETTINGS_LABEL_VOICE_HOTKEY: isize = 2050;
    const ID_SETTINGS_VOICE_HOTKEY: isize = 2051;
    const ID_SETTINGS_VOICE_RECORD: isize = 2052;
    const ID_SETTINGS_NOTE_VOICE_HOTKEY: isize = 2053;
    const ID_SETTINGS_LABEL_VOICE_ENGINE: isize = 2054;
    const ID_SETTINGS_VOICE_ENGINE: isize = 2055;
    const ID_SETTINGS_VOICE_GLOBAL: isize = 2056;
    const ID_SETTINGS_VOICE_PACK_STATUS: isize = 2057;
    const ID_SETTINGS_VOICE_PACK_INSTALL: isize = 2058;
    const ID_SETTINGS_VOICE_PACK_HEALTH: isize = 2059;
    const ID_SETTINGS_VOICE_PACK_DELETE: isize = 2060;
    const ID_SETTINGS_VOICE_PACK_PROGRESS: isize = 2063;
    const ID_SETTINGS_OPEN_LOGS_FOLDER: isize = 2064;
    const BUTTON_HEIGHT: i32 = theme::COMPACT_DENSITY.button_height;
    const BUTTON_WIDTH: i32 = 160;
    const MARGIN: i32 = theme::COMPACT_DENSITY.margin;
    const FIELD_HEIGHT: i32 = theme::COMPACT_DENSITY.field_height;
    const LABEL_HEIGHT: i32 = theme::COMPACT_DENSITY.label_height;
    const LIST_HEIGHT: i32 = 150;
    const SETTINGS_LIST_HEIGHT: i32 = 64;
    const CHECKBOX_UNCHECKED: usize = 0;
    const CHECKBOX_CHECKED: usize = 1;
    const WM_TRAYICON: u32 = 0x8001;
    const WM_STARTUP_INIT: u32 = WM_APP + 49;
    const WM_STARTUP_PROBE_REQUEST: u32 = WM_APP + 50;
    const WM_STARTUP_PROBE_COMPLETE: u32 = WM_APP + 51;
    const WM_SETTINGS_VOICE_PACK_EVENT: u32 = WM_APP + 60;
    const WM_COMPANION_ACTION_COMPLETE: u32 = WM_APP + 61;
    const TRAY_ICON_ID: u32 = 1;
    const TRAY_MENU_SHOW: usize = 1;
    const TRAY_MENU_SETTINGS: usize = 2;
    const TRAY_MENU_QUIT: usize = 3;
    static PRIMARY_SHELL_HWND: AtomicIsize = AtomicIsize::new(0);

    #[derive(Clone, Copy, Debug)]
    enum LaunchSelection {
        Template(usize),
        Preset(usize),
    }

    pub fn run() -> ExitCode {
        run_with_options(RuntimeOptions::default())
    }

    pub fn run_with_options(options: RuntimeOptions) -> ExitCode {
        logging::init();
        logging::info("windows GUI shell startup");

        match unsafe { run_gui(options) } {
            Ok(code) => code,
            Err(error) => {
                logging::error(format!("windows GUI shell failed: {error}"));
                show_fatal_startup_error(&error);
                ExitCode::FAILURE
            }
        }
    }

    fn show_fatal_startup_error(error: &str) {
        let log_folder = logging::ensure_log_directory()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|log_error| format!("unavailable ({log_error})"));
        let message = format!(
            "TerminalTiler could not start.

{error}

Logs: {log_folder}

Please include terminaltiler.log and terminaltiler-session.log when reporting this startup failure."
        );
        unsafe {
            MessageBoxW(
                ptr::null_mut(),
                wide(&message).as_ptr(),
                wide("TerminalTiler startup failure").as_ptr(),
                MB_OK | MB_ICONWARNING,
            );
        }
    }

    fn themed_surface_for_message(message: u32) -> theme::ControlSurface {
        match message {
            WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => theme::ControlSurface::Field,
            _ => theme::ControlSurface::Window,
        }
    }

    fn prewarm_theme_resources() {
        let _ = theme::brush_for(theme::ControlSurface::Window);
        let _ = theme::brush_for(theme::ControlSurface::Panel);
        let _ = theme::brush_for(theme::ControlSurface::Field);
    }

    struct AppWindowState {
        runtime_options: RuntimeOptions,
        preference_store: PreferenceStore,
        preset_store: PresetStore,
        session_store: SessionStore,
        runtime: Option<WindowsRuntime>,
        runtime_error: Option<String>,
        webview2_error: Option<String>,
        templates: Vec<LayoutTemplate>,
        presets: Vec<WorkspacePreset>,
        suggestions: Vec<ProjectSuggestion>,
        preset_warning: Option<String>,
        asset_store: AssetStore,
        asset_warning: Option<String>,
        session: Option<SavedSession>,
        session_warning: Option<String>,
        workspace_path_hwnd: HWND,
        session_name_hwnd: HWND,
        template_list_hwnd: HWND,
        preset_list_hwnd: HWND,
        tile_count_hwnd: HWND,
        selection_summary_hwnd: HWND,
        suggestion_list_hwnd: HWND,
        status_hwnd: HWND,
        settings_window_hwnd: HWND,
        tray_icon_added: bool,
        window_hidden_to_tray: bool,
        quit_requested: bool,
        startup_resume_prompted: bool,
        selected_source: LaunchSelection,
        active_layout: LayoutNode,
        active_theme: ThemeMode,
        active_density: ApplicationDensity,
        save_preset_button_hwnd: HWND,
        update_preset_button_hwnd: HWND,
        delete_preset_button_hwnd: HWND,
        launch_preset_button_hwnd: HWND,
        launch_button_hwnd: HWND,
        apply_suggestion_button_hwnd: HWND,
        theme_combo_hwnd: HWND,
        density_combo_hwnd: HWND,
        edit_tiles_button_hwnd: HWND,
        assets_button_hwnd: HWND,
        palette_button_hwnd: HWND,
        companion_button_hwnd: HWND,
        launcher_editor_hwnd: HWND,
        controls_initializing: bool,
        controls_ready: bool,
        syncing_launcher_controls: bool,
        startup_init_completed: bool,
        startup_probe_running: bool,
        runtime_probe_preferred_distribution: Option<String>,
        companion_action_running: bool,
        companion_action_generation: u64,
    }

    struct StartupProbeResult {
        runtime: Option<WindowsRuntime>,
        runtime_error: Option<String>,
        webview2_error: Option<String>,
    }

    struct CompanionActionCompletion {
        generation: u64,
        label: String,
        result: Result<crate::extension::CompanionActionResult, String>,
    }

    struct PromptWindowState {
        done: bool,
        submitted: bool,
        prompt: String,
        placeholder: String,
        visible: bool,
        edit_hwnd: HWND,
        result: Option<String>,
    }

    struct SettingsWindowState {
        window_hwnd: HWND,
        parent_hwnd: HWND,
        product_info: ProductInfo,
        preference_store: PreferenceStore,
        theme_list_hwnd: HWND,
        density_list_hwnd: HWND,
        close_background_hwnd: HWND,
        distro_hwnd: HWND,
        runtime_status_hwnd: HWND,
        fullscreen_shortcut_hwnd: HWND,
        density_shortcut_hwnd: HWND,
        zoom_in_shortcut_hwnd: HWND,
        zoom_out_shortcut_hwnd: HWND,
        command_palette_shortcut_hwnd: HWND,
        voice_enabled_hwnd: HWND,
        voice_activation_hwnd: HWND,
        voice_microphone_hwnd: HWND,
        voice_microphone_ids: Vec<Option<String>>,
        voice_hotkey_hwnd: HWND,
        voice_engine_hwnd: HWND,
        voice_global_hwnd: HWND,
        voice_pack_status_hwnd: HWND,
        voice_pack_progress_hwnd: HWND,
        shortcut_status_hwnd: HWND,
        recording_shortcut: Option<ShortcutField>,
        current_fullscreen_shortcut: String,
        current_density_shortcut: String,
        current_zoom_in_shortcut: String,
        current_zoom_out_shortcut: String,
        current_command_palette_shortcut: String,
        current_voice_hotkey: String,
    }

    enum SettingsVoicePackEvent {
        Status(String),
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ShortcutField {
        Fullscreen,
        Density,
        ZoomIn,
        ZoomOut,
        CommandPalette,
        VoiceHotkey,
    }

    unsafe fn run_gui(options: RuntimeOptions) -> Result<ExitCode, String> {
        configure_windows_taskbar_identity(options.product.effective_windows_app_user_model_id());
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err("could not resolve module handle".into());
        }

        prewarm_theme_resources();
        register_window_classes(instance)?;

        let window_title = options.product.app_title.clone();
        let catalog = options.catalog.clone();
        let state = Box::new(AppWindowState {
            runtime_options: options,
            preference_store: PreferenceStore::new(),
            preset_store: PresetStore::new().with_catalog_provider(catalog.clone()),
            session_store: SessionStore::new(),
            asset_store: AssetStore::new().with_catalog_provider(catalog),
            runtime: None,
            runtime_error: None,
            webview2_error: None,
            templates: builtin_templates(),
            presets: Vec::new(),
            suggestions: Vec::new(),
            preset_warning: None,
            asset_warning: None,
            session: None,
            session_warning: None,
            workspace_path_hwnd: ptr::null_mut(),
            session_name_hwnd: ptr::null_mut(),
            template_list_hwnd: ptr::null_mut(),
            preset_list_hwnd: ptr::null_mut(),
            tile_count_hwnd: ptr::null_mut(),
            selection_summary_hwnd: ptr::null_mut(),
            suggestion_list_hwnd: ptr::null_mut(),
            status_hwnd: ptr::null_mut(),
            settings_window_hwnd: ptr::null_mut(),
            tray_icon_added: false,
            window_hidden_to_tray: false,
            quit_requested: false,
            startup_resume_prompted: false,
            selected_source: LaunchSelection::Template(0),
            active_layout: generate_layout(1),
            active_theme: ThemeMode::System,
            active_density: ApplicationDensity::Compact,
            save_preset_button_hwnd: ptr::null_mut(),
            update_preset_button_hwnd: ptr::null_mut(),
            delete_preset_button_hwnd: ptr::null_mut(),
            launch_preset_button_hwnd: ptr::null_mut(),
            launch_button_hwnd: ptr::null_mut(),
            apply_suggestion_button_hwnd: ptr::null_mut(),
            theme_combo_hwnd: ptr::null_mut(),
            density_combo_hwnd: ptr::null_mut(),
            edit_tiles_button_hwnd: ptr::null_mut(),
            assets_button_hwnd: ptr::null_mut(),
            palette_button_hwnd: ptr::null_mut(),
            companion_button_hwnd: ptr::null_mut(),
            launcher_editor_hwnd: ptr::null_mut(),
            controls_initializing: false,
            controls_ready: false,
            syncing_launcher_controls: false,
            startup_init_completed: false,
            startup_probe_running: false,
            runtime_probe_preferred_distribution: None,
            companion_action_running: false,
            companion_action_generation: 0,
        });
        let state_ptr = Box::into_raw(state);

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                wide(WINDOW_CLASS).as_ptr(),
                wide(&window_title).as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                920,
                620,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                state_ptr.cast(),
            )
        };

        if hwnd.is_null() {
            unsafe {
                drop(Box::from_raw(state_ptr));
            }
            return Err("CreateWindowExW returned null".into());
        }

        PRIMARY_SHELL_HWND.store(hwnd as isize, Ordering::Relaxed);

        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);
        }

        let mut message = unsafe { mem::zeroed::<MSG>() };
        while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        Ok(ExitCode::SUCCESS)
    }

    fn configure_windows_taskbar_identity(app_user_model_id: &str) {
        let app_user_model_id = wide(app_user_model_id);
        let status = unsafe { SetCurrentProcessExplicitAppUserModelID(app_user_model_id.as_ptr()) };
        if status < 0 {
            logging::error(format!(
                "failed to configure Windows AppUserModelID (HRESULT {status})"
            ));
        }
    }

    fn register_window_classes(instance: HINSTANCE) -> Result<(), String> {
        register_window_class(instance, WINDOW_CLASS, window_proc)?;
        register_window_class(instance, SETTINGS_WINDOW_CLASS, settings_window_proc)?;
        register_window_class(instance, COMPANION_PROMPT_CLASS, prompt_window_proc)
    }

    fn register_window_class(
        instance: HINSTANCE,
        class_name: &str,
        window_proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
    ) -> Result<(), String> {
        let class_name = wide(class_name);
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class_name.as_ptr(),
            hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
            hbrBackground: theme::brush_for(theme::ControlSurface::Window),
            ..unsafe { mem::zeroed() }
        };

        let atom = unsafe { RegisterClassW(&window_class) };
        if atom == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(1410) {
                return Err(format!("RegisterClassW failed: {error}"));
            }
        }

        Ok(())
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            crate::windows::win32_helpers::catch_window_proc(
                "window_proc",
                hwnd,
                message,
                wparam,
                lparam,
                || window_proc_impl(hwnd, message, wparam, lparam),
            )
        }
    }

    unsafe fn window_proc_impl(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_NCCREATE => {
                let create = lparam as *const CREATESTRUCTW;
                if create.is_null() {
                    return 0;
                }

                let state_ptr = unsafe { (*create).lpCreateParams as *mut AppWindowState };
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
                }
                1
            }
            WM_CREATE => {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    state.controls_initializing = true;
                    create_controls(hwnd, state);
                    state.controls_initializing = false;
                    state.controls_ready = true;
                    logging::info("Windows launcher window created");
                    if unsafe { PostMessageW(hwnd, WM_STARTUP_INIT, 0, 0) } == 0 {
                        logging::error(format!(
                            "failed to post Windows startup init request: {}",
                            std::io::Error::last_os_error()
                        ));
                        run_deferred_startup_init(hwnd, state);
                    }
                }
                0
            }
            WM_CLOSE => {
                if let Some(state) = unsafe { state_mut(hwnd) }
                    && should_hide_to_tray(state)
                    && !state.quit_requested
                {
                    hide_window_to_tray(hwnd, state);
                    return 0;
                }
                unsafe {
                    DestroyWindow(hwnd);
                }
                0
            }
            WM_SIZE => {
                if let Some(state) = unsafe { state_mut(hwnd) }
                    && state.controls_ready
                {
                    layout_controls(hwnd, state);
                }
                0
            }
            WM_KEYDOWN => {
                if let Some(state) = unsafe { state_mut(hwnd) }
                    && handle_shell_shortcuts(hwnd, state, wparam as u32)
                {
                    return 0;
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            WM_ERASEBKGND => {
                if theme::paint_window_background(hwnd, wparam as HDC) {
                    return 1;
                }
                0
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
                let surface = themed_surface_for_message(message);
                theme::apply_control_colors(wparam as HDC, surface, true) as isize
            }
            WM_TRAYICON => {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    handle_tray_event(hwnd, state, lparam as u32);
                }
                0
            }
            WM_STARTUP_INIT => {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    run_deferred_startup_init(hwnd, state);
                }
                0
            }
            WM_STARTUP_PROBE_REQUEST => {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    start_runtime_probe(
                        hwnd,
                        state,
                        state.runtime_probe_preferred_distribution.clone(),
                    );
                }
                0
            }
            WM_STARTUP_PROBE_COMPLETE => {
                if lparam != 0 {
                    let result = unsafe { Box::from_raw(lparam as *mut StartupProbeResult) };
                    if let Some(state) = unsafe { state_mut(hwnd) } {
                        apply_startup_probe_result(hwnd, state, *result);
                    }
                }
                0
            }
            WM_COMPANION_ACTION_COMPLETE => {
                if lparam != 0 {
                    let completion =
                        unsafe { Box::from_raw(lparam as *mut CompanionActionCompletion) };
                    if let Some(state) = unsafe { state_mut(hwnd) }
                        && state.companion_action_running
                        && completion.generation == state.companion_action_generation
                    {
                        state.companion_action_running = false;
                        match completion.result {
                            Ok(result) => {
                                if !matches!(
                                    result.refresh_scope,
                                    crate::extension::CompanionRefreshScope::Panel
                                ) {
                                    refresh_state(hwnd, state);
                                }
                                unsafe {
                                    MessageBoxW(
                                        hwnd,
                                        wide(&result.message).as_ptr(),
                                        wide(&completion.label).as_ptr(),
                                        MB_OK,
                                    );
                                }
                            }
                            Err(error) => unsafe {
                                MessageBoxW(
                                    hwnd,
                                    wide(&error).as_ptr(),
                                    wide(&completion.label).as_ptr(),
                                    MB_ICONWARNING | MB_OK,
                                );
                            },
                        }
                    }
                }
                0
            }
            WM_COMMAND => {
                let command_id = (wparam & 0xffff) as isize;
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    if state.controls_initializing
                        || !state.controls_ready
                        || state.syncing_launcher_controls
                    {
                        return 0;
                    }
                    let notification = ((wparam >> 16) & 0xffff) as u32;
                    match command_id {
                        ID_TEMPLATE_LIST if notification == LBN_SELCHANGE => {
                            state.selected_source = LaunchSelection::Template(
                                selected_listbox_index(state.template_list_hwnd),
                            );
                            apply_launcher_selection(state);
                        }
                        ID_TEMPLATE_LIST if notification == LBN_DBLCLK => {
                            state.selected_source = LaunchSelection::Template(
                                selected_listbox_index(state.template_list_hwnd),
                            );
                            apply_launcher_selection(state);
                            launch_selected_preset(hwnd, state);
                        }
                        ID_PRESET_LIST if notification == LBN_SELCHANGE => {
                            state.selected_source = LaunchSelection::Preset(
                                selected_listbox_index(state.preset_list_hwnd),
                            );
                            apply_launcher_selection(state);
                        }
                        ID_PRESET_LIST if notification == LBN_DBLCLK => {
                            state.selected_source = LaunchSelection::Preset(
                                selected_listbox_index(state.preset_list_hwnd),
                            );
                            apply_launcher_selection(state);
                            launch_selected_preset(hwnd, state);
                        }
                        ID_TILE_COUNT if notification == EN_CHANGE => {
                            sync_tile_count_from_input(state);
                        }
                        ID_THEME_COMBO if notification == CBN_SELCHANGE => {
                            sync_launch_appearance_from_controls(state);
                        }
                        ID_LAUNCH_DENSITY_COMBO if notification == CBN_SELCHANGE => {
                            sync_launch_appearance_from_controls(state);
                        }
                        ID_WORKSPACE_PATH if notification == EN_CHANGE => {
                            refresh_asset_warning(state);
                            refresh_suggestions(state);
                            sync_status_text(state);
                            sync_launcher_editor(state);
                        }
                        ID_LAUNCH_NAME if notification == EN_CHANGE => {
                            sync_status_text(state);
                        }
                        ID_SUGGESTION_LIST if notification == LBN_DBLCLK => {
                            apply_selected_suggestion(state);
                        }
                        ID_REFRESH => refresh_state(hwnd, state),
                        ID_APPLY_SUGGESTION => apply_selected_suggestion(state),
                        ID_EDIT_TILES => open_launcher_editor(hwnd, state),
                        ID_ASSETS_MANAGER => open_assets_manager(hwnd, state),
                        ID_COMMAND_PALETTE => open_command_palette(hwnd, state),
                        ID_COMPANION => show_companion_dialog(hwnd, state),
                        ID_SETTINGS => open_settings_dialog(hwnd, state),
                        ID_SAVE_PRESET => save_selected_preset_as_new(hwnd, state),
                        ID_UPDATE_PRESET => update_selected_preset(hwnd, state),
                        ID_DELETE_PRESET => delete_selected_preset(hwnd, state),
                        ID_LAUNCH_PRESET => launch_selected_preset(hwnd, state),
                        ID_LAUNCH => launch_restored_session(hwnd, state),
                        ID_QUIT => unsafe {
                            state.quit_requested = true;
                            DestroyWindow(hwnd);
                        },
                        _ => {}
                    }
                }
                0
            }
            WM_DESTROY => {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    remove_tray_icon(hwnd, state);
                }
                if PRIMARY_SHELL_HWND.load(Ordering::Relaxed) == hwnd as isize {
                    PRIMARY_SHELL_HWND.store(0, Ordering::Relaxed);
                }
                unsafe {
                    PostQuitMessage(0);
                }
                0
            }
            WM_NCDESTROY => {
                let state_ptr =
                    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) } as *mut AppWindowState;
                if !state_ptr.is_null() {
                    unsafe {
                        drop(Box::from_raw(state_ptr));
                    }
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    unsafe extern "system" fn settings_window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            crate::windows::win32_helpers::catch_window_proc(
                "settings_window_proc",
                hwnd,
                message,
                wparam,
                lparam,
                || settings_window_proc_impl(hwnd, message, wparam, lparam),
            )
        }
    }

    unsafe fn settings_window_proc_impl(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_NCCREATE => {
                let create = lparam as *const CREATESTRUCTW;
                if create.is_null() {
                    return 0;
                }

                let state_ptr = unsafe { (*create).lpCreateParams as *mut SettingsWindowState };
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
                }
                1
            }
            WM_CREATE => {
                if let Some(state) = unsafe { settings_state_mut(hwnd) } {
                    state.window_hwnd = hwnd;
                    create_settings_controls(hwnd, state);
                    refresh_settings_runtime_preview(state);
                }
                0
            }
            WM_SIZE => {
                if let Some(state) = unsafe { settings_state_mut(hwnd) } {
                    layout_settings_controls(hwnd, state);
                }
                0
            }
            WM_KEYDOWN => {
                if let Some(state) = unsafe { settings_state_mut(hwnd) }
                    && handle_settings_shortcut_capture(hwnd, state, wparam as u32)
                {
                    return 0;
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            WM_ERASEBKGND => {
                if theme::paint_window_background(hwnd, wparam as HDC) {
                    return 1;
                }
                0
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
                let surface = themed_surface_for_message(message);
                theme::apply_control_colors(wparam as HDC, surface, true) as isize
            }
            WM_COMMAND => {
                let command_id = (wparam & 0xffff) as isize;
                let notification = ((wparam >> 16) & 0xffff) as u32;
                if let Some(state) = unsafe { settings_state_mut(hwnd) } {
                    match command_id {
                        ID_SETTINGS_THEME_LIST if notification == LBN_SELCHANGE => {
                            apply_live_settings_change(
                                state,
                                "Default theme updated.",
                                false,
                                true,
                            );
                        }
                        ID_SETTINGS_DENSITY_LIST if notification == LBN_SELCHANGE => {
                            apply_live_settings_change(
                                state,
                                "Default application density updated.",
                                false,
                                true,
                            );
                        }
                        ID_SETTINGS_CLOSE_BACKGROUND if notification == BN_CLICKED => {
                            apply_live_settings_change(
                                state,
                                "Background behavior updated.",
                                false,
                                true,
                            );
                        }
                        ID_SETTINGS_WSL_DISTRO if notification == EN_CHANGE => {
                            apply_live_settings_change(
                                state,
                                "Preferred WSL distro updated. Use Check Runtime to verify.",
                                false,
                                true,
                            );
                        }
                        ID_SETTINGS_VOICE_ENABLED if notification == BN_CLICKED => {
                            apply_live_settings_change(
                                state,
                                "Voice input setting updated.",
                                false,
                                true,
                            );
                        }
                        ID_SETTINGS_VOICE_ACTIVATION if notification == CBN_SELCHANGE => {
                            apply_live_settings_change(
                                state,
                                "Voice activation mode updated.",
                                false,
                                true,
                            );
                        }
                        ID_SETTINGS_VOICE_MICROPHONE if notification == CBN_SELCHANGE => {
                            apply_live_settings_change(
                                state,
                                "Voice microphone preference updated.",
                                false,
                                true,
                            );
                        }
                        ID_SETTINGS_VOICE_ENGINE if notification == CBN_SELCHANGE => {
                            apply_live_settings_change(
                                state,
                                "Voice engine preference updated.",
                                false,
                                true,
                            );
                        }
                        ID_SETTINGS_VOICE_GLOBAL if notification == BN_CLICKED => {
                            apply_live_settings_change(
                                state,
                                "Voice hotkey preference updated.",
                                false,
                                true,
                            );
                        }
                        ID_SETTINGS_RESET => reset_settings(hwnd, state),
                        ID_SETTINGS_RESET_BUILTIN_PRESETS => {
                            reset_builtin_presets_from_settings(hwnd, state)
                        }
                        ID_SETTINGS_CLOSE => unsafe {
                            DestroyWindow(hwnd);
                        },
                        ID_SETTINGS_PROBE => refresh_settings_runtime_preview(state),
                        ID_SETTINGS_FULLSCREEN_RECORD => {
                            begin_shortcut_capture(hwnd, state, ShortcutField::Fullscreen)
                        }
                        ID_SETTINGS_DENSITY_RECORD => {
                            begin_shortcut_capture(hwnd, state, ShortcutField::Density)
                        }
                        ID_SETTINGS_ZOOM_IN_RECORD => {
                            begin_shortcut_capture(hwnd, state, ShortcutField::ZoomIn)
                        }
                        ID_SETTINGS_ZOOM_OUT_RECORD => {
                            begin_shortcut_capture(hwnd, state, ShortcutField::ZoomOut)
                        }
                        ID_SETTINGS_COMMAND_PALETTE_RECORD => {
                            begin_shortcut_capture(hwnd, state, ShortcutField::CommandPalette)
                        }
                        ID_SETTINGS_VOICE_RECORD => {
                            begin_shortcut_capture(hwnd, state, ShortcutField::VoiceHotkey)
                        }
                        ID_SETTINGS_VOICE_PACK_INSTALL => install_voice_pack_from_settings(state),
                        ID_SETTINGS_VOICE_PACK_HEALTH => check_voice_pack_from_settings(state),
                        ID_SETTINGS_VOICE_PACK_DELETE => delete_voice_pack_from_settings(state),
                        ID_SETTINGS_OPEN_LOGS_FOLDER => open_logs_folder_from_settings(state),
                        ID_SETTINGS_HELP_FULLSCREEN_SHORTCUT => {
                            show_shortcut_help(hwnd, ShortcutField::Fullscreen)
                        }
                        ID_SETTINGS_HELP_DENSITY_SHORTCUT => {
                            show_shortcut_help(hwnd, ShortcutField::Density)
                        }
                        ID_SETTINGS_HELP_ZOOM_IN_SHORTCUT => {
                            show_shortcut_help(hwnd, ShortcutField::ZoomIn)
                        }
                        ID_SETTINGS_HELP_ZOOM_OUT_SHORTCUT => {
                            show_shortcut_help(hwnd, ShortcutField::ZoomOut)
                        }
                        ID_SETTINGS_HELP_COMMAND_PALETTE_SHORTCUT => {
                            show_shortcut_help(hwnd, ShortcutField::CommandPalette)
                        }
                        _ => {}
                    }
                }
                0
            }
            WM_SETTINGS_VOICE_PACK_EVENT => {
                if lparam != 0 {
                    let event = unsafe { Box::from_raw(lparam as *mut SettingsVoicePackEvent) };
                    if let Some(state) = unsafe { settings_state_mut(hwnd) } {
                        match *event {
                            SettingsVoicePackEvent::Status(message) => {
                                set_settings_status(state, &message);
                                let preferences = state.preference_store.load();
                                apply_preferences_to_settings_controls(state, &preferences);
                            }
                        }
                    }
                }
                0
            }
            WM_DESTROY => {
                if let Some(state) = unsafe { settings_state_mut(hwnd) } {
                    let mut rect = unsafe { mem::zeroed() };
                    unsafe {
                        GetClientRect(hwnd, &mut rect);
                    }
                    state
                        .preference_store
                        .save_settings_dialog_size(rect.right - rect.left, rect.bottom - rect.top);
                }
                0
            }
            WM_NCDESTROY => {
                let state_ptr = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) }
                    as *mut SettingsWindowState;
                if !state_ptr.is_null() {
                    let parent_hwnd = unsafe { (*state_ptr).parent_hwnd };
                    if let Some(parent_state) = unsafe { state_mut(parent_hwnd) } {
                        parent_state.settings_window_hwnd = ptr::null_mut();
                    }
                    unsafe {
                        drop(Box::from_raw(state_ptr));
                    }
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    fn create_controls(hwnd: HWND, state: &mut AppWindowState) {
        let default_workspace = std::env::current_dir()
            .ok()
            .or_else(home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .display()
            .to_string();

        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Workspace root",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_LABEL_PATH,
        );
        state.workspace_path_hwnd = create_child_window(
            hwnd,
            "EDIT",
            &default_workspace,
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_LEFT as u32 | ES_AUTOHSCROLL as u32,
            0,
            ID_WORKSPACE_PATH,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Launch name",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_LABEL_NAME,
        );
        state.session_name_hwnd = create_child_window(
            hwnd,
            "EDIT",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_LEFT as u32 | ES_AUTOHSCROLL as u32,
            0,
            ID_LAUNCH_NAME,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Templates",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_LABEL_TEMPLATES,
        );
        state.template_list_hwnd = create_child_window(
            hwnd,
            "LISTBOX",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | WS_VSCROLL | LBS_NOTIFY as u32,
            0,
            ID_TEMPLATE_LIST,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Presets",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_LABEL_PRESETS,
        );
        state.preset_list_hwnd = create_child_window(
            hwnd,
            "LISTBOX",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | WS_VSCROLL | LBS_NOTIFY as u32,
            0,
            ID_PRESET_LIST,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Tile count",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_LABEL_TILE_COUNT,
        );
        state.tile_count_hwnd = create_child_window(
            hwnd,
            "EDIT",
            "1",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_LEFT as u32 | ES_AUTOHSCROLL as u32,
            0,
            ID_TILE_COUNT,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Theme",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_LABEL_THEME,
        );
        state.theme_combo_hwnd = create_combo_box(hwnd, ID_THEME_COMBO);
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Density",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_LABEL_LAUNCH_DENSITY,
        );
        state.density_combo_hwnd = create_combo_box(hwnd, ID_LAUNCH_DENSITY_COMBO);
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Selection summary",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_LABEL_SELECTION_SUMMARY,
        );
        state.selection_summary_hwnd = create_child_window(
            hwnd,
            "STATIC",
            "",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SELECTION_SUMMARY,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Suggestions",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_LABEL_SUGGESTIONS,
        );
        state.suggestion_list_hwnd = create_child_window(
            hwnd,
            "LISTBOX",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | WS_VSCROLL | LBS_NOTIFY as u32,
            0,
            ID_SUGGESTION_LIST,
        );
        state.apply_suggestion_button_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Apply Suggestion",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_APPLY_SUGGESTION,
        );
        state.edit_tiles_button_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Edit Tiles",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_EDIT_TILES,
        );
        state.status_hwnd = create_child_window(
            hwnd,
            "EDIT",
            "",
            WS_CHILD
                | WS_VISIBLE
                | WS_BORDER
                | WS_VSCROLL
                | ES_LEFT as u32
                | ES_MULTILINE as u32
                | ES_AUTOVSCROLL as u32
                | ES_READONLY as u32,
            0,
            ID_STATUS,
        );
        state.launch_preset_button_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Launch Workspace",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_LAUNCH_PRESET,
        );
        state.save_preset_button_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Save as Preset",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SAVE_PRESET,
        );
        state.update_preset_button_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Update Preset",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_UPDATE_PRESET,
        );
        state.delete_preset_button_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Delete Preset",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_DELETE_PRESET,
        );
        state.launch_button_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Open Restored Workspaces",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_LAUNCH,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Refresh Runtime",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_REFRESH,
        );
        state.assets_button_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Assets Manager",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_ASSETS_MANAGER,
        );
        state.palette_button_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Command Palette",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_COMMAND_PALETTE,
        );
        if state.runtime_options.companion.is_some() {
            state.companion_button_hwnd = create_child_window(
                hwnd,
                "BUTTON",
                "Account / Sync",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
                0,
                ID_COMPANION,
            );
        }
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Settings",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Quit",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_QUIT,
        );

        let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
        for control in [
            unsafe { GetDlgItem(hwnd, ID_LABEL_PATH as i32) },
            state.workspace_path_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_NAME as i32) },
            state.session_name_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_TILE_COUNT as i32) },
            state.tile_count_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_THEME as i32) },
            state.theme_combo_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_LAUNCH_DENSITY as i32) },
            state.density_combo_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_SELECTION_SUMMARY as i32) },
            state.selection_summary_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_SUGGESTIONS as i32) },
            state.suggestion_list_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_TEMPLATES as i32) },
            state.template_list_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_PRESETS as i32) },
            state.preset_list_hwnd,
            state.apply_suggestion_button_hwnd,
            state.edit_tiles_button_hwnd,
            state.status_hwnd,
            state.save_preset_button_hwnd,
            state.update_preset_button_hwnd,
            state.delete_preset_button_hwnd,
            state.launch_preset_button_hwnd,
            state.launch_button_hwnd,
            unsafe { GetDlgItem(hwnd, ID_REFRESH as i32) },
            state.assets_button_hwnd,
            state.palette_button_hwnd,
            state.companion_button_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS as i32) },
            unsafe { GetDlgItem(hwnd, ID_QUIT as i32) },
        ] {
            if !control.is_null() {
                unsafe {
                    SendMessageW(control, WM_SETFONT, font as usize, 1);
                }
            }
        }

        populate_combo_box_items(state.theme_combo_hwnd, &["System", "Light", "Dark"]);
        populate_combo_box_items(
            state.density_combo_hwnd,
            &["Comfortable", "Standard", "Compact"],
        );
        populate_template_list(state);
        populate_suggestion_list(state);
        layout_controls(hwnd, state);
    }

    fn layout_controls(hwnd: HWND, state: &AppWindowState) {
        let mut rect = unsafe { mem::zeroed() };
        unsafe {
            GetClientRect(hwnd, &mut rect);
        }

        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let content_width = width - (MARGIN * 2);
        let workspace_label_y = MARGIN;
        let workspace_edit_y = workspace_label_y + LABEL_HEIGHT + 4;
        let name_label_y = workspace_edit_y + FIELD_HEIGHT + 10;
        let name_edit_y = name_label_y + LABEL_HEIGHT + 4;
        let tile_count_label_y = name_edit_y + FIELD_HEIGHT + 10;
        let tile_count_edit_y = tile_count_label_y + LABEL_HEIGHT + 4;
        let theme_label_y = tile_count_label_y;
        let theme_combo_y = tile_count_edit_y;
        let density_label_y = tile_count_label_y;
        let density_combo_y = tile_count_edit_y;
        let summary_label_y = tile_count_edit_y + FIELD_HEIGHT + 12;
        let summary_y = summary_label_y + LABEL_HEIGHT + 4;
        let lists_label_y = summary_y + FIELD_HEIGHT + 12;
        let list_y = lists_label_y + LABEL_HEIGHT + 4;
        let column_gap = 12;
        let column_width = ((content_width - column_gap) / 2).max(180);
        let preset_actions_y = list_y + LIST_HEIGHT + 12;
        let suggestions_label_y = preset_actions_y + BUTTON_HEIGHT + 12;
        let suggestions_y = suggestions_label_y + LABEL_HEIGHT + 4;
        let suggestions_height = 96;
        let suggestions_button_y = suggestions_y + suggestions_height + 10;
        let button_y = height - MARGIN - BUTTON_HEIGHT;
        let status_y = suggestions_button_y + BUTTON_HEIGHT + 12;
        let status_height = (button_y - status_y - 12).max(88);
        let appearance_label_x = MARGIN + 96;
        let appearance_field_x = appearance_label_x;
        let combo_width = 148;
        let density_label_x = appearance_field_x + combo_width + 20;
        let density_field_x = density_label_x;

        unsafe {
            SetWindowPos(
                GetDlgItem(hwnd, ID_LABEL_PATH as i32),
                ptr::null_mut(),
                MARGIN,
                workspace_label_y,
                content_width,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.workspace_path_hwnd,
                ptr::null_mut(),
                MARGIN,
                workspace_edit_y,
                content_width,
                FIELD_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_LABEL_NAME as i32),
                ptr::null_mut(),
                MARGIN,
                name_label_y,
                content_width,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.session_name_hwnd,
                ptr::null_mut(),
                MARGIN,
                name_edit_y,
                content_width,
                FIELD_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_LABEL_TILE_COUNT as i32),
                ptr::null_mut(),
                MARGIN,
                tile_count_label_y,
                96,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.tile_count_hwnd,
                ptr::null_mut(),
                MARGIN,
                tile_count_edit_y,
                72,
                FIELD_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_LABEL_THEME as i32),
                ptr::null_mut(),
                appearance_label_x,
                theme_label_y,
                combo_width,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.theme_combo_hwnd,
                ptr::null_mut(),
                appearance_field_x,
                theme_combo_y,
                combo_width,
                FIELD_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_LABEL_LAUNCH_DENSITY as i32),
                ptr::null_mut(),
                density_label_x,
                density_label_y,
                combo_width,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.density_combo_hwnd,
                ptr::null_mut(),
                density_field_x,
                density_combo_y,
                combo_width,
                FIELD_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_LABEL_SELECTION_SUMMARY as i32),
                ptr::null_mut(),
                MARGIN,
                summary_label_y,
                content_width,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.selection_summary_hwnd,
                ptr::null_mut(),
                MARGIN,
                summary_y,
                content_width,
                FIELD_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_LABEL_TEMPLATES as i32),
                ptr::null_mut(),
                MARGIN,
                lists_label_y,
                column_width,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.template_list_hwnd,
                ptr::null_mut(),
                MARGIN,
                list_y,
                column_width,
                LIST_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_LABEL_PRESETS as i32),
                ptr::null_mut(),
                MARGIN + column_width + column_gap,
                lists_label_y,
                column_width,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.preset_list_hwnd,
                ptr::null_mut(),
                MARGIN + column_width + column_gap,
                list_y,
                column_width,
                LIST_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.save_preset_button_hwnd,
                ptr::null_mut(),
                MARGIN,
                preset_actions_y,
                BUTTON_WIDTH,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.update_preset_button_hwnd,
                ptr::null_mut(),
                MARGIN + BUTTON_WIDTH + 12,
                preset_actions_y,
                BUTTON_WIDTH,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.delete_preset_button_hwnd,
                ptr::null_mut(),
                MARGIN + (BUTTON_WIDTH * 2) + 24,
                preset_actions_y,
                BUTTON_WIDTH - 24,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.status_hwnd,
                ptr::null_mut(),
                MARGIN,
                status_y,
                content_width,
                status_height,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_LABEL_SUGGESTIONS as i32),
                ptr::null_mut(),
                MARGIN,
                suggestions_label_y,
                content_width,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.suggestion_list_hwnd,
                ptr::null_mut(),
                MARGIN,
                suggestions_y,
                content_width,
                suggestions_height,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.apply_suggestion_button_hwnd,
                ptr::null_mut(),
                MARGIN,
                suggestions_button_y,
                BUTTON_WIDTH,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.edit_tiles_button_hwnd,
                ptr::null_mut(),
                MARGIN + BUTTON_WIDTH + 12,
                suggestions_button_y,
                BUTTON_WIDTH - 8,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_REFRESH as i32),
                ptr::null_mut(),
                MARGIN,
                button_y,
                BUTTON_WIDTH - 12,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.assets_button_hwnd,
                ptr::null_mut(),
                MARGIN + BUTTON_WIDTH,
                button_y,
                BUTTON_WIDTH,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.palette_button_hwnd,
                ptr::null_mut(),
                MARGIN + (BUTTON_WIDTH * 2) + 12,
                button_y,
                BUTTON_WIDTH,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.launch_preset_button_hwnd,
                ptr::null_mut(),
                MARGIN + (BUTTON_WIDTH * 3) + 24,
                button_y,
                BUTTON_WIDTH + 12,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.launch_button_hwnd,
                ptr::null_mut(),
                MARGIN + (BUTTON_WIDTH * 4) + 48,
                button_y,
                BUTTON_WIDTH + 24,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_QUIT as i32),
                ptr::null_mut(),
                width - MARGIN - 96,
                button_y,
                96,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            if !state.companion_button_hwnd.is_null() {
                SetWindowPos(
                    state.companion_button_hwnd,
                    ptr::null_mut(),
                    width - MARGIN - 96 - 108 - 144,
                    button_y,
                    132,
                    BUTTON_HEIGHT,
                    SWP_NOZORDER,
                );
            }
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS as i32),
                ptr::null_mut(),
                width - MARGIN - 96 - 108,
                button_y,
                96,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
        }
    }

    fn run_deferred_startup_init(hwnd: HWND, state: &mut AppWindowState) {
        if state.startup_init_completed {
            logging::info("skipped Windows startup init because it already completed");
            return;
        }
        state.startup_init_completed = true;
        logging::info("Windows startup init begin");
        install_tray_icon(hwnd, state);
        refresh_state(hwnd, state);
        logging::info("Windows startup init complete");
    }

    fn refresh_state(hwnd: HWND, state: &mut AppWindowState) {
        logging::info("Windows shell state refresh begin");
        let preferences = state.preference_store.load();
        let preferred_distribution = preferences.windows_wsl_distribution.clone();
        state.runtime = None;
        state.runtime_error = None;
        state.webview2_error = None;

        state.preset_store.ensure_seeded();
        logging::info("Windows startup presets seeded");
        state.asset_store.ensure_seeded();
        logging::info("Windows startup assets seeded");
        let preset_outcome = state.preset_store.load_presets_with_status();
        state.presets = preset_outcome.presets;
        state.preset_warning = preset_outcome.warning;
        logging::info("Windows startup presets loaded");

        refresh_asset_warning(state);
        refresh_suggestions(state);
        logging::info("Windows startup assets and suggestions refreshed");

        let was_syncing = state.syncing_launcher_controls;
        state.syncing_launcher_controls = true;
        logging::info("Windows startup preset list population begin");
        populate_preset_list(state);
        logging::info("Windows startup preset list population complete");
        logging::info("Windows startup suggestion list population begin");
        populate_suggestion_list(state);
        logging::info("Windows startup suggestion list population complete");
        logging::info("Windows startup launcher selection apply begin");
        apply_launcher_selection(state);
        logging::info("Windows startup launcher selection apply complete");
        state.syncing_launcher_controls = was_syncing;
        logging::info("Windows startup lists populated and launcher selection applied");

        let session_outcome = state.session_store.load_with_status();
        state.session = session_outcome.session;
        state.session_warning = session_outcome.warning;
        logging::info("Windows startup session loaded");

        unsafe {
            sync_status_text(state);
            EnableWindow(
                state.launch_preset_button_hwnd,
                can_launch_selected_preset(state) as i32,
            );
            EnableWindow(
                state.launch_button_hwnd,
                can_launch_saved_session(state) as i32,
            );
            EnableWindow(
                state.apply_suggestion_button_hwnd,
                (!state.suggestions.is_empty()) as i32,
            );
            EnableWindow(
                state.edit_tiles_button_hwnd,
                has_launcher_selection(state) as i32,
            );
        }
        logging::info("Windows startup controls enabled");
        sync_tray_tooltip(hwnd, state);
        logging::info("Windows startup tray tooltip synced");

        state.runtime_probe_preferred_distribution = preferred_distribution;
        if unsafe { PostMessageW(hwnd, WM_STARTUP_PROBE_REQUEST, 0, 0) } == 0 {
            logging::error(format!(
                "failed to post Windows runtime probe request: {}",
                std::io::Error::last_os_error()
            ));
            start_runtime_probe(
                hwnd,
                state,
                state.runtime_probe_preferred_distribution.clone(),
            );
        }
        logging::info("Windows startup runtime probe posted");
        logging::info("refreshed Windows shell state without blocking runtime probes");
    }

    fn start_runtime_probe(
        hwnd: HWND,
        state: &mut AppWindowState,
        preferred_distribution: Option<String>,
    ) {
        if state.startup_probe_running {
            logging::info("skipped Windows runtime probe because one is already running");
            return;
        }
        state.startup_probe_running = true;
        unsafe {
            SetWindowTextW(
                state.status_hwnd,
                wide("Checking WSL/PowerShell and WebView2 runtime availability...").as_ptr(),
            );
        }

        let hwnd_value = hwnd as isize;
        thread::spawn(move || {
            let webview2_error = workspace::probe_webview2_runtime().err();
            let (runtime, runtime_error) =
                match wsl::probe_runtime(preferred_distribution.as_deref()) {
                    Ok(runtime) => (Some(runtime), None),
                    Err(error) => (None, Some(error)),
                };
            let result = Box::new(StartupProbeResult {
                runtime,
                runtime_error,
                webview2_error,
            });
            let result_ptr = Box::into_raw(result);
            let posted = unsafe {
                PostMessageW(
                    hwnd_value as HWND,
                    WM_STARTUP_PROBE_COMPLETE,
                    0,
                    result_ptr as LPARAM,
                )
            };
            if posted == 0 {
                logging::error(format!(
                    "failed to post Windows runtime probe completion: {}",
                    std::io::Error::last_os_error()
                ));
                unsafe {
                    drop(Box::from_raw(result_ptr));
                }
            }
        });
    }

    fn apply_startup_probe_result(
        hwnd: HWND,
        state: &mut AppWindowState,
        result: StartupProbeResult,
    ) {
        state.startup_probe_running = false;
        state.runtime = result.runtime;
        state.runtime_error = result.runtime_error;
        state.webview2_error = result.webview2_error;

        unsafe {
            sync_status_text(state);
            EnableWindow(
                state.launch_preset_button_hwnd,
                can_launch_selected_preset(state) as i32,
            );
            EnableWindow(
                state.launch_button_hwnd,
                can_launch_saved_session(state) as i32,
            );
        }
        sync_tray_tooltip(hwnd, state);
        maybe_prompt_startup_resume(hwnd, state);
        logging::info("completed Windows runtime probes");
    }

    fn current_workspace_root(state: &AppWindowState) -> Option<PathBuf> {
        let workspace_root_input = read_window_text(state.workspace_path_hwnd);
        resolve_workspace_root(&PathBuf::from(workspace_root_input.trim())).ok()
    }

    fn preset_workspace_root(state: &AppWindowState) -> Option<PathBuf> {
        let workspace_root_input = read_window_text(state.workspace_path_hwnd);
        let workspace_root_input = workspace_root_input.trim();
        match resolve_workspace_root(&PathBuf::from(workspace_root_input)) {
            Ok(path) => Some(path),
            Err(error) => {
                logging::error(format!(
                    "could not resolve workspace root for preset snapshot: {error}"
                ));
                (!workspace_root_input.is_empty()).then(|| PathBuf::from(workspace_root_input))
            }
        }
    }

    fn current_launcher_assets(state: &AppWindowState) -> WorkspaceAssets {
        current_workspace_root(state)
            .map(|workspace_root| {
                state
                    .asset_store
                    .load_assets_for_workspace_root(&workspace_root)
                    .assets
            })
            .unwrap_or_default()
    }

    fn refresh_asset_warning(state: &mut AppWindowState) {
        state.asset_warning = current_workspace_root(state).and_then(|workspace_root| {
            state
                .asset_store
                .load_assets_for_workspace_root(&workspace_root)
                .warning
        });
    }

    fn maybe_prompt_startup_resume(hwnd: HWND, state: &mut AppWindowState) {
        if state.startup_resume_prompted {
            return;
        }
        state.startup_resume_prompted = true;

        if state.runtime.is_none() || state.session.is_none() {
            return;
        }
        let saved_session = state.session.clone().expect("checked above");
        let restore_mode = state.preference_store.load().default_restore_mode;

        match restore_mode {
            RestoreLaunchMode::Prompt => {
                let action = restore_prompt::present(
                    hwnd,
                    saved_session.tabs.len(),
                    state.session_warning.as_deref(),
                )
                .unwrap_or_else(|error| {
                    logging::error(format!("could not show restore prompt: {error}"));
                    RestoreStartupAction::StartFresh
                });
                if let Some(session) = session_for_startup_action(&saved_session, action) {
                    launch_saved_session(
                        state,
                        &session,
                        "restored",
                        action == RestoreStartupAction::ResumeAndRerun,
                    );
                    return;
                }
                clear_saved_startup_session(state);
            }
            RestoreLaunchMode::RerunStartupCommands | RestoreLaunchMode::ShellOnly => {
                if let Some(session) = session_for_restore_mode(&saved_session, restore_mode) {
                    launch_saved_session(
                        state,
                        &session,
                        "restored",
                        restore_mode == RestoreLaunchMode::RerunStartupCommands,
                    );
                }
            }
        }
    }

    fn launch_selected_preset(_hwnd: HWND, state: &mut AppWindowState) {
        let Some(runtime) = state.runtime.as_ref() else {
            return;
        };
        let Some(mut preset) = launcher_preset_snapshot(state) else {
            return;
        };
        if let Err(message) = require_webview2_for_preset(state, &preset) {
            unsafe {
                SetWindowTextW(state.status_hwnd, wide(&message).as_ptr());
            }
            logging::error(&message);
            return;
        }

        let workspace_root_input = read_window_text(state.workspace_path_hwnd);
        let workspace_root =
            match resolve_workspace_root(&PathBuf::from(workspace_root_input.trim())) {
                Ok(path) => path,
                Err(error) => {
                    let status = format!("Could not resolve workspace root:\r\n{error}");
                    unsafe {
                        SetWindowTextW(state.status_hwnd, wide(&status).as_ptr());
                    }
                    logging::error(format!("could not resolve workspace root: {error}"));
                    return;
                }
            };
        preset.workspace_root = Some(workspace_root.clone());

        let launch_name = read_window_text(state.session_name_hwnd);
        let trimmed_launch_name = launch_name.trim();
        let custom_title = (!trimmed_launch_name.is_empty() && trimmed_launch_name != preset.name)
            .then(|| trimmed_launch_name.to_string());

        let session = SavedSession {
            tabs: vec![crate::storage::session_store::SavedTab {
                preset: preset.clone(),
                workspace_root,
                custom_title,
                terminal_zoom_steps: 0,
                terminal_history: Vec::new(),
            }],
            active_tab_index: 0,
        };

        match wsl::collect_session_launch_commands(&session, runtime) {
            Ok(_) => match workspace::open_saved_workspaces(&session, runtime) {
                Ok((window_count, pane_count)) => {
                    let source_label = selected_source_label(state);
                    let status = format!(
                        "Opened {} new workspace window(s) with {} pane(s) from {} '{}' using {}.",
                        window_count,
                        pane_count,
                        source_label,
                        preset.name,
                        runtime.label()
                    );
                    unsafe {
                        SetWindowTextW(state.status_hwnd, wide(&status).as_ptr());
                    }
                    logging::info(format!(
                        "opened {} new workspace window(s) with {} pane(s) from {} '{}' using {}",
                        window_count,
                        pane_count,
                        source_label,
                        preset.name,
                        runtime.label()
                    ));
                }
                Err(error) => {
                    let status = format!("Could not open preset workspace:\r\n{error}");
                    unsafe {
                        SetWindowTextW(state.status_hwnd, wide(&status).as_ptr());
                    }
                    logging::error(format!("could not open preset workspace: {error}"));
                }
            },
            Err(error) => {
                let status = format!("Could not prepare preset launch:\r\n{error}");
                unsafe {
                    SetWindowTextW(state.status_hwnd, wide(&status).as_ptr());
                }
                logging::error(format!("could not prepare preset launch: {error}"));
            }
        }
    }

    fn restore_startup_overrides_for_session(
        session: &SavedSession,
    ) -> RestoreStartupOverridesByTab {
        restore_startup_overrides_for_saved_session(session)
    }

    fn launch_saved_session(
        state: &mut AppWindowState,
        session: &SavedSession,
        label: &str,
        apply_agent_resume_overrides: bool,
    ) {
        let Some(runtime) = state.runtime.as_ref() else {
            return;
        };
        if let Err(message) = require_webview2_for_session(state, session) {
            unsafe {
                SetWindowTextW(state.status_hwnd, wide(&message).as_ptr());
            }
            logging::error(&message);
            return;
        }

        let restore_startup_overrides = if apply_agent_resume_overrides {
            restore_startup_overrides_for_session(session)
        } else {
            RestoreStartupOverridesByTab::new()
        };

        match wsl::collect_session_launch_commands_with_restore_overrides(
            session,
            runtime,
            &restore_startup_overrides,
        ) {
            Ok(_) => match workspace::open_saved_workspaces_with_restore_overrides(
                session,
                runtime,
                restore_startup_overrides,
            ) {
                Ok((window_count, pane_count)) => {
                    let status = format!(
                        "Opened {} workspace window(s) with {} owned pane(s) using {}.",
                        window_count,
                        pane_count,
                        runtime.label()
                    );
                    unsafe {
                        SetWindowTextW(state.status_hwnd, wide(&status).as_ptr());
                    }
                    logging::info(format!(
                        "opened {} {label} Windows workspace host window(s) with {} pane(s) using {}",
                        window_count,
                        pane_count,
                        runtime.label()
                    ));
                }
                Err(error) => {
                    let status = format!("Could not open restored workspaces:\r\n{error}");
                    unsafe {
                        SetWindowTextW(state.status_hwnd, wide(&status).as_ptr());
                    }
                    logging::error(format!("could not open restored workspaces: {error}"));
                }
            },
            Err(error) => {
                let status = format!("Could not prepare restored session launch:\r\n{error}");
                unsafe {
                    SetWindowTextW(state.status_hwnd, wide(&status).as_ptr());
                }
                logging::error(format!(
                    "could not prepare restored session launch: {error}"
                ));
            }
        }
    }

    fn launch_restored_session(_hwnd: HWND, state: &mut AppWindowState) {
        let Some(session) = state.session.clone() else {
            return;
        };
        launch_saved_session(state, &session, "restored", false);
    }

    fn clear_saved_startup_session(state: &mut AppWindowState) {
        state.session_store.clear();
        state.session = None;
        state.session_warning = None;
        unsafe {
            sync_status_text(state);
            EnableWindow(state.launch_button_hwnd, 0);
        }
        logging::info("cleared saved Windows session at startup");
    }

    fn build_status_text(state: &AppWindowState, preferred_distribution: Option<&str>) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "{} Windows shell",
            state.runtime_options.product.display_name
        ));
        if let Some(license) = state.runtime_options.product.license_name.as_deref() {
            lines.push(format!("License: {license}"));
        }
        if let Some(source) = state.runtime_options.product.source_url.as_deref() {
            lines.push(format!("Source: {source}"));
        }
        lines.push(theme::accessibility_summary());
        lines.push(String::new());

        if let Some(runtime) = state.runtime.as_ref() {
            lines.push(format!("Active runtime: {}", runtime.label()));
            lines.push(format!("Runtime status: {}", runtime.selection_reason()));
            if let WindowsRuntime::Wsl(runtime) = runtime {
                lines.push(format!(
                    "Installed distros: {}",
                    runtime
                        .distributions
                        .iter()
                        .map(|distribution| distribution.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        } else {
            lines.push("Active runtime: unavailable".into());
            if let Some(preferred_distribution) = preferred_distribution {
                lines.push(format!(
                    "Configured WSL preference: {}",
                    preferred_distribution
                ));
            }
            if let Some(error) = state.runtime_error.as_deref() {
                lines.push(format!("Runtime status: {}", error));
            }
        }

        lines.push(String::new());
        if let Some(error) = state.webview2_error.as_deref() {
            lines.push("Browser runtime: unavailable".into());
            lines.push(
                "Web tiles require Microsoft Edge WebView2 Runtime and cannot be launched until it is installed."
                    .into(),
            );
            lines.push(format!("Browser status: {}", error));
        } else {
            lines.push("Browser runtime: WebView2 available".into());
        }

        lines.push(String::new());
        lines.push(format!(
            "Workspace root: {}",
            read_window_text(state.workspace_path_hwnd).trim()
        ));
        let launch_name = read_window_text(state.session_name_hwnd);
        if !launch_name.trim().is_empty() {
            lines.push(format!("Launch name: {}", launch_name.trim()));
        }
        lines.push(format!(
            "Selection summary: {}",
            build_selection_summary_text(state)
        ));
        lines.push(String::new());
        lines.push(format!("Available templates: {}", state.templates.len()));
        if let Some(template) = selected_template(state) {
            lines.push(format!(
                "Selected template: {} ({})",
                template.label, template.subtitle
            ));
        }
        lines.push(format!("Available presets: {}", state.presets.len()));
        if let Some(preset) = selected_preset(state) {
            lines.push(format!(
                "Selected preset: {} ({} tiles)",
                preset.name,
                preset.layout.tile_specs().len()
            ));
        }
        lines.push(format!(
            "Launcher selection: {}",
            match state.selected_source {
                LaunchSelection::Template(_) => "template",
                LaunchSelection::Preset(_) => "preset",
            }
        ));
        lines.push(format!(
            "Active tile count: {}",
            state.active_layout.tile_count()
        ));
        if let Some(warning) = state.preset_warning.as_deref() {
            lines.push(format!("Preset warning: {}", warning));
        }
        if let Some(warning) = state.asset_warning.as_deref() {
            lines.push(format!("Asset warning: {}", warning));
        }

        lines.push(String::new());
        if let Some(session) = state.session.as_ref() {
            let tile_count = session
                .tabs
                .iter()
                .map(|tab| tab.preset.layout.tile_specs().len())
                .sum::<usize>();
            lines.push(format!(
                "Restorable workspace tabs: {} ({} tiles total)",
                session.tabs.len(),
                tile_count
            ));
            for tab in &session.tabs {
                lines.push(format!(
                    "- {} [{}]",
                    tab.preset.name,
                    tab.workspace_root.display()
                ));
            }
        } else {
            lines.push("Restorable workspace tabs: none".into());
        }

        if let Some(warning) = state.session_warning.as_deref() {
            lines.push(String::new());
            lines.push("Session warning:".into());
            lines.push(warning.into());
        }

        lines.push(String::new());
        lines.push(format!(
            "Tray status: {}",
            if state.tray_icon_added {
                if state.window_hidden_to_tray {
                    "available, window hidden to background"
                } else {
                    "available"
                }
            } else {
                "unavailable"
            }
        ));
        lines.push(format!(
            "Close-to-background: {}",
            if state.preference_store.load().close_to_background {
                if state.tray_icon_added {
                    "enabled"
                } else {
                    "enabled, but tray is unavailable so close will still quit"
                }
            } else {
                "disabled"
            }
        ));

        if selected_launcher_requires_webview2(state) {
            lines.push(format!(
                "Selected launch includes web tiles: {}",
                if state.webview2_error.is_none() {
                    "ready"
                } else {
                    "blocked until WebView2 is installed"
                }
            ));
        }

        if let Some(session) = state.session.as_ref()
            && session_requires_webview2(session)
        {
            lines.push(format!(
                "Restored session includes web tiles: {}",
                if state.webview2_error.is_none() {
                    "ready"
                } else {
                    "blocked until WebView2 is installed"
                }
            ));
        }

        lines.push(String::new());
        lines.push("Actions:".into());
        lines.push(
            "- Refresh Runtime reloads WSL/PowerShell availability and saved session state.".into(),
        );
        lines.push(
            "- Launch Workspace opens a new native workspace window from the selected template or preset using the current tile count."
                .into(),
        );
        lines.push(
            "- Save as Preset stores the current launcher selection, using the Launch name field as the preset name when provided."
                .into(),
        );
        lines.push(
            "- Update Preset rewrites the selected custom preset, while builtin presets are copied instead of modified in place. Templates can only be saved as new presets."
                .into(),
        );
        lines.push(
            "- Open Restored Workspaces opens the restored session inside one native workspace host window with Windows-managed tabs."
                .into(),
        );

        lines.join("\r\n")
    }

    fn layout_requires_webview2(layout: &LayoutNode) -> bool {
        layout
            .tile_specs()
            .into_iter()
            .any(|tile| tile.tile_kind == TileKind::WebView)
    }

    fn session_requires_webview2(session: &SavedSession) -> bool {
        session
            .tabs
            .iter()
            .any(|tab| layout_requires_webview2(&tab.preset.layout))
    }

    fn selected_launcher_requires_webview2(state: &AppWindowState) -> bool {
        layout_requires_webview2(&state.active_layout)
    }

    fn can_launch_selected_preset(state: &AppWindowState) -> bool {
        state.runtime.is_some()
            && has_launcher_selection(state)
            && (!selected_launcher_requires_webview2(state) || state.webview2_error.is_none())
    }

    fn can_launch_saved_session(state: &AppWindowState) -> bool {
        state.runtime.is_some()
            && state.session.as_ref().is_some_and(|session| {
                !session_requires_webview2(session) || state.webview2_error.is_none()
            })
    }

    fn require_webview2_for_preset(
        state: &AppWindowState,
        preset: &WorkspacePreset,
    ) -> Result<(), String> {
        if layout_requires_webview2(&preset.layout)
            && let Some(error) = state.webview2_error.as_deref()
        {
            return Err(format!(
                "Cannot launch '{}' because it includes web tiles and Microsoft Edge WebView2 Runtime is unavailable.\r\n\r\n{}",
                preset.name, error
            ));
        }
        Ok(())
    }

    fn require_webview2_for_session(
        state: &AppWindowState,
        session: &SavedSession,
    ) -> Result<(), String> {
        if session_requires_webview2(session)
            && let Some(error) = state.webview2_error.as_deref()
        {
            return Err(format!(
                "Cannot open the restored session because it includes web tiles and Microsoft Edge WebView2 Runtime is unavailable.\r\n\r\n{}",
                error
            ));
        }
        Ok(())
    }

    fn install_tray_icon(hwnd: HWND, state: &mut AppWindowState) {
        let mut notify = tray_icon_data(hwnd);
        fill_wide_buffer(&mut notify.szTip, &state.runtime_options.product.tray_title);
        let icon = unsafe { LoadIconW(ptr::null_mut(), IDI_APPLICATION) };
        if icon.is_null() {
            state.tray_icon_added = false;
            return;
        }
        notify.hIcon = icon;
        let added = unsafe { Shell_NotifyIconW(NIM_ADD, &notify) } != 0;
        state.tray_icon_added = added;
        if added {
            logging::info("installed Windows tray icon");
        } else {
            logging::error("failed to install Windows tray icon");
        }
    }

    fn remove_tray_icon(hwnd: HWND, state: &mut AppWindowState) {
        if !state.tray_icon_added {
            return;
        }
        let notify = tray_icon_data(hwnd);
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &notify);
        }
        state.tray_icon_added = false;
        state.window_hidden_to_tray = false;
    }

    fn should_hide_to_tray(state: &AppWindowState) -> bool {
        state.tray_icon_added && state.preference_store.load().close_to_background
    }

    fn hide_window_to_tray(hwnd: HWND, state: &mut AppWindowState) {
        unsafe {
            ShowWindow(hwnd, SW_HIDE);
        }
        state.window_hidden_to_tray = true;
        sync_tray_tooltip(hwnd, state);
        logging::info("hiding TerminalTiler shell window to tray");
    }

    fn restore_window_from_tray(hwnd: HWND, state: &mut AppWindowState) {
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
            UpdateWindow(hwnd);
        }
        state.window_hidden_to_tray = false;
        state.quit_requested = false;
        sync_tray_tooltip(hwnd, state);
        refresh_state(hwnd, state);
    }

    fn sync_tray_tooltip(hwnd: HWND, state: &AppWindowState) {
        if !state.tray_icon_added {
            return;
        }
        let mut notify = tray_icon_data(hwnd);
        let tooltip = if state.window_hidden_to_tray {
            format!(
                "{} (hidden to background)",
                state.runtime_options.product.tray_title
            )
        } else {
            state.runtime_options.product.tray_title.clone()
        };
        fill_wide_buffer(&mut notify.szTip, &tooltip);
        let icon = unsafe { LoadIconW(ptr::null_mut(), IDI_APPLICATION) };
        notify.hIcon = icon;
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &notify);
        }
    }

    fn handle_tray_event(hwnd: HWND, state: &mut AppWindowState, event: u32) {
        match event {
            WM_LBUTTONUP => restore_window_from_tray(hwnd, state),
            WM_RBUTTONUP => show_tray_menu(hwnd, state),
            _ => {}
        }
    }

    fn show_tray_menu(hwnd: HWND, state: &mut AppWindowState) {
        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            return;
        }

        unsafe {
            AppendMenuW(
                menu,
                MF_STRING,
                TRAY_MENU_SHOW,
                wide("Show / Restore").as_ptr(),
            );
            AppendMenuW(
                menu,
                MF_STRING,
                TRAY_MENU_SETTINGS,
                wide("Open Settings").as_ptr(),
            );
            AppendMenuW(menu, MF_STRING, TRAY_MENU_QUIT, wide("Quit").as_ptr());
        }

        let mut point = unsafe { mem::zeroed() };
        unsafe {
            GetCursorPos(&mut point);
            SetForegroundWindow(hwnd);
        }

        let selected = unsafe {
            TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON | TPM_RETURNCMD,
                point.x,
                point.y,
                0,
                hwnd,
                ptr::null(),
            )
        };

        match selected as usize {
            TRAY_MENU_SHOW => restore_window_from_tray(hwnd, state),
            TRAY_MENU_SETTINGS => {
                restore_window_from_tray(hwnd, state);
                open_settings_dialog(hwnd, state);
            }
            TRAY_MENU_QUIT => {
                state.quit_requested = true;
                state.window_hidden_to_tray = false;
                sync_tray_tooltip(hwnd, state);
                unsafe {
                    DestroyWindow(hwnd);
                }
            }
            _ => {}
        }

        unsafe {
            DestroyMenu(menu);
        }
    }

    fn tray_icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            uFlags: NIF_MESSAGE | NIF_TIP | NIF_ICON,
            uCallbackMessage: WM_TRAYICON,
            ..unsafe { mem::zeroed() }
        }
    }

    fn open_settings_dialog(parent_hwnd: HWND, state: &mut AppWindowState) {
        if !state.settings_window_hwnd.is_null() {
            unsafe {
                ShowWindow(state.settings_window_hwnd, SW_SHOW);
            }
            return;
        }

        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            logging::error("could not resolve module handle for settings window");
            return;
        }

        let preferences = state.preference_store.load();
        let settings_state = Box::new(SettingsWindowState {
            window_hwnd: ptr::null_mut(),
            parent_hwnd,
            product_info: state.runtime_options.product.clone(),
            preference_store: state.preference_store.clone(),
            theme_list_hwnd: ptr::null_mut(),
            density_list_hwnd: ptr::null_mut(),
            close_background_hwnd: ptr::null_mut(),
            distro_hwnd: ptr::null_mut(),
            runtime_status_hwnd: ptr::null_mut(),
            fullscreen_shortcut_hwnd: ptr::null_mut(),
            density_shortcut_hwnd: ptr::null_mut(),
            zoom_in_shortcut_hwnd: ptr::null_mut(),
            zoom_out_shortcut_hwnd: ptr::null_mut(),
            command_palette_shortcut_hwnd: ptr::null_mut(),
            voice_enabled_hwnd: ptr::null_mut(),
            voice_activation_hwnd: ptr::null_mut(),
            voice_microphone_hwnd: ptr::null_mut(),
            voice_microphone_ids: Vec::new(),
            voice_hotkey_hwnd: ptr::null_mut(),
            voice_engine_hwnd: ptr::null_mut(),
            voice_global_hwnd: ptr::null_mut(),
            voice_pack_status_hwnd: ptr::null_mut(),
            voice_pack_progress_hwnd: ptr::null_mut(),
            shortcut_status_hwnd: ptr::null_mut(),
            recording_shortcut: None,
            current_fullscreen_shortcut: preferences.workspace_fullscreen_shortcut.clone(),
            current_density_shortcut: preferences.workspace_density_shortcut.clone(),
            current_zoom_in_shortcut: preferences.workspace_zoom_in_shortcut.clone(),
            current_zoom_out_shortcut: preferences.workspace_zoom_out_shortcut.clone(),
            current_command_palette_shortcut: preferences.command_palette_shortcut.clone(),
            current_voice_hotkey: preferences.voice.hotkey.clone(),
        });
        let settings_state_ptr = Box::into_raw(settings_state);

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                wide(SETTINGS_WINDOW_CLASS).as_ptr(),
                wide(&state.runtime_options.product.settings_title).as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                preferences.settings_dialog_width,
                preferences.settings_dialog_height,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                settings_state_ptr.cast(),
            )
        };

        if hwnd.is_null() {
            unsafe {
                drop(Box::from_raw(settings_state_ptr));
            }
            logging::error("CreateWindowExW returned null for settings window");
            return;
        }

        state.settings_window_hwnd = hwnd;
    }

    fn create_settings_controls(hwnd: HWND, state: &mut SettingsWindowState) {
        let preferences = state.preference_store.load();
        let _ = create_child_window(
            hwnd,
            "STATIC",
            state.product_info.display_name.as_str(),
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_SUMMARY_TITLE,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            state.product_info.settings_summary.as_str(),
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_SUMMARY_COPY,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "MIT core",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_META_AUTOSAVE,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Public source",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_META_LIVE,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Theme default",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_LABEL_THEME,
        );
        state.theme_list_hwnd = create_child_window(
            hwnd,
            "LISTBOX",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | LBS_NOTIFY as u32,
            0,
            ID_SETTINGS_THEME_LIST,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Density default",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_LABEL_DENSITY,
        );
        state.density_list_hwnd = create_child_window(
            hwnd,
            "LISTBOX",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | LBS_NOTIFY as u32,
            0,
            ID_SETTINGS_DENSITY_LIST,
        );
        state.close_background_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Keep TerminalTiler running in the background when the main window closes",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            0,
            ID_SETTINGS_CLOSE_BACKGROUND,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Preferred WSL distro (optional)",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_LABEL_DISTRO,
        );
        state.distro_hwnd = create_child_window(
            hwnd,
            "EDIT",
            preferences
                .windows_wsl_distribution
                .as_deref()
                .unwrap_or(""),
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_LEFT as u32 | ES_AUTOHSCROLL as u32,
            0,
            ID_SETTINGS_WSL_DISTRO,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Voice input",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_LABEL_VOICE,
        );
        state.voice_enabled_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Enable local NVIDIA Parakeet voice-to-text",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            0,
            ID_SETTINGS_VOICE_ENABLED,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Activation",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_LABEL_VOICE_ACTIVATION,
        );
        state.voice_activation_hwnd = create_combo_box(hwnd, ID_SETTINGS_VOICE_ACTIVATION);
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Microphone",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_LABEL_VOICE_MICROPHONE,
        );
        state.voice_microphone_hwnd = create_combo_box(hwnd, ID_SETTINGS_VOICE_MICROPHONE);
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Voice hotkey",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_LABEL_VOICE_HOTKEY,
        );
        state.voice_hotkey_hwnd = create_child_window(
            hwnd,
            "EDIT",
            &shortcut_capture::display_label(&preferences.voice.hotkey),
            WS_CHILD | WS_VISIBLE | WS_BORDER | ES_READONLY as u32,
            0,
            ID_SETTINGS_VOICE_HOTKEY,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Record",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_VOICE_RECORD,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Final transcript chunks are inserted into the focused TerminalTiler terminal only.",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_NOTE_VOICE_HOTKEY,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Engine",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_LABEL_VOICE_ENGINE,
        );
        state.voice_engine_hwnd = create_combo_box(hwnd, ID_SETTINGS_VOICE_ENGINE);
        state.voice_global_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Prefer Win32 global hotkey when available",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            0,
            ID_SETTINGS_VOICE_GLOBAL,
        );
        state.voice_pack_status_hwnd = create_child_window(
            hwnd,
            "STATIC",
            &preferences.voice.pack_status.summary(),
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_VOICE_PACK_STATUS,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Install / Reinstall Pack",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_VOICE_PACK_INSTALL,
        );
        state.voice_pack_progress_hwnd = create_child_window(
            hwnd,
            "msctls_progress32",
            "",
            WS_CHILD | PBS_SMOOTH,
            0,
            ID_SETTINGS_VOICE_PACK_PROGRESS,
        );
        unsafe {
            SendMessageW(state.voice_pack_progress_hwnd, PBM_SETRANGE32, 0, 100);
        }
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Health Check",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_VOICE_PACK_HEALTH,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Delete Pack",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_VOICE_PACK_DELETE,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Workspace shortcuts",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_LABEL_SHORTCUTS,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Fullscreen",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_LABEL_FULLSCREEN_SHORTCUT,
        );
        state.fullscreen_shortcut_hwnd = create_child_window(
            hwnd,
            "EDIT",
            &shortcut_capture::display_label(&preferences.workspace_fullscreen_shortcut),
            WS_CHILD | WS_VISIBLE | WS_BORDER | ES_READONLY as u32,
            0,
            ID_SETTINGS_FULLSCREEN_SHORTCUT,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Record",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_FULLSCREEN_RECORD,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "?",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_HELP_FULLSCREEN_SHORTCUT,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Available only while a workspace tab is active.",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_NOTE_FULLSCREEN_SHORTCUT,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Density",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_LABEL_DENSITY_SHORTCUT,
        );
        state.density_shortcut_hwnd = create_child_window(
            hwnd,
            "EDIT",
            &shortcut_capture::display_label(&preferences.workspace_density_shortcut),
            WS_CHILD | WS_VISIBLE | WS_BORDER | ES_READONLY as u32,
            0,
            ID_SETTINGS_DENSITY_SHORTCUT,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Record",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_DENSITY_RECORD,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "?",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_HELP_DENSITY_SHORTCUT,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Rotates only the current workspace without changing the saved app default.",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_NOTE_DENSITY_SHORTCUT,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Zoom in",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_LABEL_ZOOM_IN_SHORTCUT,
        );
        state.zoom_in_shortcut_hwnd = create_child_window(
            hwnd,
            "EDIT",
            &shortcut_capture::display_label(&preferences.workspace_zoom_in_shortcut),
            WS_CHILD | WS_VISIBLE | WS_BORDER | ES_READONLY as u32,
            0,
            ID_SETTINGS_ZOOM_IN_SHORTCUT,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Record",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_ZOOM_IN_RECORD,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "?",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_HELP_ZOOM_IN_SHORTCUT,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Applies only to the active workspace and is restored with saved workspace sessions.",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_NOTE_ZOOM_IN_SHORTCUT,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Zoom out",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_LABEL_ZOOM_OUT_SHORTCUT,
        );
        state.zoom_out_shortcut_hwnd = create_child_window(
            hwnd,
            "EDIT",
            &shortcut_capture::display_label(&preferences.workspace_zoom_out_shortcut),
            WS_CHILD | WS_VISIBLE | WS_BORDER | ES_READONLY as u32,
            0,
            ID_SETTINGS_ZOOM_OUT_SHORTCUT,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Record",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_ZOOM_OUT_RECORD,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "?",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_HELP_ZOOM_OUT_SHORTCUT,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Applies only to the active workspace and is restored with saved workspace sessions.",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_NOTE_ZOOM_OUT_SHORTCUT,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Command palette",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_LABEL_COMMAND_PALETTE_SHORTCUT,
        );
        state.command_palette_shortcut_hwnd = create_child_window(
            hwnd,
            "EDIT",
            &shortcut_capture::display_label(&preferences.command_palette_shortcut),
            WS_CHILD | WS_VISIBLE | WS_BORDER | ES_READONLY as u32,
            0,
            ID_SETTINGS_COMMAND_PALETTE_SHORTCUT,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Record",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_COMMAND_PALETTE_RECORD,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "?",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_HELP_COMMAND_PALETTE_SHORTCUT,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Available in launch tabs and workspaces for fast navigation and actions.",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_NOTE_COMMAND_PALETTE_SHORTCUT,
        );
        state.shortcut_status_hwnd = create_child_window(
            hwnd,
            "STATIC",
            default_settings_status(),
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_SHORTCUT_STATUS,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Runtime preview",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_LABEL_RUNTIME,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Open Logs Folder",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_OPEN_LOGS_FOLDER,
        );
        state.runtime_status_hwnd = create_child_window(
            hwnd,
            "EDIT",
            "",
            WS_CHILD
                | WS_VISIBLE
                | WS_BORDER
                | WS_VSCROLL
                | ES_LEFT as u32
                | ES_MULTILINE as u32
                | ES_AUTOVSCROLL as u32
                | ES_READONLY as u32,
            0,
            ID_SETTINGS_RUNTIME_STATUS,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Check Runtime",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_PROBE,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Reset Defaults",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_RESET,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Reset Default Saved Presets",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_RESET_BUILTIN_PRESETS,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Close",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_CLOSE,
        );

        let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
        for control in [
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_SUMMARY_TITLE as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_SUMMARY_COPY as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_META_AUTOSAVE as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_META_LIVE as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_THEME as i32) },
            state.theme_list_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_DENSITY as i32) },
            state.density_list_hwnd,
            state.close_background_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_DISTRO as i32) },
            state.distro_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_VOICE as i32) },
            state.voice_enabled_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_VOICE_ACTIVATION as i32) },
            state.voice_activation_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_VOICE_MICROPHONE as i32) },
            state.voice_microphone_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_VOICE_HOTKEY as i32) },
            state.voice_hotkey_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_VOICE_RECORD as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_NOTE_VOICE_HOTKEY as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_VOICE_ENGINE as i32) },
            state.voice_engine_hwnd,
            state.voice_global_hwnd,
            state.voice_pack_status_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_VOICE_PACK_INSTALL as i32) },
            state.voice_pack_progress_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_VOICE_PACK_HEALTH as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_VOICE_PACK_DELETE as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_SHORTCUTS as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_FULLSCREEN_SHORTCUT as i32) },
            state.fullscreen_shortcut_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_FULLSCREEN_RECORD as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_HELP_FULLSCREEN_SHORTCUT as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_NOTE_FULLSCREEN_SHORTCUT as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_DENSITY_SHORTCUT as i32) },
            state.density_shortcut_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_DENSITY_RECORD as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_HELP_DENSITY_SHORTCUT as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_NOTE_DENSITY_SHORTCUT as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_ZOOM_IN_SHORTCUT as i32) },
            state.zoom_in_shortcut_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_ZOOM_IN_RECORD as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_HELP_ZOOM_IN_SHORTCUT as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_NOTE_ZOOM_IN_SHORTCUT as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_ZOOM_OUT_SHORTCUT as i32) },
            state.zoom_out_shortcut_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_ZOOM_OUT_RECORD as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_HELP_ZOOM_OUT_SHORTCUT as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_NOTE_ZOOM_OUT_SHORTCUT as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_COMMAND_PALETTE_SHORTCUT as i32) },
            state.command_palette_shortcut_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_COMMAND_PALETTE_RECORD as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_HELP_COMMAND_PALETTE_SHORTCUT as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_NOTE_COMMAND_PALETTE_SHORTCUT as i32) },
            state.shortcut_status_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_RUNTIME as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_OPEN_LOGS_FOLDER as i32) },
            state.runtime_status_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_PROBE as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_RESET as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_RESET_BUILTIN_PRESETS as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_CLOSE as i32) },
        ] {
            if !control.is_null() {
                unsafe {
                    SendMessageW(control, WM_SETFONT, font as usize, 1);
                }
            }
        }

        populate_listbox_items(state.theme_list_hwnd, &["System", "Light", "Dark"]);
        populate_listbox_items(
            state.density_list_hwnd,
            &["Comfortable", "Standard", "Compact"],
        );
        populate_combo_box_items(state.voice_activation_hwnd, &["Push to Talk", "Toggle"]);
        populate_voice_microphones(state, preferences.voice.microphone_id.as_deref());
        populate_combo_box_items(state.voice_engine_hwnd, &["Auto", "CUDA", "CPU"]);
        populate_voice_microphones(state, preferences.voice.microphone_id.as_deref());
        apply_preferences_to_settings_controls(state, &preferences);
        layout_settings_controls(hwnd, state);
    }

    fn layout_settings_controls(hwnd: HWND, state: &SettingsWindowState) {
        let mut rect = unsafe { mem::zeroed() };
        unsafe {
            GetClientRect(hwnd, &mut rect);
        }
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let content_width = width - (MARGIN * 2);

        let summary_title_y = MARGIN;
        let summary_copy_y = summary_title_y + 22;
        let summary_meta_y = summary_copy_y + 40;
        let theme_label_y = summary_meta_y + 24;
        let theme_list_y = theme_label_y + LABEL_HEIGHT + 4;
        let density_label_y = theme_list_y + SETTINGS_LIST_HEIGHT + 12;
        let density_list_y = density_label_y + LABEL_HEIGHT + 4;
        let checkbox_y = density_list_y + SETTINGS_LIST_HEIGHT + 12;
        let distro_label_y = checkbox_y + 28 + 12;
        let distro_edit_y = distro_label_y + LABEL_HEIGHT + 4;
        let voice_label_y = distro_edit_y + FIELD_HEIGHT + 12;
        let voice_enabled_y = voice_label_y + LABEL_HEIGHT + 8;
        let voice_controls_y = voice_enabled_y + 30;
        let voice_microphone_y = voice_controls_y + FIELD_HEIGHT + 8;
        let voice_hotkey_y = voice_microphone_y + FIELD_HEIGHT + 8;
        let voice_pack_y = voice_hotkey_y + FIELD_HEIGHT + LABEL_HEIGHT + 10;
        let shortcuts_label_y = voice_pack_y + BUTTON_HEIGHT + 12;
        let shortcut_row_height = FIELD_HEIGHT + LABEL_HEIGHT + 18;
        let shortcut_row_1_y = shortcuts_label_y + LABEL_HEIGHT + 8;
        let shortcut_row_2_y = shortcut_row_1_y + shortcut_row_height;
        let shortcut_row_3_y = shortcut_row_2_y + shortcut_row_height;
        let shortcut_row_4_y = shortcut_row_3_y + shortcut_row_height;
        let shortcut_row_5_y = shortcut_row_4_y + shortcut_row_height;
        let shortcut_status_y = shortcut_row_5_y + shortcut_row_height;
        let runtime_label_y = shortcut_status_y + LABEL_HEIGHT + 12;
        let runtime_edit_y = runtime_label_y + LABEL_HEIGHT + 4;
        let button_y = height - MARGIN - BUTTON_HEIGHT;
        let runtime_height = (button_y - runtime_edit_y - 12).max(120);
        let shortcut_label_width = 140;
        let shortcut_button_width = 84;
        let shortcut_help_width = 36;
        let shortcut_edit_x = MARGIN + shortcut_label_width + 8;
        let shortcut_edit_width = (content_width
            - shortcut_label_width
            - shortcut_button_width
            - shortcut_help_width
            - 24)
            .max(120);
        let shortcut_button_x = shortcut_edit_x + shortcut_edit_width + 8;
        let shortcut_help_x = shortcut_button_x + shortcut_button_width + 8;
        let voice_column_width = ((content_width - 24) / 3).max(120);
        let voice_activation_x = MARGIN + 92;
        let voice_engine_x = voice_activation_x + voice_column_width + 12;
        let voice_global_x = voice_engine_x + voice_column_width + 12;

        unsafe {
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_SUMMARY_TITLE as i32),
                ptr::null_mut(),
                MARGIN,
                summary_title_y,
                content_width,
                22,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_SUMMARY_COPY as i32),
                ptr::null_mut(),
                MARGIN,
                summary_copy_y,
                content_width,
                36,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_META_AUTOSAVE as i32),
                ptr::null_mut(),
                MARGIN,
                summary_meta_y,
                120,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_META_LIVE as i32),
                ptr::null_mut(),
                MARGIN + 128,
                summary_meta_y,
                96,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_LABEL_THEME as i32),
                ptr::null_mut(),
                MARGIN,
                theme_label_y,
                content_width,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.theme_list_hwnd,
                ptr::null_mut(),
                MARGIN,
                theme_list_y,
                content_width,
                SETTINGS_LIST_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_LABEL_DENSITY as i32),
                ptr::null_mut(),
                MARGIN,
                density_label_y,
                content_width,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.density_list_hwnd,
                ptr::null_mut(),
                MARGIN,
                density_list_y,
                content_width,
                SETTINGS_LIST_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.close_background_hwnd,
                ptr::null_mut(),
                MARGIN,
                checkbox_y,
                content_width,
                24,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_LABEL_DISTRO as i32),
                ptr::null_mut(),
                MARGIN,
                distro_label_y,
                content_width,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.distro_hwnd,
                ptr::null_mut(),
                MARGIN,
                distro_edit_y,
                content_width,
                FIELD_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_LABEL_VOICE as i32),
                ptr::null_mut(),
                MARGIN,
                voice_label_y,
                content_width,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.voice_enabled_hwnd,
                ptr::null_mut(),
                MARGIN,
                voice_enabled_y,
                content_width,
                24,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_LABEL_VOICE_ACTIVATION as i32),
                ptr::null_mut(),
                MARGIN,
                voice_controls_y + 5,
                84,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.voice_activation_hwnd,
                ptr::null_mut(),
                voice_activation_x,
                voice_controls_y,
                voice_column_width,
                140,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_LABEL_VOICE_ENGINE as i32),
                ptr::null_mut(),
                voice_engine_x - 64,
                voice_controls_y + 5,
                56,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.voice_engine_hwnd,
                ptr::null_mut(),
                voice_engine_x,
                voice_controls_y,
                voice_column_width,
                140,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.voice_global_hwnd,
                ptr::null_mut(),
                voice_global_x,
                voice_controls_y + 2,
                (content_width - (voice_global_x - MARGIN)).max(160),
                24,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_LABEL_VOICE_MICROPHONE as i32),
                ptr::null_mut(),
                MARGIN,
                voice_microphone_y + 5,
                84,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.voice_microphone_hwnd,
                ptr::null_mut(),
                voice_activation_x,
                voice_microphone_y,
                (content_width - 92).max(180),
                180,
                SWP_NOZORDER,
            );
            layout_shortcut_row(
                hwnd,
                ShortcutRowControls {
                    label_id: ID_SETTINGS_LABEL_VOICE_HOTKEY,
                    edit_hwnd: state.voice_hotkey_hwnd,
                    button_id: ID_SETTINGS_VOICE_RECORD,
                    note_id: ID_SETTINGS_NOTE_VOICE_HOTKEY,
                    help_id: 0,
                },
                ShortcutRowLayout {
                    y: voice_hotkey_y,
                    label_width: shortcut_label_width,
                    edit_x: shortcut_edit_x,
                    edit_width: shortcut_edit_width,
                    button_x: shortcut_button_x,
                    help_x: shortcut_help_x,
                },
            );
            SetWindowPos(
                state.voice_pack_status_hwnd,
                ptr::null_mut(),
                MARGIN,
                voice_pack_y + 7,
                (content_width - (BUTTON_WIDTH * 3) - 36).max(180),
                LABEL_HEIGHT + 6,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_VOICE_PACK_INSTALL as i32),
                ptr::null_mut(),
                width - MARGIN - (BUTTON_WIDTH * 3) - 24,
                voice_pack_y,
                BUTTON_WIDTH,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.voice_pack_progress_hwnd,
                ptr::null_mut(),
                width - MARGIN - (BUTTON_WIDTH * 3) - 24,
                voice_pack_y + 6,
                BUTTON_WIDTH,
                BUTTON_HEIGHT - 12,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_VOICE_PACK_HEALTH as i32),
                ptr::null_mut(),
                width - MARGIN - (BUTTON_WIDTH * 2) - 12,
                voice_pack_y,
                BUTTON_WIDTH,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_VOICE_PACK_DELETE as i32),
                ptr::null_mut(),
                width - MARGIN - BUTTON_WIDTH,
                voice_pack_y,
                BUTTON_WIDTH,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_LABEL_SHORTCUTS as i32),
                ptr::null_mut(),
                MARGIN,
                shortcuts_label_y,
                content_width,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            layout_shortcut_row(
                hwnd,
                ShortcutRowControls {
                    label_id: ID_SETTINGS_LABEL_FULLSCREEN_SHORTCUT,
                    edit_hwnd: state.fullscreen_shortcut_hwnd,
                    button_id: ID_SETTINGS_FULLSCREEN_RECORD,
                    note_id: ID_SETTINGS_NOTE_FULLSCREEN_SHORTCUT,
                    help_id: ID_SETTINGS_HELP_FULLSCREEN_SHORTCUT,
                },
                ShortcutRowLayout {
                    y: shortcut_row_1_y,
                    label_width: shortcut_label_width,
                    edit_x: shortcut_edit_x,
                    edit_width: shortcut_edit_width,
                    button_x: shortcut_button_x,
                    help_x: shortcut_help_x,
                },
            );
            layout_shortcut_row(
                hwnd,
                ShortcutRowControls {
                    label_id: ID_SETTINGS_LABEL_DENSITY_SHORTCUT,
                    edit_hwnd: state.density_shortcut_hwnd,
                    button_id: ID_SETTINGS_DENSITY_RECORD,
                    note_id: ID_SETTINGS_NOTE_DENSITY_SHORTCUT,
                    help_id: ID_SETTINGS_HELP_DENSITY_SHORTCUT,
                },
                ShortcutRowLayout {
                    y: shortcut_row_2_y,
                    label_width: shortcut_label_width,
                    edit_x: shortcut_edit_x,
                    edit_width: shortcut_edit_width,
                    button_x: shortcut_button_x,
                    help_x: shortcut_help_x,
                },
            );
            layout_shortcut_row(
                hwnd,
                ShortcutRowControls {
                    label_id: ID_SETTINGS_LABEL_ZOOM_IN_SHORTCUT,
                    edit_hwnd: state.zoom_in_shortcut_hwnd,
                    button_id: ID_SETTINGS_ZOOM_IN_RECORD,
                    note_id: ID_SETTINGS_NOTE_ZOOM_IN_SHORTCUT,
                    help_id: ID_SETTINGS_HELP_ZOOM_IN_SHORTCUT,
                },
                ShortcutRowLayout {
                    y: shortcut_row_3_y,
                    label_width: shortcut_label_width,
                    edit_x: shortcut_edit_x,
                    edit_width: shortcut_edit_width,
                    button_x: shortcut_button_x,
                    help_x: shortcut_help_x,
                },
            );
            layout_shortcut_row(
                hwnd,
                ShortcutRowControls {
                    label_id: ID_SETTINGS_LABEL_ZOOM_OUT_SHORTCUT,
                    edit_hwnd: state.zoom_out_shortcut_hwnd,
                    button_id: ID_SETTINGS_ZOOM_OUT_RECORD,
                    note_id: ID_SETTINGS_NOTE_ZOOM_OUT_SHORTCUT,
                    help_id: ID_SETTINGS_HELP_ZOOM_OUT_SHORTCUT,
                },
                ShortcutRowLayout {
                    y: shortcut_row_4_y,
                    label_width: shortcut_label_width,
                    edit_x: shortcut_edit_x,
                    edit_width: shortcut_edit_width,
                    button_x: shortcut_button_x,
                    help_x: shortcut_help_x,
                },
            );
            layout_shortcut_row(
                hwnd,
                ShortcutRowControls {
                    label_id: ID_SETTINGS_LABEL_COMMAND_PALETTE_SHORTCUT,
                    edit_hwnd: state.command_palette_shortcut_hwnd,
                    button_id: ID_SETTINGS_COMMAND_PALETTE_RECORD,
                    note_id: ID_SETTINGS_NOTE_COMMAND_PALETTE_SHORTCUT,
                    help_id: ID_SETTINGS_HELP_COMMAND_PALETTE_SHORTCUT,
                },
                ShortcutRowLayout {
                    y: shortcut_row_5_y,
                    label_width: shortcut_label_width,
                    edit_x: shortcut_edit_x,
                    edit_width: shortcut_edit_width,
                    button_x: shortcut_button_x,
                    help_x: shortcut_help_x,
                },
            );
            SetWindowPos(
                state.shortcut_status_hwnd,
                ptr::null_mut(),
                MARGIN,
                shortcut_status_y,
                content_width,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_LABEL_RUNTIME as i32),
                ptr::null_mut(),
                MARGIN,
                runtime_label_y,
                (content_width - BUTTON_WIDTH - 12).max(120),
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_OPEN_LOGS_FOLDER as i32),
                ptr::null_mut(),
                width - MARGIN - BUTTON_WIDTH,
                runtime_label_y - 6,
                BUTTON_WIDTH,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.runtime_status_hwnd,
                ptr::null_mut(),
                MARGIN,
                runtime_edit_y,
                content_width,
                runtime_height,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_PROBE as i32),
                ptr::null_mut(),
                MARGIN,
                button_y,
                BUTTON_WIDTH,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_RESET as i32),
                ptr::null_mut(),
                MARGIN + BUTTON_WIDTH + 12,
                button_y,
                132,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_RESET_BUILTIN_PRESETS as i32),
                ptr::null_mut(),
                MARGIN + BUTTON_WIDTH + 156,
                button_y,
                208,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_CLOSE as i32),
                ptr::null_mut(),
                width - MARGIN - 96,
                button_y,
                96,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
        }
    }

    struct ShortcutRowControls {
        label_id: isize,
        edit_hwnd: HWND,
        button_id: isize,
        note_id: isize,
        help_id: isize,
    }

    struct ShortcutRowLayout {
        y: i32,
        label_width: i32,
        edit_x: i32,
        edit_width: i32,
        button_x: i32,
        help_x: i32,
    }

    fn layout_shortcut_row(
        hwnd: HWND,
        controls: ShortcutRowControls,
        row_layout: ShortcutRowLayout,
    ) {
        unsafe {
            SetWindowPos(
                GetDlgItem(hwnd, controls.label_id as i32),
                ptr::null_mut(),
                MARGIN,
                row_layout.y + 4,
                row_layout.label_width,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                controls.edit_hwnd,
                ptr::null_mut(),
                row_layout.edit_x,
                row_layout.y,
                row_layout.edit_width,
                FIELD_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, controls.button_id as i32),
                ptr::null_mut(),
                row_layout.button_x,
                row_layout.y - 2,
                76,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            if controls.help_id != 0 {
                SetWindowPos(
                    GetDlgItem(hwnd, controls.help_id as i32),
                    ptr::null_mut(),
                    row_layout.help_x,
                    row_layout.y - 2,
                    30,
                    BUTTON_HEIGHT,
                    SWP_NOZORDER,
                );
            }
            SetWindowPos(
                GetDlgItem(hwnd, controls.note_id as i32),
                ptr::null_mut(),
                row_layout.edit_x,
                row_layout.y + FIELD_HEIGHT + 4,
                row_layout.help_x + 30 - row_layout.edit_x,
                LABEL_HEIGHT + 8,
                SWP_NOZORDER,
            );
        }
    }

    fn refresh_settings_runtime_preview(state: &SettingsWindowState) {
        let preferred_distribution = read_window_text(state.distro_hwnd);
        let runtime_text = match wsl::probe_runtime(Some(preferred_distribution.as_str())) {
            Ok(runtime) => format!(
                "Active runtime: {}\r\nStatus: {}",
                runtime.label(),
                runtime.selection_reason()
            ),
            Err(error) => format!("Runtime unavailable:\r\n{error}"),
        };
        let browser_text = match workspace::probe_webview2_runtime() {
            Ok(()) => "Browser runtime: WebView2 available".to_string(),
            Err(error) => format!(
                "Browser runtime: unavailable\r\nWeb tiles require Microsoft Edge WebView2 Runtime.\r\n{}",
                error
            ),
        };
        unsafe {
            SetWindowTextW(
                state.runtime_status_hwnd,
                wide(&format!("{}\r\n\r\n{}", runtime_text, browser_text)).as_ptr(),
            );
        }
    }

    fn open_logs_folder_from_settings(state: &SettingsWindowState) {
        match logging::ensure_log_directory() {
            Ok(path) => match open_path_with_shell(state.window_hwnd, &path) {
                Ok(()) => {
                    logging::info(format!("opened application logs folder {}", path.display()));
                    set_settings_status(state, "Opened logs folder.");
                }
                Err(error) => {
                    logging::error(format!(
                        "failed to open application logs folder '{}': {}",
                        path.display(),
                        error
                    ));
                    set_settings_status(state, "Failed to open logs folder.");
                }
            },
            Err(error) => {
                logging::error(format!(
                    "failed to prepare application logs folder: {}",
                    error
                ));
                set_settings_status(state, "Could not resolve logs folder.");
            }
        }
    }

    fn begin_shortcut_capture(hwnd: HWND, state: &mut SettingsWindowState, field: ShortcutField) {
        if state.recording_shortcut == Some(field) {
            state.recording_shortcut = None;
            update_shortcut_record_button_labels(hwnd, state);
            set_settings_status(state, default_settings_status());
            return;
        }
        state.recording_shortcut = Some(field);
        update_shortcut_record_button_labels(hwnd, state);
        set_settings_status(
            state,
            &format!("Recording {}. Press Esc to cancel.", shortcut_title(field)),
        );
        unsafe { SetFocus(hwnd) };
    }

    fn handle_settings_shortcut_capture(
        hwnd: HWND,
        state: &mut SettingsWindowState,
        virtual_key: u32,
    ) -> bool {
        let Some(field) = state.recording_shortcut else {
            return false;
        };
        let Some(rendered) = shortcut_capture::capture_shortcut_from_keydown(virtual_key) else {
            set_settings_status(
                state,
                "That key cannot be used alone. Try a function key or add modifiers.",
            );
            return true;
        };
        state.recording_shortcut = None;
        update_shortcut_record_button_labels(hwnd, state);
        if rendered.is_empty() {
            set_settings_status(state, default_settings_status());
            return true;
        }
        let target_hwnd = shortcut_hwnd_for_field(state, field);
        *shortcut_value_mut(state, field) = rendered.clone();
        unsafe {
            SetWindowTextW(
                target_hwnd,
                wide(&shortcut_capture::display_label(&rendered)).as_ptr(),
            );
            SetFocus(hwnd);
        }
        apply_live_settings_change(
            state,
            &format!(
                "{} updated to {}.",
                shortcut_title(field),
                shortcut_capture::display_label(&rendered)
            ),
            false,
            true,
        );
        true
    }

    fn shortcut_hwnd_for_field(state: &SettingsWindowState, field: ShortcutField) -> HWND {
        match field {
            ShortcutField::Fullscreen => state.fullscreen_shortcut_hwnd,
            ShortcutField::Density => state.density_shortcut_hwnd,
            ShortcutField::ZoomIn => state.zoom_in_shortcut_hwnd,
            ShortcutField::ZoomOut => state.zoom_out_shortcut_hwnd,
            ShortcutField::CommandPalette => state.command_palette_shortcut_hwnd,
            ShortcutField::VoiceHotkey => state.voice_hotkey_hwnd,
        }
    }

    fn shortcut_value_mut(state: &mut SettingsWindowState, field: ShortcutField) -> &mut String {
        match field {
            ShortcutField::Fullscreen => &mut state.current_fullscreen_shortcut,
            ShortcutField::Density => &mut state.current_density_shortcut,
            ShortcutField::ZoomIn => &mut state.current_zoom_in_shortcut,
            ShortcutField::ZoomOut => &mut state.current_zoom_out_shortcut,
            ShortcutField::CommandPalette => &mut state.current_command_palette_shortcut,
            ShortcutField::VoiceHotkey => &mut state.current_voice_hotkey,
        }
    }

    fn shortcut_record_button_id(field: ShortcutField) -> isize {
        match field {
            ShortcutField::Fullscreen => ID_SETTINGS_FULLSCREEN_RECORD,
            ShortcutField::Density => ID_SETTINGS_DENSITY_RECORD,
            ShortcutField::ZoomIn => ID_SETTINGS_ZOOM_IN_RECORD,
            ShortcutField::ZoomOut => ID_SETTINGS_ZOOM_OUT_RECORD,
            ShortcutField::CommandPalette => ID_SETTINGS_COMMAND_PALETTE_RECORD,
            ShortcutField::VoiceHotkey => ID_SETTINGS_VOICE_RECORD,
        }
    }

    fn shortcut_title(field: ShortcutField) -> &'static str {
        match field {
            ShortcutField::Fullscreen => "Toggle workspace fullscreen",
            ShortcutField::Density => "Cycle active workspace density",
            ShortcutField::ZoomIn => "Zoom in terminal text",
            ShortcutField::ZoomOut => "Zoom out terminal text",
            ShortcutField::CommandPalette => "Open command palette",
            ShortcutField::VoiceHotkey => "Voice hotkey",
        }
    }

    fn shortcut_note(field: ShortcutField) -> &'static str {
        match field {
            ShortcutField::Fullscreen => "Available only while a workspace tab is active.",
            ShortcutField::Density => {
                "Rotates only the current workspace without changing the saved app default."
            }
            ShortcutField::ZoomIn | ShortcutField::ZoomOut => {
                "Applies only to the active workspace and is restored with saved workspace sessions."
            }
            ShortcutField::CommandPalette => {
                "Available in launch tabs and workspaces for fast navigation and actions."
            }
            ShortcutField::VoiceHotkey => {
                "Push-to-talk uses press/release. Toggle mode starts and stops on repeated presses."
            }
        }
    }

    fn shortcut_examples(field: ShortcutField) -> &'static [&'static str] {
        match field {
            ShortcutField::Fullscreen => &["F11", "<Shift>F11", "<Ctrl>F11"],
            ShortcutField::Density => &["<Ctrl><Shift>D", "<Shift>F8", "<Alt><Super>D"],
            ShortcutField::ZoomIn => &["<Ctrl>plus", "<Ctrl>equal", "<Ctrl>KP_Add"],
            ShortcutField::ZoomOut => &["<Ctrl>minus", "<Ctrl>KP_Subtract"],
            ShortcutField::CommandPalette => &["<Ctrl><Shift>P", "<Ctrl>P", "<Super>P"],
            ShortcutField::VoiceHotkey => &["<Ctrl><Shift>space", "<Alt>space", "F9"],
        }
    }

    fn default_settings_status() -> &'static str {
        "Changes are saved automatically. Click Record, then press the shortcut you want. Press Esc to cancel."
    }

    fn set_settings_status(state: &SettingsWindowState, message: &str) {
        unsafe {
            SetWindowTextW(state.shortcut_status_hwnd, wide(message).as_ptr());
        }
    }

    fn show_shortcut_help(hwnd: HWND, field: ShortcutField) {
        let examples = shortcut_examples(field).join("\r\n");
        let body = format!(
            "{}\r\n\r\n{}\r\n\r\nClick Record, then press the shortcut you want to use. Press Esc while recording to cancel.\r\n\r\nExamples\r\n{}",
            shortcut_title(field),
            shortcut_note(field),
            examples
        );
        unsafe {
            MessageBoxW(
                hwnd,
                wide(&body).as_ptr(),
                wide(shortcut_title(field)).as_ptr(),
                MB_OK,
            );
        }
    }

    fn update_shortcut_record_button_labels(hwnd: HWND, state: &SettingsWindowState) {
        for field in [
            ShortcutField::Fullscreen,
            ShortcutField::Density,
            ShortcutField::ZoomIn,
            ShortcutField::ZoomOut,
            ShortcutField::CommandPalette,
            ShortcutField::VoiceHotkey,
        ] {
            let label = if state.recording_shortcut == Some(field) {
                "Press keys..."
            } else {
                "Record"
            };
            unsafe {
                SetWindowTextW(
                    GetDlgItem(hwnd, shortcut_record_button_id(field) as i32),
                    wide(label).as_ptr(),
                );
            }
        }
    }

    fn set_voice_pack_status(state: &mut SettingsWindowState, status: VoicePackStatus) {
        let mut preferences = settings_snapshot_from_controls(state);
        preferences.voice.pack_status = status.clone();
        state.preference_store.save(&preferences);
        apply_preferences_to_settings_controls(state, &preferences);
    }

    fn install_voice_pack_from_settings(state: &mut SettingsWindowState) {
        let Some(root) = pack::default_voice_pack_dir() else {
            set_settings_status(state, "Could not resolve application data directory.");
            return;
        };
        let engine_mode = settings_snapshot_from_controls(state).voice.engine_mode;
        let preference_store = state.preference_store.clone();
        let hwnd = state.window_hwnd as isize;
        set_voice_pack_status(state, VoicePackStatus::Downloading { percent: 1 });
        set_settings_status(
            state,
            "Installing NVIDIA Parakeet voice pack in the background…",
        );

        thread::spawn(move || {
            let post = |message: &str| post_settings_voice_pack_event(hwnd, message);
            let save_status = |status: VoicePackStatus| {
                let mut preferences = preference_store.load();
                preferences.voice.pack_status = status;
                preference_store.save(&preferences);
            };
            let save_progress = |percent| {
                save_voice_pack_download_progress(&preference_store, percent);
            };

            match pack::install_builtin_parakeet_pack(&root) {
                Ok(manifest) => {
                    save_progress(40);
                    post("Installing Python dependencies for NVIDIA Parakeet…");
                    match pack::prepare_python_environment_with_progress(
                        &root,
                        &manifest,
                        save_progress,
                    ) {
                        Ok(_) => {
                            save_voice_pack_download_progress(&preference_store, 80);
                            post("Verifying NVIDIA Parakeet runtime dependencies…");
                            match pack::health_check(&root, &manifest) {
                                health @ VoicePackHealth::Ready { .. } => {
                                    let (progress_stop, progress_thread) =
                                        start_voice_pack_progress_heartbeat(
                                            preference_store.clone(),
                                            81,
                                            96,
                                        );
                                    let health_event = engine::run_voice_engine_health_check(
                                        &manifest,
                                        health,
                                        engine_mode,
                                    );
                                    progress_stop.store(true, Ordering::Relaxed);
                                    let _ = progress_thread.join();
                                    match health_event {
                                        Ok(VoiceEngineEvent::Health { ok: true, detail }) => {
                                            save_status(VoicePackStatus::Installed {
                                                version: manifest.version.clone(),
                                            });
                                            logging::info(format!(
                                                "installed bundled NVIDIA Parakeet voice pack on Windows id={} version={} root={} health={}",
                                                manifest.id,
                                                manifest.version,
                                                root.display(),
                                                detail
                                            ));
                                            post(
                                                "NVIDIA Parakeet voice pack installed; model will warm on first use.",
                                            );
                                        }
                                        Ok(VoiceEngineEvent::Health { detail, .. })
                                        | Ok(VoiceEngineEvent::Error(detail)) => {
                                            save_status(VoicePackStatus::Error {
                                                message: detail.clone(),
                                            });
                                            logging::error(format!(
                                                "NVIDIA Parakeet voice pack installed on Windows but runtime health failed: {detail}"
                                            ));
                                            post(
                                                "Voice pack installed, but Parakeet verification failed.",
                                            );
                                        }
                                        Ok(other) => {
                                            save_status(VoicePackStatus::Error {
                                                message: format!(
                                                    "inconclusive health check: {other:?}"
                                                ),
                                            });
                                            post(
                                                "Voice pack installed, but health check was inconclusive.",
                                            );
                                        }
                                        Err(error) => {
                                            save_status(VoicePackStatus::Error {
                                                message: error.to_string(),
                                            });
                                            logging::error(format!(
                                                "failed to verify NVIDIA Parakeet voice pack on Windows: {error}"
                                            ));
                                            post("Voice pack installed, but verification failed.");
                                        }
                                    }
                                }
                                VoicePackHealth::Missing | VoicePackHealth::Broken(_) => {
                                    save_status(VoicePackStatus::Error {
                                        message: "voice pack files are incomplete after install"
                                            .into(),
                                    });
                                    post("NVIDIA Parakeet voice pack is incomplete after install.");
                                }
                            }
                        }
                        Err(error) => {
                            save_status(VoicePackStatus::Error {
                                message: error.user_message(),
                            });
                            logging::error(format!(
                                "failed to prepare NVIDIA Parakeet Python environment on Windows: {error:?}"
                            ));
                            post("Voice pack installed, but Python dependencies failed.");
                        }
                    }
                }
                Err(error) => {
                    save_status(VoicePackStatus::Error {
                        message: error.user_message(),
                    });
                    logging::error(format!(
                        "failed to install bundled NVIDIA Parakeet voice pack on Windows: {error:?}"
                    ));
                    post("Failed to install NVIDIA Parakeet voice pack.");
                }
            }
        });
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

    fn start_voice_pack_progress_heartbeat(
        preference_store: PreferenceStore,
        start_percent: u8,
        end_percent: u8,
    ) -> (Arc<AtomicBool>, thread::JoinHandle<()>) {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let handle = thread::spawn(move || {
            let mut percent = start_percent.clamp(1, 99);
            let end_percent = end_percent.clamp(percent, 99);
            save_voice_pack_download_progress(&preference_store, percent);
            while !worker_stop.load(Ordering::Relaxed) {
                thread::sleep(std::time::Duration::from_secs(5));
                if worker_stop.load(Ordering::Relaxed) {
                    break;
                }
                if percent < end_percent {
                    percent += 1;
                }
                save_voice_pack_download_progress(&preference_store, percent);
            }
        });
        (stop, handle)
    }

    fn post_settings_voice_pack_event(hwnd: isize, message: &str) {
        let event = Box::into_raw(Box::new(SettingsVoicePackEvent::Status(message.into())));
        let posted = unsafe {
            PostMessageW(
                hwnd as HWND,
                WM_SETTINGS_VOICE_PACK_EVENT,
                0,
                event as LPARAM,
            )
        };
        if posted == 0 {
            unsafe {
                drop(Box::from_raw(event));
            }
        }
    }

    fn check_voice_pack_from_settings(state: &mut SettingsWindowState) {
        let manifest = pack::builtin_parakeet_manifest();
        let Some(root) = pack::default_voice_pack_dir() else {
            set_settings_status(state, "Could not resolve application data directory.");
            return;
        };
        match pack::health_check(&root, &manifest) {
            health @ VoicePackHealth::Ready { .. } => {
                let engine_mode = settings_snapshot_from_controls(state).voice.engine_mode;
                match engine::run_voice_engine_health_check(&manifest, health, engine_mode) {
                    Ok(VoiceEngineEvent::Health { ok, detail }) if ok => {
                        set_settings_status(
                            state,
                            &format!("NVIDIA Parakeet runtime is healthy: {detail}"),
                        );
                    }
                    Ok(VoiceEngineEvent::Health { detail, .. })
                    | Ok(VoiceEngineEvent::Error(detail)) => {
                        set_settings_status(
                            state,
                            &format!("NVIDIA Parakeet runtime dependencies are missing: {detail}"),
                        );
                    }
                    Ok(other) => {
                        set_settings_status(
                            state,
                            &format!("NVIDIA Parakeet health check was inconclusive: {other:?}"),
                        );
                    }
                    Err(error) => {
                        set_settings_status(
                            state,
                            &format!("Failed to run NVIDIA Parakeet health check: {error}"),
                        );
                    }
                }
            }
            VoicePackHealth::Missing => {
                set_settings_status(state, "NVIDIA Parakeet voice pack is not installed.");
            }
            VoicePackHealth::Broken(message) => {
                set_settings_status(
                    state,
                    &format!("NVIDIA Parakeet voice pack is incomplete: {message}"),
                );
            }
        }
    }

    fn delete_voice_pack_from_settings(state: &mut SettingsWindowState) {
        let manifest = pack::builtin_parakeet_manifest();
        let Some(root) = pack::default_voice_pack_dir() else {
            set_settings_status(state, "Could not resolve application data directory.");
            return;
        };
        match pack::delete_pack(&root, &manifest) {
            Ok(_) => {
                set_voice_pack_status(state, VoicePackStatus::NotInstalled);
                set_settings_status(state, "NVIDIA Parakeet voice pack deleted.");
                logging::info(format!(
                    "deleted NVIDIA Parakeet voice pack on Windows id={} version={} root={}",
                    manifest.id,
                    manifest.version,
                    root.display()
                ));
            }
            Err(error) => {
                set_settings_status(state, "Failed to delete NVIDIA Parakeet voice pack.");
                logging::error(format!(
                    "failed to delete NVIDIA Parakeet voice pack on Windows: {error:?}"
                ));
            }
        }
    }

    fn settings_snapshot_from_controls(state: &SettingsWindowState) -> AppPreferences {
        let mut preferences = state.preference_store.load();
        preferences.default_theme = theme_from_index(selected_listbox_index(state.theme_list_hwnd));
        preferences.default_density =
            density_from_index(selected_listbox_index(state.density_list_hwnd));
        preferences.close_to_background =
            unsafe { SendMessageW(state.close_background_hwnd, BM_GETCHECK, 0, 0) }
                == CHECKBOX_CHECKED as isize;
        let preferred_distribution = read_window_text(state.distro_hwnd);
        preferences.windows_wsl_distribution = if preferred_distribution.trim().is_empty() {
            None
        } else {
            Some(preferred_distribution.trim().to_string())
        };
        preferences.workspace_fullscreen_shortcut = state.current_fullscreen_shortcut.clone();
        preferences.workspace_density_shortcut = state.current_density_shortcut.clone();
        preferences.workspace_zoom_in_shortcut = state.current_zoom_in_shortcut.clone();
        preferences.workspace_zoom_out_shortcut = state.current_zoom_out_shortcut.clone();
        preferences.command_palette_shortcut = state.current_command_palette_shortcut.clone();
        preferences.voice.enabled =
            unsafe { SendMessageW(state.voice_enabled_hwnd, BM_GETCHECK, 0, 0) }
                == CHECKBOX_CHECKED as isize;
        preferences.voice.activation_mode =
            voice_activation_from_index(selected_combo_index(state.voice_activation_hwnd));
        preferences.voice.microphone_id = selected_voice_microphone_id(state);
        preferences.voice.hotkey = state.current_voice_hotkey.clone();
        preferences.voice.engine_mode =
            voice_engine_from_index(selected_combo_index(state.voice_engine_hwnd));
        preferences.voice.prefer_global_hotkey =
            unsafe { SendMessageW(state.voice_global_hwnd, BM_GETCHECK, 0, 0) }
                == CHECKBOX_CHECKED as isize;
        preferences
    }

    fn settings_match_defaults(preferences: &AppPreferences) -> bool {
        let defaults = AppPreferences::default();
        preferences.default_theme == defaults.default_theme
            && preferences.default_density == defaults.default_density
            && preferences.close_to_background == defaults.close_to_background
            && preferences.windows_wsl_distribution == defaults.windows_wsl_distribution
            && preferences.workspace_fullscreen_shortcut == defaults.workspace_fullscreen_shortcut
            && preferences.workspace_density_shortcut == defaults.workspace_density_shortcut
            && preferences.workspace_zoom_in_shortcut == defaults.workspace_zoom_in_shortcut
            && preferences.workspace_zoom_out_shortcut == defaults.workspace_zoom_out_shortcut
            && preferences.command_palette_shortcut == defaults.command_palette_shortcut
            && preferences.voice == defaults.voice
    }

    fn sync_settings_reset_button_state(state: &SettingsWindowState) {
        let current = settings_snapshot_from_controls(state);
        unsafe {
            EnableWindow(
                GetDlgItem(state.window_hwnd, ID_SETTINGS_RESET as i32),
                if settings_match_defaults(&current) {
                    0
                } else {
                    1
                },
            );
        }
    }

    fn apply_live_settings_change(
        state: &mut SettingsWindowState,
        status: &str,
        refresh_runtime_preview: bool,
        refresh_parent: bool,
    ) {
        let next = settings_snapshot_from_controls(state);
        let changed = next != state.preference_store.load();
        if changed {
            state.preference_store.save(&next);
        }
        if refresh_runtime_preview {
            refresh_settings_runtime_preview(state);
        }
        set_settings_status(state, status);
        sync_settings_reset_button_state(state);

        if refresh_parent && let Some(parent_state) = unsafe { state_mut(state.parent_hwnd) } {
            refresh_state(state.parent_hwnd, parent_state);
        }
    }

    fn reset_settings(_hwnd: HWND, state: &mut SettingsWindowState) {
        let mut preferences = state.preference_store.load();
        let defaults = AppPreferences::default();
        preferences.default_theme = defaults.default_theme;
        preferences.default_density = defaults.default_density;
        preferences.close_to_background = defaults.close_to_background;
        preferences.windows_wsl_distribution = defaults.windows_wsl_distribution;
        preferences.workspace_fullscreen_shortcut = defaults.workspace_fullscreen_shortcut;
        preferences.workspace_density_shortcut = defaults.workspace_density_shortcut;
        preferences.workspace_zoom_in_shortcut = defaults.workspace_zoom_in_shortcut;
        preferences.workspace_zoom_out_shortcut = defaults.workspace_zoom_out_shortcut;
        preferences.command_palette_shortcut = defaults.command_palette_shortcut;
        preferences.terminal_history_lines = defaults.terminal_history_lines;
        preferences.voice = defaults.voice;
        state.preference_store.save(&preferences);
        apply_preferences_to_settings_controls(state, &preferences);
        refresh_settings_runtime_preview(state);
        set_settings_status(state, "Defaults restored. Changes are live.");

        if let Some(parent_state) = unsafe { state_mut(state.parent_hwnd) } {
            refresh_state(state.parent_hwnd, parent_state);
        }
    }

    fn reset_builtin_presets_from_settings(hwnd: HWND, state: &mut SettingsWindowState) {
        let response = unsafe {
            MessageBoxW(
                hwnd,
                wide(
                    "Restore the factory versions of TerminalTiler's default saved presets?\r\n\r\nYour custom presets will be kept.",
                )
                .as_ptr(),
                wide("Reset Default Saved Presets").as_ptr(),
                MB_OKCANCEL | MB_ICONWARNING,
            )
        };
        if response != IDOK {
            return;
        }

        if let Some(parent_state) = unsafe { state_mut(state.parent_hwnd) } {
            match parent_state.preset_store.reset_builtin_presets() {
                Ok(()) => {
                    refresh_state(state.parent_hwnd, parent_state);
                    set_settings_status(state, "Default saved presets restored. Changes are live.");
                    logging::info("reset builtin saved presets to factory defaults on Windows");
                }
                Err(error) => {
                    let message = format!("Could not restore default saved presets:\r\n{error}");
                    unsafe {
                        MessageBoxW(
                            hwnd,
                            wide(&message).as_ptr(),
                            wide("Reset Default Saved Presets").as_ptr(),
                            MB_OK | MB_ICONWARNING,
                        );
                    }
                    logging::error(format!(
                        "failed to reset builtin saved presets on Windows: {error}"
                    ));
                }
            }
        }
    }

    fn apply_preferences_to_settings_controls(
        state: &mut SettingsWindowState,
        preferences: &AppPreferences,
    ) {
        state.current_fullscreen_shortcut = preferences.workspace_fullscreen_shortcut.clone();
        state.current_density_shortcut = preferences.workspace_density_shortcut.clone();
        state.current_zoom_in_shortcut = preferences.workspace_zoom_in_shortcut.clone();
        state.current_zoom_out_shortcut = preferences.workspace_zoom_out_shortcut.clone();
        state.current_command_palette_shortcut = preferences.command_palette_shortcut.clone();
        state.current_voice_hotkey = preferences.voice.hotkey.clone();
        state.recording_shortcut = None;
        select_listbox_index(
            state.theme_list_hwnd,
            theme_index(preferences.default_theme),
        );
        select_listbox_index(
            state.density_list_hwnd,
            density_index(preferences.default_density),
        );
        unsafe {
            SendMessageW(
                state.close_background_hwnd,
                BM_SETCHECK,
                if preferences.close_to_background {
                    CHECKBOX_CHECKED
                } else {
                    CHECKBOX_UNCHECKED
                },
                0,
            );
            SendMessageW(
                state.voice_enabled_hwnd,
                BM_SETCHECK,
                if preferences.voice.enabled {
                    CHECKBOX_CHECKED
                } else {
                    CHECKBOX_UNCHECKED
                },
                0,
            );
            SendMessageW(
                state.voice_global_hwnd,
                BM_SETCHECK,
                if preferences.voice.prefer_global_hotkey {
                    CHECKBOX_CHECKED
                } else {
                    CHECKBOX_UNCHECKED
                },
                0,
            );
            select_combo_index(
                state.voice_activation_hwnd,
                voice_activation_index(preferences.voice.activation_mode),
            );
            select_combo_index(
                state.voice_engine_hwnd,
                voice_engine_index(preferences.voice.engine_mode),
            );
            select_voice_microphone(state, preferences.voice.microphone_id.as_deref());
            select_voice_microphone(state, preferences.voice.microphone_id.as_deref());
            SetWindowTextW(
                state.distro_hwnd,
                wide(
                    preferences
                        .windows_wsl_distribution
                        .as_deref()
                        .unwrap_or(""),
                )
                .as_ptr(),
            );
            SetWindowTextW(
                state.fullscreen_shortcut_hwnd,
                wide(&shortcut_capture::display_label(
                    &preferences.workspace_fullscreen_shortcut,
                ))
                .as_ptr(),
            );
            SetWindowTextW(
                state.density_shortcut_hwnd,
                wide(&shortcut_capture::display_label(
                    &preferences.workspace_density_shortcut,
                ))
                .as_ptr(),
            );
            SetWindowTextW(
                state.zoom_in_shortcut_hwnd,
                wide(&shortcut_capture::display_label(
                    &preferences.workspace_zoom_in_shortcut,
                ))
                .as_ptr(),
            );
            SetWindowTextW(
                state.zoom_out_shortcut_hwnd,
                wide(&shortcut_capture::display_label(
                    &preferences.workspace_zoom_out_shortcut,
                ))
                .as_ptr(),
            );
            SetWindowTextW(
                state.command_palette_shortcut_hwnd,
                wide(&shortcut_capture::display_label(
                    &preferences.command_palette_shortcut,
                ))
                .as_ptr(),
            );
            SetWindowTextW(
                state.voice_hotkey_hwnd,
                wide(&shortcut_capture::display_label(&preferences.voice.hotkey)).as_ptr(),
            );
            SetWindowTextW(
                state.voice_pack_status_hwnd,
                wide(&preferences.voice.pack_status.summary()).as_ptr(),
            );
        }
        sync_voice_pack_progress_controls(state, &preferences.voice.pack_status);
        set_settings_status(state, default_settings_status());
        update_shortcut_record_button_labels(state.window_hwnd, state);
        sync_settings_reset_button_state(state);
    }

    fn sync_voice_pack_progress_controls(state: &SettingsWindowState, status: &VoicePackStatus) {
        unsafe {
            match status {
                VoicePackStatus::Downloading { percent } => {
                    let bounded_percent = (*percent).clamp(1, 99);
                    SendMessageW(
                        state.voice_pack_progress_hwnd,
                        PBM_SETPOS,
                        bounded_percent as WPARAM,
                        0,
                    );
                    ShowWindow(
                        GetDlgItem(state.window_hwnd, ID_SETTINGS_VOICE_PACK_INSTALL as i32),
                        SW_HIDE,
                    );
                    ShowWindow(state.voice_pack_progress_hwnd, SW_SHOW);
                }
                _ => {
                    ShowWindow(state.voice_pack_progress_hwnd, SW_HIDE);
                    ShowWindow(
                        GetDlgItem(state.window_hwnd, ID_SETTINGS_VOICE_PACK_INSTALL as i32),
                        SW_SHOW,
                    );
                }
            }
        }
    }

    fn create_combo_box(hwnd: HWND, control_id: isize) -> HWND {
        unsafe {
            CreateWindowExW(
                0,
                wide("COMBOBOX").as_ptr(),
                wide("").as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32,
                0,
                0,
                0,
                0,
                hwnd,
                control_id as HMENU,
                GetModuleHandleW(ptr::null()),
                ptr::null(),
            )
        }
    }

    unsafe fn state_mut(hwnd: HWND) -> Option<&'static mut AppWindowState> {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut AppWindowState;
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *ptr })
        }
    }

    unsafe fn settings_state_mut(hwnd: HWND) -> Option<&'static mut SettingsWindowState> {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut SettingsWindowState;
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *ptr })
        }
    }

    fn fill_wide_buffer(buffer: &mut [u16], value: &str) {
        if buffer.is_empty() {
            return;
        }
        let wide = wide(value);
        let copy_len = wide.len().min(buffer.len());
        buffer[..copy_len].copy_from_slice(&wide[..copy_len]);
        if copy_len < buffer.len() {
            buffer[copy_len..].fill(0);
        } else {
            buffer[buffer.len() - 1] = 0;
        }
    }

    fn populate_listbox_items(hwnd: HWND, items: &[&str]) {
        unsafe {
            SendMessageW(hwnd, LB_RESETCONTENT, 0, 0);
            for item in items {
                SendMessageW(hwnd, LB_ADDSTRING, 0, wide(item).as_ptr() as LPARAM);
            }
        }
    }

    fn populate_combo_box_items(hwnd: HWND, items: &[&str]) {
        unsafe {
            SendMessageW(hwnd, CB_RESETCONTENT, 0, 0);
            for item in items {
                SendMessageW(hwnd, CB_ADDSTRING, 0, wide(item).as_ptr() as LPARAM);
            }
        }
    }

    fn populate_voice_microphones(state: &mut SettingsWindowState, selected_id: Option<&str>) {
        state.voice_microphone_ids.clear();
        unsafe {
            SendMessageW(state.voice_microphone_hwnd, CB_RESETCONTENT, 0, 0);
            SendMessageW(
                state.voice_microphone_hwnd,
                CB_ADDSTRING,
                0,
                wide("System default").as_ptr() as LPARAM,
            );
        }
        state.voice_microphone_ids.push(None);

        let mut selected_index = 0usize;
        match AudioCapture::enumerate_microphones() {
            Ok(devices) => {
                for microphone in devices {
                    let label = if microphone.is_default {
                        format!("{} (default)", microphone.name)
                    } else {
                        microphone.name.clone()
                    };
                    unsafe {
                        SendMessageW(
                            state.voice_microphone_hwnd,
                            CB_ADDSTRING,
                            0,
                            wide(&label).as_ptr() as LPARAM,
                        );
                    }
                    state.voice_microphone_ids.push(Some(microphone.id.clone()));
                    if selected_id == Some(microphone.id.as_str()) {
                        selected_index = state.voice_microphone_ids.len() - 1;
                    }
                }
            }
            Err(error) => {
                logging::error(format!(
                    "failed to enumerate Windows microphones: {error:?}"
                ));
            }
        }

        if let Some(selected_id) = selected_id.filter(|value| !value.trim().is_empty())
            && selected_index == 0
        {
            let label = format!("Saved device: {selected_id}");
            unsafe {
                SendMessageW(
                    state.voice_microphone_hwnd,
                    CB_ADDSTRING,
                    0,
                    wide(&label).as_ptr() as LPARAM,
                );
            }
            state
                .voice_microphone_ids
                .push(Some(selected_id.to_string()));
            selected_index = state.voice_microphone_ids.len() - 1;
        }

        select_combo_index(state.voice_microphone_hwnd, selected_index);
    }

    fn select_voice_microphone(state: &SettingsWindowState, selected_id: Option<&str>) {
        let selected_index = state
            .voice_microphone_ids
            .iter()
            .position(|id| id.as_deref() == selected_id)
            .unwrap_or(0);
        select_combo_index(state.voice_microphone_hwnd, selected_index);
    }

    fn selected_voice_microphone_id(state: &SettingsWindowState) -> Option<String> {
        state
            .voice_microphone_ids
            .get(selected_combo_index(state.voice_microphone_hwnd))
            .cloned()
            .flatten()
            .filter(|value| !value.trim().is_empty())
    }

    fn populate_template_list(state: &AppWindowState) {
        unsafe {
            SendMessageW(state.template_list_hwnd, LB_RESETCONTENT, 0, 0);
            for template in &state.templates {
                let label = format!(
                    "{}  •  {}  •  {} tiles",
                    template.label, template.subtitle, template.tile_count
                );
                SendMessageW(
                    state.template_list_hwnd,
                    LB_ADDSTRING,
                    0,
                    wide(&label).as_ptr() as LPARAM,
                );
            }
            if !state.templates.is_empty() {
                SendMessageW(state.template_list_hwnd, LB_SETCURSEL, 0, 0);
            }
        }
    }

    fn populate_preset_list(state: &AppWindowState) {
        unsafe {
            SendMessageW(state.preset_list_hwnd, LB_RESETCONTENT, 0, 0);
            for preset in &state.presets {
                let label = format!(
                    "{}  •  {} tiles",
                    preset.name,
                    preset.layout.tile_specs().len()
                );
                SendMessageW(
                    state.preset_list_hwnd,
                    LB_ADDSTRING,
                    0,
                    wide(&label).as_ptr() as LPARAM,
                );
            }
            if !state.presets.is_empty() {
                SendMessageW(state.preset_list_hwnd, LB_SETCURSEL, 0, 0);
            }
        }
    }

    fn populate_suggestion_list(state: &AppWindowState) {
        unsafe {
            SendMessageW(state.suggestion_list_hwnd, LB_RESETCONTENT, 0, 0);
            for suggestion in &state.suggestions {
                let label = format!(
                    "{}  •  {} tiles  •  {}",
                    suggestion.title,
                    suggestion.tile_count,
                    suggestion.tags.join(", ")
                );
                SendMessageW(
                    state.suggestion_list_hwnd,
                    LB_ADDSTRING,
                    0,
                    wide(&label).as_ptr() as LPARAM,
                );
            }
            if !state.suggestions.is_empty() {
                SendMessageW(state.suggestion_list_hwnd, LB_SETCURSEL, 0, 0);
            }
        }
    }

    fn refresh_suggestions(state: &mut AppWindowState) {
        state.suggestions = current_workspace_root(state)
            .map(|workspace_root| detect_project_suggestions(&workspace_root))
            .unwrap_or_default();
        populate_suggestion_list(state);
    }

    fn apply_selected_suggestion(state: &mut AppWindowState) {
        if state.suggestions.is_empty() {
            return;
        }
        let index = selected_listbox_index(state.suggestion_list_hwnd)
            .min(state.suggestions.len().saturating_sub(1));
        let Some(suggestion) = state.suggestions.get(index).cloned() else {
            return;
        };
        let assets = current_launcher_assets(state);
        state.active_layout = apply_project_suggestion(&state.active_layout, &suggestion, &assets);
        unsafe {
            SetWindowTextW(state.session_name_hwnd, wide(&suggestion.title).as_ptr());
            SetWindowTextW(
                state.tile_count_hwnd,
                wide(&suggestion.tile_count.to_string()).as_ptr(),
            );
        }
        sync_launcher_editor(state);
        sync_status_text(state);
    }

    fn open_assets_manager(hwnd: HWND, state: &mut AppWindowState) {
        let workspace_root = current_workspace_root(state);
        let on_saved = Rc::new(move || {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                refresh_state(hwnd, state);
            }
        });
        let _ = assets_manager::present(hwnd, state.asset_store.clone(), workspace_root, on_saved);
    }

    fn open_launcher_editor(hwnd: HWND, state: &mut AppWindowState) {
        if !state.launcher_editor_hwnd.is_null() {
            unsafe {
                ShowWindow(state.launcher_editor_hwnd, SW_SHOW);
                SetForegroundWindow(state.launcher_editor_hwnd);
            }
            sync_launcher_editor(state);
            return;
        }

        let on_layout_changed = Rc::new(move |layout: LayoutNode| {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.active_layout = layout;
                unsafe {
                    SetWindowTextW(
                        state.tile_count_hwnd,
                        wide(&state.active_layout.tile_count().to_string()).as_ptr(),
                    );
                }
                sync_status_text(state);
            }
        });
        let on_closed = Rc::new(move || {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.launcher_editor_hwnd = ptr::null_mut();
            }
        });

        match launcher_editor::present(
            hwnd,
            state.active_layout.clone(),
            current_launcher_assets(state),
            on_layout_changed,
            on_closed,
        ) {
            Ok(editor_hwnd) => {
                state.launcher_editor_hwnd = editor_hwnd;
            }
            Err(error) => {
                let status = format!("Could not open tile editor:\r\n{error}");
                unsafe {
                    SetWindowTextW(state.status_hwnd, wide(&status).as_ptr());
                }
                logging::error(format!("could not open tile editor: {error}"));
            }
        }
    }

    fn open_command_palette(hwnd: HWND, state: &mut AppWindowState) {
        let mut actions = Vec::new();
        let product_info = state.runtime_options.product.clone();
        actions.push(command_palette::PaletteAction {
            title: format!("About {}", state.runtime_options.product.display_name),
            subtitle: "Version, license, source, and open-core model.".into(),
            on_activate: Rc::new(move || show_about_dialog(hwnd, &product_info)),
        });
        actions.push(command_palette::PaletteAction {
            title: "Refresh Runtime".into(),
            subtitle: "Probe WSL and PowerShell availability again.".into(),
            on_activate: Rc::new(move || {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    refresh_state(hwnd, state);
                }
            }),
        });
        actions.push(command_palette::PaletteAction {
            title: "Open Settings".into(),
            subtitle: "Adjust launcher and workspace preferences.".into(),
            on_activate: Rc::new(move || {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    open_settings_dialog(hwnd, state);
                }
            }),
        });
        actions.push(command_palette::PaletteAction {
            title: "Open Assets Manager".into(),
            subtitle: "Edit global and workspace-local connection and role assets.".into(),
            on_activate: Rc::new(move || {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    open_assets_manager(hwnd, state);
                }
            }),
        });
        if state.runtime_options.companion.is_some() {
            actions.push(command_palette::PaletteAction {
                title: "Open Account / Sync".into(),
                subtitle: "Account, activation, device, and sync controls.".into(),
                on_activate: Rc::new(move || {
                    if let Some(state) = unsafe { state_mut(hwnd) } {
                        show_companion_dialog(hwnd, state);
                    }
                }),
            });
        }
        actions.push(command_palette::PaletteAction {
            title: "Edit Tiles".into(),
            subtitle: "Adjust tile titles, roles, connections, and startup commands.".into(),
            on_activate: Rc::new(move || {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    open_launcher_editor(hwnd, state);
                }
            }),
        });
        if let Some(preset) = launcher_preset_snapshot(state) {
            let preset_name = preset.name.clone();
            actions.push(command_palette::PaletteAction {
                title: format!("Launch Workspace: {preset_name}"),
                subtitle: "Open the current launcher draft as a new workspace window.".into(),
                on_activate: Rc::new(move || {
                    if let Some(state) = unsafe { state_mut(hwnd) } {
                        launch_selected_preset(hwnd, state);
                    }
                }),
            });
        }
        for suggestion in state.suggestions.iter().cloned() {
            let title = suggestion.title.clone();
            actions.push(command_palette::PaletteAction {
                title: format!("Apply Suggestion: {title}"),
                subtitle: suggestion.description.clone(),
                on_activate: Rc::new(move || {
                    if let Some(state) = unsafe { state_mut(hwnd) }
                        && let Some(index) = state
                            .suggestions
                            .iter()
                            .position(|candidate| candidate.id == suggestion.id)
                    {
                        unsafe {
                            SendMessageW(state.suggestion_list_hwnd, LB_SETCURSEL, index, 0);
                        }
                        apply_selected_suggestion(state);
                    }
                }),
            });
        }
        let _ = command_palette::present(hwnd, "Command Palette", actions);
    }

    fn show_companion_dialog(parent_hwnd: HWND, state: &mut AppWindowState) {
        let Some(companion) = state.runtime_options.companion.clone() else {
            return;
        };
        let snapshot = companion.snapshot();
        let body = companion_snapshot_text(&snapshot);
        unsafe {
            MessageBoxW(
                parent_hwnd,
                wide(&body).as_ptr(),
                wide(&snapshot.title).as_ptr(),
                MB_OK,
            );
        }

        if let Some(action) = preferred_followup_action(&snapshot) {
            invoke_companion_action(parent_hwnd, state, companion, action);
        }
    }

    fn companion_snapshot_text(snapshot: &CompanionPanelSnapshot) -> String {
        let mut lines = vec![
            format!("Status: {}", snapshot.status.label()),
            snapshot.subtitle.clone(),
            String::new(),
        ];
        append_companion_rows(&mut lines, "Account", &snapshot.account_rows);
        append_companion_rows(&mut lines, "Sync", &snapshot.sync_rows);
        append_companion_rows(&mut lines, "Devices and teams", &snapshot.device_rows);
        if !snapshot.actions.is_empty() {
            lines.push("Actions:".to_string());
            for action in &snapshot.actions {
                lines.push(format!(
                    "- {}{}",
                    action.label,
                    action
                        .detail
                        .as_deref()
                        .map(|d| format!(": {d}"))
                        .unwrap_or_default()
                ));
            }
            lines.push(String::new());
            lines.push(
                "Primary activation or refresh actions run after this dialog when applicable."
                    .to_string(),
            );
        }
        lines.join("\r\n")
    }

    fn append_companion_rows(
        lines: &mut Vec<String>,
        title: &str,
        rows: &[crate::extension::CompanionRow],
    ) {
        if rows.is_empty() {
            return;
        }
        lines.push(format!("{title}:"));
        for row in rows {
            lines.push(format!("  {}: {}", row.label, row.value));
            if let Some(detail) = &row.detail {
                lines.push(format!("    {detail}"));
            }
        }
        lines.push(String::new());
    }

    fn preferred_followup_action(snapshot: &CompanionPanelSnapshot) -> Option<CompanionAction> {
        snapshot
            .actions
            .iter()
            .find(|action| action.input.is_some())
            .or_else(|| {
                snapshot
                    .actions
                    .iter()
                    .find(|action| action.id == "refresh" || action.id == "sync_now")
            })
            .cloned()
    }

    fn invoke_companion_action(
        parent_hwnd: HWND,
        state: &mut AppWindowState,
        companion: std::sync::Arc<dyn CompanionIntegration>,
        action: CompanionAction,
    ) {
        if state.companion_action_running {
            return;
        }
        if let Some(url) = action.external_url.as_deref() {
            unsafe {
                MessageBoxW(
                    parent_hwnd,
                    wide(url).as_ptr(),
                    wide(&action.label).as_ptr(),
                    MB_OK,
                );
            }
            return;
        }
        let input = if let Some(prompt) = action.input.as_ref() {
            let Some(text) = prompt_text_input(
                parent_hwnd,
                &action.label,
                &prompt.prompt,
                prompt.placeholder.as_deref(),
                !prompt.secret,
            ) else {
                return;
            };
            CompanionActionInput { text: Some(text) }
        } else {
            CompanionActionInput::default()
        };
        state.companion_action_running = true;
        state.companion_action_generation = state.companion_action_generation.wrapping_add(1);
        let generation = state.companion_action_generation;
        let timeout = action.timeout;
        let completion_hwnd = parent_hwnd as isize;
        let action_id = action.id.clone();
        let label = action.label.clone();
        thread::spawn(move || {
            let completion = Box::new(CompanionActionCompletion {
                generation,
                label,
                result: companion.invoke(&action_id, input),
            });
            post_companion_completion(completion_hwnd as HWND, completion);
        });
        let label = action.label;
        let timeout_hwnd = parent_hwnd as isize;
        thread::spawn(move || {
            thread::sleep(timeout);
            let completion = Box::new(CompanionActionCompletion {
                generation,
                label,
                result: Err(format!(
                    "Companion action timed out after {} seconds",
                    timeout.as_secs()
                )),
            });
            post_companion_completion(timeout_hwnd as HWND, completion);
        });
    }

    fn post_companion_completion(hwnd: HWND, completion: Box<CompanionActionCompletion>) {
        let raw = Box::into_raw(completion);
        if unsafe { PostMessageW(hwnd, WM_COMPANION_ACTION_COMPLETE, 0, raw as LPARAM) } == 0 {
            unsafe { drop(Box::from_raw(raw)) };
        }
    }

    fn prompt_text_input(
        parent_hwnd: HWND,
        title: &str,
        prompt: &str,
        placeholder: Option<&str>,
        visible: bool,
    ) -> Option<String> {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return None;
        }
        let state = Box::new(PromptWindowState {
            done: false,
            submitted: false,
            prompt: prompt.to_string(),
            placeholder: placeholder.unwrap_or("").to_string(),
            visible,
            edit_hwnd: ptr::null_mut(),
            result: None,
        });
        let state_ptr = Box::into_raw(state);
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                wide(COMPANION_PROMPT_CLASS).as_ptr(),
                wide(title).as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                560,
                190,
                parent_hwnd,
                ptr::null_mut(),
                instance,
                state_ptr.cast(),
            )
        };
        if hwnd.is_null() {
            unsafe {
                drop(Box::from_raw(state_ptr));
            }
            return None;
        }
        unsafe {
            EnableWindow(parent_hwnd, 0);
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);
        }
        let mut message = unsafe { mem::zeroed::<MSG>() };
        loop {
            if unsafe { (*state_ptr).done } {
                break;
            }
            let got = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
            if got <= 0 {
                break;
            }
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        unsafe {
            EnableWindow(parent_hwnd, 1);
            SetForegroundWindow(parent_hwnd);
            let state = Box::from_raw(state_ptr);
            if state.submitted {
                state.result.filter(|value| !value.trim().is_empty())
            } else {
                None
            }
        }
    }

    unsafe extern "system" fn prompt_window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            crate::windows::win32_helpers::catch_window_proc(
                "prompt_window_proc",
                hwnd,
                message,
                wparam,
                lparam,
                || prompt_window_proc_impl(hwnd, message, wparam, lparam),
            )
        }
    }

    unsafe fn prompt_window_proc_impl(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_NCCREATE => {
                let create = lparam as *const CREATESTRUCTW;
                if create.is_null() {
                    return 0;
                }
                let state_ptr = unsafe { (*create).lpCreateParams as *mut PromptWindowState };
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
                }
                1
            }
            WM_CREATE => {
                if let Some(state) = unsafe { prompt_state_mut(hwnd) } {
                    create_prompt_controls(hwnd, state);
                }
                0
            }
            WM_SIZE => {
                if let Some(state) = unsafe { prompt_state_mut(hwnd) } {
                    layout_prompt_controls(hwnd, state);
                }
                0
            }
            WM_COMMAND => {
                let command_id = (wparam & 0xffff) as isize;
                if let Some(state) = unsafe { prompt_state_mut(hwnd) } {
                    match command_id {
                        ID_PROMPT_OK => {
                            state.result =
                                Some(read_window_text(state.edit_hwnd).trim().to_string());
                            state.submitted = true;
                            state.done = true;
                            unsafe { DestroyWindow(hwnd) };
                        }
                        ID_PROMPT_CANCEL => {
                            state.done = true;
                            unsafe { DestroyWindow(hwnd) };
                        }
                        _ => {}
                    }
                }
                0
            }
            WM_CLOSE => {
                if let Some(state) = unsafe { prompt_state_mut(hwnd) } {
                    state.done = true;
                }
                unsafe { DestroyWindow(hwnd) };
                0
            }
            WM_DESTROY => {
                if let Some(state) = unsafe { prompt_state_mut(hwnd) } {
                    state.done = true;
                }
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    fn create_prompt_controls(hwnd: HWND, state: &mut PromptWindowState) {
        let _ = create_child_window(
            hwnd,
            "STATIC",
            &state.prompt,
            WS_CHILD | WS_VISIBLE,
            0,
            ID_PROMPT_LABEL,
        );
        let edit_style = WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WS_BORDER
            | ES_LEFT as u32
            | ES_AUTOHSCROLL as u32
            | if state.visible { 0 } else { ES_PASSWORD as u32 };
        state.edit_hwnd = create_child_window(
            hwnd,
            "EDIT",
            &state.placeholder,
            edit_style,
            0,
            ID_PROMPT_EDIT,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "OK",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_PROMPT_OK,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Cancel",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_PROMPT_CANCEL,
        );
        let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
        for control in [
            unsafe { GetDlgItem(hwnd, ID_PROMPT_LABEL as i32) },
            state.edit_hwnd,
            unsafe { GetDlgItem(hwnd, ID_PROMPT_OK as i32) },
            unsafe { GetDlgItem(hwnd, ID_PROMPT_CANCEL as i32) },
        ] {
            if !control.is_null() {
                unsafe {
                    SendMessageW(control, WM_SETFONT, font as usize, 1);
                }
            }
        }
        layout_prompt_controls(hwnd, state);
        unsafe {
            SetFocus(state.edit_hwnd);
        }
    }

    fn layout_prompt_controls(hwnd: HWND, state: &PromptWindowState) {
        let mut rect = unsafe { mem::zeroed() };
        unsafe {
            GetClientRect(hwnd, &mut rect);
        }
        let width = rect.right - rect.left;
        let content_width = width - (MARGIN * 2);
        unsafe {
            SetWindowPos(
                GetDlgItem(hwnd, ID_PROMPT_LABEL as i32),
                ptr::null_mut(),
                MARGIN,
                MARGIN,
                content_width,
                44,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.edit_hwnd,
                ptr::null_mut(),
                MARGIN,
                MARGIN + 50,
                content_width,
                FIELD_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_PROMPT_OK as i32),
                ptr::null_mut(),
                width - MARGIN - 196,
                MARGIN + 90,
                88,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_PROMPT_CANCEL as i32),
                ptr::null_mut(),
                width - MARGIN - 96,
                MARGIN + 90,
                96,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
        }
    }

    unsafe fn prompt_state_mut(hwnd: HWND) -> Option<&'static mut PromptWindowState> {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut PromptWindowState;
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *ptr })
        }
    }

    fn show_about_dialog(parent_hwnd: HWND, product: &ProductInfo) {
        let mut lines = vec![format!("{} v{}", product.display_name, product.version)];
        if let Some(copyright) = product.copyright.as_deref() {
            lines.push(copyright.to_string());
        }
        if let Some(license) = product.license_name.as_deref() {
            lines.push(license.to_string());
        }
        if let Some(copy) = product.about_copy.as_deref() {
            lines.extend([String::new(), copy.to_string()]);
        }
        if let Some(copy) = product.about_extra_copy.as_deref() {
            lines.extend([String::new(), copy.to_string()]);
        }
        lines.extend([
            String::new(),
            format!("Website: {}", product.homepage_url),
            format!("Account: {}", product.account_url),
            format!("Support: {}", product.support_url),
            format!("Privacy: {}", product.privacy_url),
            format!("Terms: {}", product.terms_url),
        ]);
        if let Some(source) = product.source_url.as_deref() {
            lines.push(format!("Source: {source}"));
        }
        if let Some(issues) = product.issues_url.as_deref() {
            lines.push(format!("Issues: {issues}"));
        }
        let body = lines.join("\r\n");
        let title = format!("About {}", product.display_name);
        unsafe {
            MessageBoxW(
                parent_hwnd,
                wide(&body).as_ptr(),
                wide(&title).as_ptr(),
                MB_OK,
            );
        }
    }

    fn handle_shell_shortcuts(hwnd: HWND, state: &mut AppWindowState, virtual_key: u32) -> bool {
        let preferences = state.preference_store.load();
        if shortcut_capture::matches_keydown(&preferences.command_palette_shortcut, virtual_key) {
            open_command_palette(hwnd, state);
            return true;
        }
        false
    }

    fn apply_launcher_selection(state: &mut AppWindowState) {
        let was_syncing = state.syncing_launcher_controls;
        state.syncing_launcher_controls = true;

        apply_launcher_selection_controls(state);
        sync_launch_appearance_controls(state);
        sync_launcher_editor(state);
        update_preset_action_buttons(state);
        sync_status_text(state);

        state.syncing_launcher_controls = was_syncing;
    }

    fn apply_launcher_selection_controls(state: &mut AppWindowState) {
        let selected_source = match state.selected_source {
            LaunchSelection::Preset(_) if state.presets.is_empty() => LaunchSelection::Template(0),
            selected_source => selected_source,
        };

        match selected_source {
            LaunchSelection::Template(index) => {
                let resolved = index.min(state.templates.len().saturating_sub(1));
                state.selected_source = LaunchSelection::Template(resolved);
                if let Some(template) = state.templates.get(resolved) {
                    state.active_layout = generate_layout(template.tile_count);
                    state.active_theme = state.preference_store.load().default_theme;
                    state.active_density = state.preference_store.load().default_density;
                    unsafe {
                        SendMessageW(state.template_list_hwnd, LB_SETCURSEL, resolved, 0);
                        SendMessageW(state.preset_list_hwnd, LB_SETCURSEL, usize::MAX, 0);
                        SetWindowTextW(state.session_name_hwnd, wide(template.label).as_ptr());
                        SetWindowTextW(
                            state.tile_count_hwnd,
                            wide(&template.tile_count.to_string()).as_ptr(),
                        );
                    }
                }
            }
            LaunchSelection::Preset(index) => {
                let resolved = index.min(state.presets.len().saturating_sub(1));
                state.selected_source = LaunchSelection::Preset(resolved);
                if let Some(preset) = state.presets.get(resolved) {
                    state.active_layout = preset.layout.clone();
                    state.active_theme = preset.theme;
                    state.active_density = preset.density;
                    unsafe {
                        SendMessageW(state.preset_list_hwnd, LB_SETCURSEL, resolved, 0);
                        SendMessageW(state.template_list_hwnd, LB_SETCURSEL, usize::MAX, 0);
                        SetWindowTextW(state.session_name_hwnd, wide(&preset.name).as_ptr());
                        SetWindowTextW(
                            state.tile_count_hwnd,
                            wide(&preset.layout.tile_count().to_string()).as_ptr(),
                        );
                        if let Some(workspace_root) = preset.workspace_root.as_ref() {
                            SetWindowTextW(
                                state.workspace_path_hwnd,
                                wide(&workspace_root.display().to_string()).as_ptr(),
                            );
                        }
                    }
                }
            }
        }
    }

    fn sync_status_text(state: &AppWindowState) {
        sync_selection_summary(state);
        let preferences = state.preference_store.load();
        let status_text = build_status_text(state, preferences.windows_wsl_distribution.as_deref());
        unsafe {
            SetWindowTextW(state.status_hwnd, wide(&status_text).as_ptr());
        }
    }

    fn sync_tile_count_from_input(state: &mut AppWindowState) {
        let requested = read_window_text(state.tile_count_hwnd);
        let Ok(tile_count) = requested.trim().parse::<usize>() else {
            return;
        };
        let tile_count = tile_count.clamp(1, 16);
        state.active_layout = resize_layout(&state.active_layout, tile_count);
        unsafe {
            SetWindowTextW(
                state.tile_count_hwnd,
                wide(&tile_count.to_string()).as_ptr(),
            );
        }
        sync_launcher_editor(state);
        sync_status_text(state);
    }

    fn sync_launch_appearance_from_controls(state: &mut AppWindowState) {
        state.active_theme = theme_from_index(selected_combo_index(state.theme_combo_hwnd));
        state.active_density = density_from_index(selected_combo_index(state.density_combo_hwnd));
        sync_status_text(state);
    }

    fn sync_launch_appearance_controls(state: &AppWindowState) {
        if state.theme_combo_hwnd.is_null() || state.density_combo_hwnd.is_null() {
            return;
        }
        select_combo_index(state.theme_combo_hwnd, theme_index(state.active_theme));
        select_combo_index(
            state.density_combo_hwnd,
            density_index(state.active_density),
        );
    }

    fn sync_launcher_editor(state: &AppWindowState) {
        if state.launcher_editor_hwnd.is_null() {
            return;
        }
        launcher_editor::sync_draft_state(
            state.launcher_editor_hwnd,
            state.active_layout.clone(),
            current_launcher_assets(state),
        );
    }

    fn has_launcher_selection(state: &AppWindowState) -> bool {
        selected_template(state).is_some() || selected_preset(state).is_some()
    }

    fn selected_source_label(state: &AppWindowState) -> &'static str {
        match state.selected_source {
            LaunchSelection::Template(_) => "template",
            LaunchSelection::Preset(_) => "preset",
        }
    }

    fn selected_template(state: &AppWindowState) -> Option<&LayoutTemplate> {
        match state.selected_source {
            LaunchSelection::Template(index) => state.templates.get(index),
            LaunchSelection::Preset(_) => None,
        }
    }

    fn sync_selection_summary(state: &AppWindowState) {
        if state.selection_summary_hwnd.is_null() {
            return;
        }
        let summary = build_selection_summary_text(state);
        unsafe {
            SetWindowTextW(state.selection_summary_hwnd, wide(&summary).as_ptr());
        }
    }

    fn build_selection_summary_text(state: &AppWindowState) -> String {
        let launch_name = read_window_text(state.session_name_hwnd);
        let launch_name = launch_name.trim();
        match state.selected_source {
            LaunchSelection::Template(index) => {
                let Some(template) = state.templates.get(index) else {
                    return "Choose a template or preset to begin.".into();
                };
                let tile_summary = if state.active_layout.tile_count() != template.tile_count {
                    format!(
                        "customized from {} to {} tiles",
                        template.tile_count,
                        state.active_layout.tile_count()
                    )
                } else {
                    format!("{} tiles", template.tile_count)
                };
                if launch_name.is_empty() || launch_name == template.label {
                    format!(
                        "{} template, {}, {} theme / {} density",
                        template.label,
                        tile_summary,
                        state.active_theme.label(),
                        state.active_density.label()
                    )
                } else {
                    format!(
                        "{} template, {}, launches as '{}', {} theme / {} density",
                        template.label,
                        tile_summary,
                        launch_name,
                        state.active_theme.label(),
                        state.active_density.label()
                    )
                }
            }
            LaunchSelection::Preset(index) => {
                let Some(preset) = state.presets.get(index) else {
                    return "Choose a template or preset to begin.".into();
                };
                let tile_summary = if state.active_layout.tile_count() != preset.layout.tile_count()
                {
                    format!(
                        "customized from {} to {} tiles",
                        preset.layout.tile_count(),
                        state.active_layout.tile_count()
                    )
                } else {
                    format!("{} tiles", preset.layout.tile_count())
                };
                if launch_name.is_empty() || launch_name == preset.name {
                    format!(
                        "{} preset, {}, {} theme / {} density",
                        preset.name,
                        tile_summary,
                        state.active_theme.label(),
                        state.active_density.label()
                    )
                } else {
                    format!(
                        "{} preset, {}, launches as '{}', {} theme / {} density",
                        preset.name,
                        tile_summary,
                        launch_name,
                        state.active_theme.label(),
                        state.active_density.label()
                    )
                }
            }
        }
    }

    fn update_preset_action_buttons(state: &AppWindowState) {
        let has_selection = has_launcher_selection(state);
        let selected_is_builtin = selected_preset(state)
            .map(|preset| is_builtin_preset_id(&preset.id))
            .unwrap_or(false);
        let allow_update = selected_preset(state).is_some();

        unsafe {
            EnableWindow(state.save_preset_button_hwnd, has_selection as i32);
            EnableWindow(state.update_preset_button_hwnd, allow_update as i32);
            EnableWindow(state.edit_tiles_button_hwnd, has_selection as i32);
            EnableWindow(
                state.delete_preset_button_hwnd,
                (has_selection && !selected_is_builtin) as i32,
            );
            SetWindowTextW(
                state.update_preset_button_hwnd,
                wide(if !allow_update {
                    "Update Preset"
                } else if selected_is_builtin {
                    "Save Copy"
                } else {
                    "Update Preset"
                })
                .as_ptr(),
            );
        }
    }

    fn selected_preset(state: &AppWindowState) -> Option<&WorkspacePreset> {
        match state.selected_source {
            LaunchSelection::Preset(index) => state.presets.get(index),
            LaunchSelection::Template(_) => None,
        }
    }

    fn save_selected_preset_as_new(hwnd: HWND, state: &mut AppWindowState) {
        let Some(mut preset) = launcher_preset_snapshot(state) else {
            return;
        };

        let name = desired_preset_name(state, format!("{} Copy", preset.name));
        preset.id = unique_preset_id(&name);
        preset.name = name.clone();

        match state.preset_store.upsert_preset(preset) {
            Ok(()) => {
                refresh_state(hwnd, state);
                select_preset_by_id(state, &unique_preset_lookup_name(&state.presets, &name));
                apply_launcher_selection(state);
                update_preset_action_buttons(state);
                sync_status_text(state);
                unsafe {
                    SetWindowTextW(
                        state.status_hwnd,
                        wide(&format!("Saved preset copy '{}'.", name)).as_ptr(),
                    );
                }
                logging::info(format!("saved preset copy '{name}'"));
            }
            Err(error) => {
                let status = format!("Could not save preset copy:\r\n{error}");
                unsafe {
                    SetWindowTextW(state.status_hwnd, wide(&status).as_ptr());
                }
                logging::error(format!("could not save preset copy: {error}"));
            }
        }
    }

    fn update_selected_preset(hwnd: HWND, state: &mut AppWindowState) {
        let Some(selected) = selected_preset(state).cloned() else {
            return;
        };
        let Some(mut preset) = launcher_preset_snapshot(state) else {
            return;
        };

        let builtin = is_builtin_preset_id(&selected.id);
        let name = desired_preset_name(
            state,
            if builtin {
                format!("{} Copy", selected.name)
            } else {
                selected.name.clone()
            },
        );

        if builtin {
            preset.id = unique_preset_id(&name);
        } else {
            preset.id = selected.id.clone();
        }
        preset.name = name.clone();

        match state.preset_store.upsert_preset(preset) {
            Ok(()) => {
                refresh_state(hwnd, state);
                let target_id = if builtin {
                    state
                        .presets
                        .iter()
                        .find(|preset| preset.name == name)
                        .map(|preset| preset.id.clone())
                        .unwrap_or_default()
                } else {
                    selected.id.clone()
                };
                if !target_id.is_empty() {
                    select_preset_by_id(state, &target_id);
                }
                apply_launcher_selection(state);
                update_preset_action_buttons(state);
                let status = if builtin {
                    format!(
                        "Saved builtin preset '{}' as new preset '{}'.",
                        selected.name, name
                    )
                } else {
                    format!("Updated preset '{}'.", name)
                };
                unsafe {
                    SetWindowTextW(state.status_hwnd, wide(&status).as_ptr());
                }
                logging::info(status);
            }
            Err(error) => {
                let status = format!("Could not update preset:\r\n{error}");
                unsafe {
                    SetWindowTextW(state.status_hwnd, wide(&status).as_ptr());
                }
                logging::error(format!("could not update preset: {error}"));
            }
        }
    }

    fn delete_selected_preset(hwnd: HWND, state: &mut AppWindowState) {
        let Some(selected) = selected_preset(state).cloned() else {
            return;
        };
        if is_builtin_preset_id(&selected.id) {
            unsafe {
                SetWindowTextW(
                    state.status_hwnd,
                    wide("Builtin presets cannot be deleted. Save a copy instead.").as_ptr(),
                );
            }
            return;
        }

        let response = unsafe {
            MessageBoxW(
                hwnd,
                wide(&format!("Delete preset '{}' permanently?", selected.name)).as_ptr(),
                wide("Delete Preset").as_ptr(),
                MB_OKCANCEL | MB_ICONWARNING,
            )
        };
        if response != IDOK {
            return;
        }

        match state.preset_store.delete_preset(&selected.id) {
            Ok(()) => {
                refresh_state(hwnd, state);
                apply_launcher_selection(state);
                update_preset_action_buttons(state);
                unsafe {
                    SetWindowTextW(
                        state.status_hwnd,
                        wide(&format!("Deleted preset '{}'.", selected.name)).as_ptr(),
                    );
                }
                logging::info(format!("deleted preset '{}'", selected.name));
            }
            Err(error) => {
                let status = format!("Could not delete preset:\r\n{error}");
                unsafe {
                    SetWindowTextW(state.status_hwnd, wide(&status).as_ptr());
                }
                logging::error(format!("could not delete preset: {error}"));
            }
        }
    }

    fn launcher_preset_snapshot(state: &AppWindowState) -> Option<WorkspacePreset> {
        let mut preset = match state.selected_source {
            LaunchSelection::Preset(index) => {
                let mut preset = state.presets.get(index)?.clone();
                preset.layout = state.active_layout.clone();
                preset
            }
            LaunchSelection::Template(index) => {
                let template = state.templates.get(index)?;
                WorkspacePreset {
                    id: format!("template-{}", template.tile_count),
                    name: template.label.to_string(),
                    description: template.subtitle.to_string(),
                    tags: vec!["template".into(), "windows".into()],
                    root_label: "Workspace root".into(),
                    workspace_root: preset_workspace_root(state),
                    theme: state.active_theme,
                    density: state.active_density,
                    layout: state.active_layout.clone(),
                }
            }
        };
        preset.theme = state.active_theme;
        preset.density = state.active_density;
        preset.workspace_root = preset_workspace_root(state);
        let desired_name = read_window_text(state.session_name_hwnd);
        let desired_name = desired_name.trim();
        if !desired_name.is_empty() {
            preset.name = desired_name.to_string();
        }
        Some(preset)
    }

    fn desired_preset_name(state: &AppWindowState, fallback: String) -> String {
        let candidate = read_window_text(state.session_name_hwnd);
        let candidate = candidate.trim();
        if candidate.is_empty() {
            fallback
        } else {
            candidate.to_string()
        }
    }

    fn select_preset_by_id(state: &mut AppWindowState, preset_id: &str) {
        if let Some(index) = state
            .presets
            .iter()
            .position(|preset| preset.id == preset_id)
        {
            state.selected_source = LaunchSelection::Preset(index);
            unsafe {
                SendMessageW(state.preset_list_hwnd, LB_SETCURSEL, index, 0);
            }
        }
    }

    fn unique_preset_lookup_name(presets: &[WorkspacePreset], name: &str) -> String {
        presets
            .iter()
            .find(|preset| preset.name == name)
            .map(|preset| preset.id.clone())
            .unwrap_or_default()
    }

    fn slugify(name: &str) -> String {
        let slug = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>();
        let segments = slug
            .split('-')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if segments.is_empty() {
            "preset".to_string()
        } else {
            segments.join("-")
        }
    }

    fn unique_preset_id(name: &str) -> String {
        format!("{}-{}", slugify(name), uuid::Uuid::new_v4().simple())
    }

    fn selected_listbox_index(hwnd: HWND) -> usize {
        let index = unsafe { SendMessageW(hwnd, LB_GETCURSEL, 0, 0) };
        if index == LB_ERR as isize || index < 0 {
            0
        } else {
            index as usize
        }
    }

    fn selected_combo_index(hwnd: HWND) -> usize {
        let index = unsafe { SendMessageW(hwnd, CB_GETCURSEL, 0, 0) };
        if index < 0 { 0 } else { index as usize }
    }

    fn select_listbox_index(hwnd: HWND, index: usize) {
        unsafe {
            SendMessageW(hwnd, LB_SETCURSEL, index, 0);
        }
    }

    fn select_combo_index(hwnd: HWND, index: usize) {
        unsafe {
            SendMessageW(hwnd, CB_SETCURSEL, index, 0);
        }
    }

    fn theme_index(theme: crate::model::preset::ThemeMode) -> usize {
        match theme {
            crate::model::preset::ThemeMode::System => 0,
            crate::model::preset::ThemeMode::Light => 1,
            crate::model::preset::ThemeMode::Dark => 2,
        }
    }

    fn theme_from_index(index: usize) -> crate::model::preset::ThemeMode {
        match index {
            1 => crate::model::preset::ThemeMode::Light,
            2 => crate::model::preset::ThemeMode::Dark,
            _ => crate::model::preset::ThemeMode::System,
        }
    }

    fn density_index(density: crate::model::preset::ApplicationDensity) -> usize {
        match density {
            crate::model::preset::ApplicationDensity::Comfortable => 0,
            crate::model::preset::ApplicationDensity::Standard => 1,
            crate::model::preset::ApplicationDensity::Compact => 2,
        }
    }

    fn density_from_index(index: usize) -> crate::model::preset::ApplicationDensity {
        match index {
            0 => crate::model::preset::ApplicationDensity::Comfortable,
            1 => crate::model::preset::ApplicationDensity::Standard,
            _ => crate::model::preset::ApplicationDensity::Compact,
        }
    }

    fn voice_activation_index(mode: VoiceActivationMode) -> usize {
        match mode {
            VoiceActivationMode::PushToTalk => 0,
            VoiceActivationMode::Toggle => 1,
        }
    }

    fn voice_activation_from_index(index: usize) -> VoiceActivationMode {
        match index {
            1 => VoiceActivationMode::Toggle,
            _ => VoiceActivationMode::PushToTalk,
        }
    }

    fn voice_engine_index(mode: VoiceEngineMode) -> usize {
        match mode {
            VoiceEngineMode::Auto => 0,
            VoiceEngineMode::Cuda => 1,
            VoiceEngineMode::Cpu => 2,
        }
    }

    fn voice_engine_from_index(index: usize) -> VoiceEngineMode {
        match index {
            1 => VoiceEngineMode::Cuda,
            2 => VoiceEngineMode::Cpu,
            _ => VoiceEngineMode::Auto,
        }
    }

    pub(crate) fn show_primary_shell_window() -> bool {
        let hwnd = PRIMARY_SHELL_HWND.load(Ordering::Relaxed) as HWND;
        if hwnd.is_null() {
            return false;
        }

        if let Some(state) = unsafe { state_mut(hwnd) } {
            restore_window_from_tray(hwnd, state);
            true
        } else {
            false
        }
    }
}

#[cfg(target_os = "windows")]
pub fn run() -> ExitCode {
    imp::run()
}

#[cfg(target_os = "windows")]
pub fn run_with_options(options: crate::extension::RuntimeOptions) -> ExitCode {
    imp::run_with_options(options)
}

#[cfg(target_os = "windows")]
pub(crate) fn show_primary_shell_window() -> bool {
    imp::show_primary_shell_window()
}

#[cfg(not(target_os = "windows"))]
pub fn run() -> ExitCode {
    ExitCode::FAILURE
}
