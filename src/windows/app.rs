use std::process::ExitCode;

#[cfg(target_os = "windows")]
mod imp {
    use super::ExitCode;
    use std::mem;
    use std::path::PathBuf;
    use std::ptr;
    use std::sync::atomic::{AtomicIsize, Ordering};

    use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        COLOR_WINDOW, DEFAULT_GUI_FONT, GetStockObject, UpdateWindow,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
        Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_PUSHBUTTON, CREATESTRUCTW,
        CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
        DestroyMenu, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_LEFT,
        ES_MULTILINE, ES_READONLY, GWLP_USERDATA, GetClientRect, GetCursorPos, GetDlgItem,
        GetMessageW, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW,
        IDI_APPLICATION, IDOK, LB_ADDSTRING, LB_ERR, LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL,
        LBN_SELCHANGE, LBS_NOTIFY, LoadCursorW, LoadIconW, MB_ICONWARNING, MB_OKCANCEL, MF_STRING,
        MSG, MessageBoxW, PostQuitMessage, RegisterClassW, SW_HIDE, SW_SHOW, SWP_NOZORDER,
        SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowTextW,
        ShowWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage,
        WINDOW_EX_STYLE, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_LBUTTONUP, WM_NCCREATE,
        WM_NCDESTROY, WM_RBUTTONUP, WM_SETFONT, WM_SIZE, WNDCLASSW, WS_BORDER, WS_CHILD,
        WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
    };

    use crate::logging;
    use crate::model::preset::{WorkspacePreset, is_builtin_preset_id};
    use crate::platform::{home_dir, resolve_workspace_root};
    use crate::storage::preference_store::{AppPreferences, PreferenceStore};
    use crate::storage::preset_store::PresetStore;
    use crate::storage::session_store::{SavedSession, SessionStore};
    use crate::windows::workspace;
    use crate::windows::wsl::{self, WindowsRuntime};

    const WINDOW_CLASS: &str = "TerminalTilerWindowsShell";
    const SETTINGS_WINDOW_CLASS: &str = "TerminalTilerWindowsSettings";
    const WINDOW_TITLE: &str = "TerminalTiler for Windows";
    const SETTINGS_WINDOW_TITLE: &str = "TerminalTiler Settings";
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
    const ID_SETTINGS_THEME_LIST: isize = 2001;
    const ID_SETTINGS_DENSITY_LIST: isize = 2002;
    const ID_SETTINGS_CLOSE_BACKGROUND: isize = 2003;
    const ID_SETTINGS_WSL_DISTRO: isize = 2004;
    const ID_SETTINGS_RUNTIME_STATUS: isize = 2005;
    const ID_SETTINGS_SAVE: isize = 2006;
    const ID_SETTINGS_RESET: isize = 2007;
    const ID_SETTINGS_CLOSE: isize = 2008;
    const ID_SETTINGS_PROBE: isize = 2009;
    const ID_SETTINGS_LABEL_THEME: isize = 2010;
    const ID_SETTINGS_LABEL_DENSITY: isize = 2011;
    const ID_SETTINGS_LABEL_DISTRO: isize = 2012;
    const ID_SETTINGS_LABEL_RUNTIME: isize = 2013;
    const BUTTON_HEIGHT: i32 = 32;
    const BUTTON_WIDTH: i32 = 160;
    const MARGIN: i32 = 16;
    const FIELD_HEIGHT: i32 = 28;
    const LABEL_HEIGHT: i32 = 18;
    const LIST_HEIGHT: i32 = 150;
    const SETTINGS_LIST_HEIGHT: i32 = 64;
    const CHECKBOX_UNCHECKED: usize = 0;
    const CHECKBOX_CHECKED: usize = 1;
    const WM_TRAYICON: u32 = 0x8001;
    const TRAY_ICON_ID: u32 = 1;
    const TRAY_MENU_SHOW: usize = 1;
    const TRAY_MENU_SETTINGS: usize = 2;
    const TRAY_MENU_QUIT: usize = 3;
    static PRIMARY_SHELL_HWND: AtomicIsize = AtomicIsize::new(0);

    pub fn run() -> ExitCode {
        logging::init();
        logging::info("windows GUI shell startup");

        match unsafe { run_gui() } {
            Ok(code) => code,
            Err(error) => {
                logging::error(format!("windows GUI shell failed: {error}"));
                eprintln!("TerminalTiler Windows shell failed: {error}");
                ExitCode::FAILURE
            }
        }
    }

    struct AppWindowState {
        preference_store: PreferenceStore,
        preset_store: PresetStore,
        session_store: SessionStore,
        runtime: Option<WindowsRuntime>,
        runtime_error: Option<String>,
        presets: Vec<WorkspacePreset>,
        preset_warning: Option<String>,
        session: Option<SavedSession>,
        session_warning: Option<String>,
        workspace_path_hwnd: HWND,
        session_name_hwnd: HWND,
        preset_list_hwnd: HWND,
        status_hwnd: HWND,
        settings_window_hwnd: HWND,
        tray_icon_added: bool,
        window_hidden_to_tray: bool,
        quit_requested: bool,
        save_preset_button_hwnd: HWND,
        update_preset_button_hwnd: HWND,
        delete_preset_button_hwnd: HWND,
        launch_preset_button_hwnd: HWND,
        launch_button_hwnd: HWND,
    }

    struct SettingsWindowState {
        parent_hwnd: HWND,
        preference_store: PreferenceStore,
        theme_list_hwnd: HWND,
        density_list_hwnd: HWND,
        close_background_hwnd: HWND,
        distro_hwnd: HWND,
        runtime_status_hwnd: HWND,
    }

    unsafe fn run_gui() -> Result<ExitCode, String> {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err("could not resolve module handle".into());
        }

        register_window_classes(instance)?;

        let state = Box::new(AppWindowState {
            preference_store: PreferenceStore::new(),
            preset_store: PresetStore::new(),
            session_store: SessionStore::new(),
            runtime: None,
            runtime_error: None,
            presets: Vec::new(),
            preset_warning: None,
            session: None,
            session_warning: None,
            workspace_path_hwnd: ptr::null_mut(),
            session_name_hwnd: ptr::null_mut(),
            preset_list_hwnd: ptr::null_mut(),
            status_hwnd: ptr::null_mut(),
            settings_window_hwnd: ptr::null_mut(),
            tray_icon_added: false,
            window_hidden_to_tray: false,
            quit_requested: false,
            save_preset_button_hwnd: ptr::null_mut(),
            update_preset_button_hwnd: ptr::null_mut(),
            delete_preset_button_hwnd: ptr::null_mut(),
            launch_preset_button_hwnd: ptr::null_mut(),
            launch_button_hwnd: ptr::null_mut(),
        });
        let state_ptr = Box::into_raw(state);

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                wide(WINDOW_CLASS).as_ptr(),
                wide(WINDOW_TITLE).as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                760,
                520,
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

    fn register_window_classes(instance: HINSTANCE) -> Result<(), String> {
        register_window_class(instance, WINDOW_CLASS, window_proc)?;
        register_window_class(instance, SETTINGS_WINDOW_CLASS, settings_window_proc)
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
            hbrBackground: (COLOR_WINDOW as isize + 1) as _,
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
                    create_controls(hwnd, state);
                    install_tray_icon(hwnd, state);
                    refresh_state(hwnd, state);
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
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    layout_controls(hwnd, state);
                }
                0
            }
            WM_TRAYICON => {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    handle_tray_event(hwnd, state, lparam as u32);
                }
                0
            }
            WM_COMMAND => {
                let command_id = (wparam & 0xffff) as isize;
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    match command_id {
                        ID_PRESET_LIST if ((wparam >> 16) & 0xffff) as u32 == LBN_SELCHANGE => {
                            sync_launch_name_to_selection(state);
                            update_preset_action_buttons(state);
                            sync_status_text(state);
                        }
                        ID_REFRESH => refresh_state(hwnd, state),
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
            WM_COMMAND => {
                let command_id = (wparam & 0xffff) as isize;
                if let Some(state) = unsafe { settings_state_mut(hwnd) } {
                    match command_id {
                        ID_SETTINGS_SAVE => save_settings(hwnd, state),
                        ID_SETTINGS_RESET => reset_settings(hwnd, state),
                        ID_SETTINGS_CLOSE => unsafe {
                            DestroyWindow(hwnd);
                        },
                        ID_SETTINGS_PROBE => refresh_settings_runtime_preview(state),
                        _ => {}
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
            "Launch Selected Preset",
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
            unsafe { GetDlgItem(hwnd, ID_LABEL_PRESETS as i32) },
            state.preset_list_hwnd,
            state.status_hwnd,
            state.save_preset_button_hwnd,
            state.update_preset_button_hwnd,
            state.delete_preset_button_hwnd,
            state.launch_preset_button_hwnd,
            state.launch_button_hwnd,
            unsafe { GetDlgItem(hwnd, ID_REFRESH as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS as i32) },
            unsafe { GetDlgItem(hwnd, ID_QUIT as i32) },
        ] {
            if !control.is_null() {
                unsafe {
                    SendMessageW(control, WM_SETFONT, font as usize, 1);
                }
            }
        }

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
        let presets_label_y = name_edit_y + FIELD_HEIGHT + 12;
        let preset_list_y = presets_label_y + LABEL_HEIGHT + 4;
        let preset_actions_y = preset_list_y + LIST_HEIGHT + 12;
        let button_y = height - MARGIN - BUTTON_HEIGHT;
        let status_y = preset_actions_y + BUTTON_HEIGHT + 12;
        let status_height = (button_y - status_y - 12).max(120);

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
                GetDlgItem(hwnd, ID_LABEL_PRESETS as i32),
                ptr::null_mut(),
                MARGIN,
                presets_label_y,
                content_width,
                LABEL_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.preset_list_hwnd,
                ptr::null_mut(),
                MARGIN,
                preset_list_y,
                content_width,
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
                GetDlgItem(hwnd, ID_REFRESH as i32),
                ptr::null_mut(),
                MARGIN,
                button_y,
                BUTTON_WIDTH,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.launch_preset_button_hwnd,
                ptr::null_mut(),
                MARGIN + BUTTON_WIDTH + 12,
                button_y,
                BUTTON_WIDTH + 20,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.launch_button_hwnd,
                ptr::null_mut(),
                MARGIN + (BUTTON_WIDTH * 2) + 44,
                button_y,
                BUTTON_WIDTH + 30,
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

    fn refresh_state(hwnd: HWND, state: &mut AppWindowState) {
        let preferences = state.preference_store.load();
        let preferred_distribution = preferences.windows_wsl_distribution.clone();
        state.runtime = None;
        state.runtime_error = None;

        match wsl::probe_runtime(preferred_distribution.as_deref()) {
            Ok(runtime) => state.runtime = Some(runtime),
            Err(error) => state.runtime_error = Some(error),
        }

        state.preset_store.ensure_seeded();
        let preset_outcome = state.preset_store.load_presets_with_status();
        state.presets = preset_outcome.presets;
        state.preset_warning = preset_outcome.warning;
        populate_preset_list(state);
        sync_launch_name_to_selection(state);
        update_preset_action_buttons(state);

        let session_outcome = state.session_store.load_with_status();
        state.session = session_outcome.session;
        state.session_warning = session_outcome.warning;

        unsafe {
            sync_status_text(state);
            EnableWindow(
                state.launch_preset_button_hwnd,
                (state.runtime.is_some() && !state.presets.is_empty()) as i32,
            );
            EnableWindow(
                state.launch_button_hwnd,
                (state.runtime.is_some() && state.session.is_some()) as i32,
            );
        }
        sync_tray_tooltip(hwnd, state);

        logging::info("refreshed Windows shell state");
    }

    fn launch_selected_preset(_hwnd: HWND, state: &mut AppWindowState) {
        let Some(runtime) = state.runtime.as_ref() else {
            return;
        };
        let Some(preset) = selected_preset(state).cloned() else {
            return;
        };

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
            }],
            active_tab_index: 0,
        };

        match wsl::collect_session_launch_commands(&session, runtime) {
            Ok(_) => match workspace::open_saved_workspaces(&session, runtime) {
                Ok((window_count, pane_count)) => {
                    let status = format!(
                        "Opened {} new workspace window(s) with {} pane(s) from preset '{}' using {}.",
                        window_count,
                        pane_count,
                        preset.name,
                        runtime.label()
                    );
                    unsafe {
                        SetWindowTextW(state.status_hwnd, wide(&status).as_ptr());
                    }
                    logging::info(format!(
                        "opened {} new workspace window(s) with {} pane(s) from preset '{}' using {}",
                        window_count,
                        pane_count,
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

    fn launch_restored_session(_hwnd: HWND, state: &mut AppWindowState) {
        let Some(runtime) = state.runtime.as_ref() else {
            return;
        };
        let Some(session) = state.session.as_ref() else {
            return;
        };

        match wsl::collect_session_launch_commands(session, runtime) {
            Ok(_) => match workspace::open_saved_workspaces(session, runtime) {
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
                        "opened {} Windows workspace host window(s) with {} pane(s) using {}",
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

    fn build_status_text(state: &AppWindowState, preferred_distribution: Option<&str>) -> String {
        let mut lines = Vec::new();
        lines.push("TerminalTiler Windows shell".to_string());
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
        lines.push(format!("Available presets: {}", state.presets.len()));
        if let Some(preset) = selected_preset(state) {
            lines.push(format!(
                "Selected preset: {} ({} tiles)",
                preset.name,
                preset.layout.tile_specs().len()
            ));
        }
        if let Some(warning) = state.preset_warning.as_deref() {
            lines.push(format!("Preset warning: {}", warning));
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

        lines.push(String::new());
        lines.push("Actions:".into());
        lines.push(
            "- Refresh Runtime reloads WSL/PowerShell availability and saved session state.".into(),
        );
        lines.push(
            "- Launch Selected Preset opens a new native workspace window from the chosen preset and workspace root."
                .into(),
        );
        lines.push(
            "- Save as Preset stores a copy of the selected preset, using the Launch name field as the preset name when provided."
                .into(),
        );
        lines.push(
            "- Update Preset rewrites the selected custom preset, while builtin presets are copied instead of modified in place."
                .into(),
        );
        lines.push(
            "- Open Restored Workspaces opens the restored session inside one native workspace host window with Windows-managed tabs."
                .into(),
        );

        lines.join("\r\n")
    }

    fn install_tray_icon(hwnd: HWND, state: &mut AppWindowState) {
        let mut notify = tray_icon_data(hwnd);
        fill_wide_buffer(&mut notify.szTip, "TerminalTiler");
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
            "TerminalTiler (hidden to background)"
        } else {
            "TerminalTiler"
        };
        fill_wide_buffer(&mut notify.szTip, tooltip);
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
        let notify = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            uFlags: NIF_MESSAGE | NIF_TIP | NIF_ICON,
            uCallbackMessage: WM_TRAYICON,
            ..unsafe { mem::zeroed() }
        };
        notify
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
            parent_hwnd,
            preference_store: state.preference_store.clone(),
            theme_list_hwnd: ptr::null_mut(),
            density_list_hwnd: ptr::null_mut(),
            close_background_hwnd: ptr::null_mut(),
            distro_hwnd: ptr::null_mut(),
            runtime_status_hwnd: ptr::null_mut(),
        });
        let settings_state_ptr = Box::into_raw(settings_state);

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                wide(SETTINGS_WINDOW_CLASS).as_ptr(),
                wide(SETTINGS_WINDOW_TITLE).as_ptr(),
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
            "Runtime preview",
            WS_CHILD | WS_VISIBLE,
            0,
            ID_SETTINGS_LABEL_RUNTIME,
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
            "Save",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_SAVE,
        );
        let _ = create_child_window(
            hwnd,
            "BUTTON",
            "Reset",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_SETTINGS_RESET,
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
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_THEME as i32) },
            state.theme_list_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_DENSITY as i32) },
            state.density_list_hwnd,
            state.close_background_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_DISTRO as i32) },
            state.distro_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_LABEL_RUNTIME as i32) },
            state.runtime_status_hwnd,
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_PROBE as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_SAVE as i32) },
            unsafe { GetDlgItem(hwnd, ID_SETTINGS_RESET as i32) },
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

        let theme_label_y = MARGIN;
        let theme_list_y = theme_label_y + LABEL_HEIGHT + 4;
        let density_label_y = theme_list_y + SETTINGS_LIST_HEIGHT + 12;
        let density_list_y = density_label_y + LABEL_HEIGHT + 4;
        let checkbox_y = density_list_y + SETTINGS_LIST_HEIGHT + 12;
        let distro_label_y = checkbox_y + 28 + 12;
        let distro_edit_y = distro_label_y + LABEL_HEIGHT + 4;
        let runtime_label_y = distro_edit_y + FIELD_HEIGHT + 12;
        let runtime_edit_y = runtime_label_y + LABEL_HEIGHT + 4;
        let button_y = height - MARGIN - BUTTON_HEIGHT;
        let runtime_height = (button_y - runtime_edit_y - 12).max(120);

        unsafe {
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
                GetDlgItem(hwnd, ID_SETTINGS_LABEL_RUNTIME as i32),
                ptr::null_mut(),
                MARGIN,
                runtime_label_y,
                content_width,
                LABEL_HEIGHT,
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
                GetDlgItem(hwnd, ID_SETTINGS_SAVE as i32),
                ptr::null_mut(),
                MARGIN + BUTTON_WIDTH + 12,
                button_y,
                96,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                GetDlgItem(hwnd, ID_SETTINGS_RESET as i32),
                ptr::null_mut(),
                MARGIN + BUTTON_WIDTH + 120,
                button_y,
                96,
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
        unsafe {
            SetWindowTextW(state.runtime_status_hwnd, wide(&runtime_text).as_ptr());
        }
    }

    fn save_settings(hwnd: HWND, state: &SettingsWindowState) {
        let theme = theme_from_index(selected_listbox_index(state.theme_list_hwnd));
        let density = density_from_index(selected_listbox_index(state.density_list_hwnd));
        let close_to_background =
            unsafe { SendMessageW(state.close_background_hwnd, BM_GETCHECK, 0, 0) }
                == CHECKBOX_CHECKED as isize;
        let preferred_distribution = read_window_text(state.distro_hwnd);

        state.preference_store.save_default_theme(theme);
        state.preference_store.save_default_density(density);
        state
            .preference_store
            .save_close_to_background(close_to_background);
        state
            .preference_store
            .save_windows_wsl_distribution(Some(preferred_distribution.as_str()));
        refresh_settings_runtime_preview(state);

        if let Some(parent_state) = unsafe { state_mut(state.parent_hwnd) } {
            refresh_state(state.parent_hwnd, parent_state);
        }

        let mut rect = unsafe { mem::zeroed() };
        unsafe {
            GetClientRect(hwnd, &mut rect);
        }
        state
            .preference_store
            .save_settings_dialog_size(rect.right - rect.left, rect.bottom - rect.top);
    }

    fn reset_settings(_hwnd: HWND, state: &SettingsWindowState) {
        let defaults = AppPreferences::default();
        apply_preferences_to_settings_controls(state, &defaults);
        state.preference_store.save(&defaults);
        refresh_settings_runtime_preview(state);

        if let Some(parent_state) = unsafe { state_mut(state.parent_hwnd) } {
            refresh_state(state.parent_hwnd, parent_state);
        }
    }

    fn apply_preferences_to_settings_controls(
        state: &SettingsWindowState,
        preferences: &AppPreferences,
    ) {
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
        }
    }

    fn create_child_window(
        hwnd: HWND,
        class_name: &str,
        text: &str,
        style: u32,
        ex_style: WINDOW_EX_STYLE,
        control_id: isize,
    ) -> HWND {
        unsafe {
            CreateWindowExW(
                ex_style,
                wide(class_name).as_ptr(),
                wide(text).as_ptr(),
                style,
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

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
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

    fn sync_status_text(state: &AppWindowState) {
        let preferences = state.preference_store.load();
        let status_text = build_status_text(state, preferences.windows_wsl_distribution.as_deref());
        unsafe {
            SetWindowTextW(state.status_hwnd, wide(&status_text).as_ptr());
        }
    }

    fn sync_launch_name_to_selection(state: &AppWindowState) {
        if let Some(preset) = selected_preset(state) {
            unsafe {
                SetWindowTextW(state.session_name_hwnd, wide(&preset.name).as_ptr());
            }
        }
    }

    fn update_preset_action_buttons(state: &AppWindowState) {
        let has_selection = selected_preset(state).is_some();
        let selected_is_builtin = selected_preset(state)
            .map(|preset| is_builtin_preset_id(&preset.id))
            .unwrap_or(false);

        unsafe {
            EnableWindow(state.save_preset_button_hwnd, has_selection as i32);
            EnableWindow(state.update_preset_button_hwnd, has_selection as i32);
            EnableWindow(
                state.delete_preset_button_hwnd,
                (has_selection && !selected_is_builtin) as i32,
            );
            SetWindowTextW(
                state.update_preset_button_hwnd,
                wide(if selected_is_builtin {
                    "Save Copy"
                } else {
                    "Update Preset"
                })
                .as_ptr(),
            );
        }
    }

    fn selected_preset(state: &AppWindowState) -> Option<&WorkspacePreset> {
        let index = unsafe { SendMessageW(state.preset_list_hwnd, LB_GETCURSEL, 0, 0) };
        if index == LB_ERR as isize || index < 0 {
            None
        } else {
            state.presets.get(index as usize)
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
                sync_launch_name_to_selection(state);
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
                sync_launch_name_to_selection(state);
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
                sync_launch_name_to_selection(state);
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
        let mut preset = selected_preset(state)?.clone();
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

    fn select_preset_by_id(state: &AppWindowState, preset_id: &str) {
        if let Some(index) = state
            .presets
            .iter()
            .position(|preset| preset.id == preset_id)
        {
            unsafe {
                SendMessageW(state.preset_list_hwnd, LB_SETCURSEL, index, 0);
            }
        }
    }

    fn unique_preset_lookup_name<'a>(presets: &'a [WorkspacePreset], name: &str) -> String {
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

    fn read_window_text(hwnd: HWND) -> String {
        let length = unsafe { GetWindowTextLengthW(hwnd) };
        if length <= 0 {
            return String::new();
        }

        let mut buffer = vec![0u16; length as usize + 1];
        let copied = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
        String::from_utf16_lossy(&buffer[..copied as usize])
    }

    fn selected_listbox_index(hwnd: HWND) -> usize {
        let index = unsafe { SendMessageW(hwnd, LB_GETCURSEL, 0, 0) };
        if index == LB_ERR as isize || index < 0 {
            0
        } else {
            index as usize
        }
    }

    fn select_listbox_index(hwnd: HWND, index: usize) {
        unsafe {
            SendMessageW(hwnd, LB_SETCURSEL, index, 0);
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
pub(crate) fn show_primary_shell_window() -> bool {
    imp::show_primary_shell_window()
}

#[cfg(not(target_os = "windows"))]
pub fn run() -> ExitCode {
    ExitCode::FAILURE
}
