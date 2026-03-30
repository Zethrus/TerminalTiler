use std::process::ExitCode;

#[cfg(target_os = "windows")]
mod imp {
    use super::ExitCode;
    use std::mem;
    use std::ptr;

    use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        COLOR_WINDOW, DEFAULT_GUI_FONT, GetStockObject, UpdateWindow,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BS_PUSHBUTTON, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW,
        DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOVSCROLL, ES_LEFT, ES_MULTILINE,
        ES_READONLY, GWLP_USERDATA, GetClientRect, GetDlgItem, GetMessageW, GetWindowLongPtrW,
        HMENU, IDC_ARROW, LoadCursorW, MSG, PostQuitMessage, RegisterClassW, SW_SHOW, SWP_NOZORDER,
        SendMessageW, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
        TranslateMessage, WINDOW_EX_STYLE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_NCCREATE,
        WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW, WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW,
        WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
    };

    use crate::logging;
    use crate::storage::preference_store::PreferenceStore;
    use crate::storage::session_store::{SavedSession, SessionStore};
    use crate::windows::workspace;
    use crate::windows::wsl::{self, WindowsRuntime};

    const WINDOW_CLASS: &str = "TerminalTilerWindowsShell";
    const WINDOW_TITLE: &str = "TerminalTiler for Windows";
    const ID_STATUS: isize = 1001;
    const ID_REFRESH: isize = 1002;
    const ID_LAUNCH: isize = 1003;
    const ID_QUIT: isize = 1004;
    const BUTTON_HEIGHT: i32 = 32;
    const BUTTON_WIDTH: i32 = 160;
    const MARGIN: i32 = 16;

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
        session_store: SessionStore,
        runtime: Option<WindowsRuntime>,
        runtime_error: Option<String>,
        session: Option<SavedSession>,
        session_warning: Option<String>,
        status_hwnd: HWND,
        launch_button_hwnd: HWND,
    }

    unsafe fn run_gui() -> Result<ExitCode, String> {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err("could not resolve module handle".into());
        }

        register_window_class(instance)?;

        let state = Box::new(AppWindowState {
            preference_store: PreferenceStore::new(),
            session_store: SessionStore::new(),
            runtime: None,
            runtime_error: None,
            session: None,
            session_warning: None,
            status_hwnd: ptr::null_mut(),
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

    fn register_window_class(instance: HINSTANCE) -> Result<(), String> {
        let class_name = wide(WINDOW_CLASS);
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
            return Err("RegisterClassW failed".into());
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
                    refresh_state(hwnd, state);
                }
                0
            }
            WM_SIZE => {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    layout_controls(hwnd, state);
                }
                0
            }
            WM_COMMAND => {
                let command_id = (wparam & 0xffff) as isize;
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    match command_id {
                        ID_REFRESH => refresh_state(hwnd, state),
                        ID_LAUNCH => launch_restored_session(hwnd, state),
                        ID_QUIT => unsafe {
                            DestroyWindow(hwnd);
                        },
                        _ => {}
                    }
                }
                0
            }
            WM_DESTROY => {
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

    fn create_controls(hwnd: HWND, state: &mut AppWindowState) {
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
            "Quit",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            ID_QUIT,
        );

        let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
        for control in [
            state.status_hwnd,
            state.launch_button_hwnd,
            unsafe { GetDlgItem(hwnd, ID_REFRESH as i32) },
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
        let button_y = height - MARGIN - BUTTON_HEIGHT;
        let status_height = (button_y - (MARGIN * 2)).max(120);

        unsafe {
            SetWindowPos(
                state.status_hwnd,
                ptr::null_mut(),
                MARGIN,
                MARGIN,
                width - (MARGIN * 2),
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
                state.launch_button_hwnd,
                ptr::null_mut(),
                MARGIN + BUTTON_WIDTH + 12,
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

        let session_outcome = state.session_store.load_with_status();
        state.session = session_outcome.session;
        state.session_warning = session_outcome.warning;

        let status_text = build_status_text(state, preferred_distribution.as_deref());
        unsafe {
            SetWindowTextW(state.status_hwnd, wide(&status_text).as_ptr());
            EnableWindow(
                state.launch_button_hwnd,
                (state.runtime.is_some() && state.session.is_some()) as i32,
            );
        }

        logging::info("refreshed Windows shell state");
        let _ = hwnd;
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
        lines.push("Actions:".into());
        lines.push(
            "- Refresh Runtime reloads WSL/PowerShell availability and saved session state.".into(),
        );
        lines.push(
            "- Open Restored Workspaces opens one native workspace host window per restored tab."
                .into(),
        );

        lines.join("\r\n")
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

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(target_os = "windows")]
pub fn run() -> ExitCode {
    imp::run()
}

#[cfg(not(target_os = "windows"))]
pub fn run() -> ExitCode {
    ExitCode::FAILURE
}
