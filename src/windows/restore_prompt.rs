#[cfg(target_os = "windows")]
mod imp {
    use std::mem;
    use std::ptr;

    use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
        DispatchMessageW, ES_AUTOVSCROLL, ES_LEFT, ES_MULTILINE, ES_READONLY, GWLP_USERDATA,
        GetClientRect, GetMessageW, GetWindowLongPtrW, HMENU, IDC_ARROW, IsWindow, LoadCursorW,
        MSG, RegisterClassW, SW_SHOW, SWP_NOZORDER, SendMessageW, SetForegroundWindow,
        SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WM_CLOSE,
        WM_COMMAND, WM_CREATE, WM_NCCREATE, WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW,
        WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
    };

    use crate::services::session_restore::RestoreStartupAction;

    const WINDOW_CLASS: &str = "TerminalTilerWindowsRestorePrompt";
    const ID_BODY: isize = 1001;
    const ID_START_FRESH: isize = 1002;
    const ID_RESUME_SHELLS: isize = 1003;
    const ID_RESUME_RERUN: isize = 1004;
    const MARGIN: i32 = 16;
    const BUTTON_HEIGHT: i32 = 34;

    struct PromptWindowState {
        body: String,
        selected_action: *mut RestoreStartupAction,
        body_hwnd: HWND,
        start_fresh_hwnd: HWND,
        resume_shells_hwnd: HWND,
        resume_rerun_hwnd: HWND,
    }

    pub fn present(
        parent_hwnd: HWND,
        session_count: usize,
        warning: Option<&str>,
    ) -> Result<RestoreStartupAction, String> {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err("could not resolve module handle for restore prompt".into());
        }

        register_window_class(instance)?;
        let mut selected_action = RestoreStartupAction::StartFresh;
        let state = Box::new(PromptWindowState {
            body: prompt_body(session_count, warning),
            selected_action: &mut selected_action,
            body_hwnd: ptr::null_mut(),
            start_fresh_hwnd: ptr::null_mut(),
            resume_shells_hwnd: ptr::null_mut(),
            resume_rerun_hwnd: ptr::null_mut(),
        });
        let state_ptr = Box::into_raw(state);

        if !parent_hwnd.is_null() {
            unsafe {
                EnableWindow(parent_hwnd, 0);
            }
        }

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                wide(WINDOW_CLASS).as_ptr(),
                wide("Resume Previous Session?").as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                220,
                220,
                620,
                320,
                parent_hwnd,
                ptr::null_mut(),
                instance,
                state_ptr.cast(),
            )
        };

        if hwnd.is_null() {
            if !parent_hwnd.is_null() {
                unsafe {
                    EnableWindow(parent_hwnd, 1);
                    SetForegroundWindow(parent_hwnd);
                }
            }
            unsafe {
                drop(Box::from_raw(state_ptr));
            }
            return Err("CreateWindowExW returned null for restore prompt".into());
        }

        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }

        let mut message = unsafe { mem::zeroed::<MSG>() };
        loop {
            if unsafe { IsWindow(hwnd) } == 0 {
                break;
            }
            let result = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
            if result <= 0 {
                break;
            }
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        if !parent_hwnd.is_null() {
            unsafe {
                EnableWindow(parent_hwnd, 1);
                SetForegroundWindow(parent_hwnd);
            }
        }

        Ok(selected_action)
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
                let state_ptr = unsafe { (*create).lpCreateParams as *mut PromptWindowState };
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
                }
                1
            }
            WM_CREATE => {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    create_controls(hwnd, state);
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
                    let action = match command_id {
                        ID_START_FRESH => Some(RestoreStartupAction::StartFresh),
                        ID_RESUME_SHELLS => Some(RestoreStartupAction::ResumeAsShells),
                        ID_RESUME_RERUN => Some(RestoreStartupAction::ResumeAndRerun),
                        _ => None,
                    };
                    if let Some(action) = action {
                        unsafe {
                            *state.selected_action = action;
                            DestroyWindow(hwnd);
                        }
                    }
                }
                0
            }
            WM_CLOSE => {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    unsafe {
                        *state.selected_action = RestoreStartupAction::StartFresh;
                    }
                }
                unsafe {
                    DestroyWindow(hwnd);
                }
                0
            }
            WM_NCDESTROY => {
                let state_ptr =
                    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) } as *mut PromptWindowState;
                if !state_ptr.is_null() {
                    drop(unsafe { Box::from_raw(state_ptr) });
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    fn create_controls(hwnd: HWND, state: &mut PromptWindowState) {
        state.body_hwnd = create_child_window(
            hwnd,
            "EDIT",
            &state.body,
            WS_CHILD
                | WS_VISIBLE
                | WS_BORDER
                | WS_VSCROLL
                | ES_LEFT as u32
                | ES_MULTILINE as u32
                | ES_AUTOVSCROLL as u32
                | ES_READONLY as u32,
            ID_BODY,
        );
        state.start_fresh_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Start Fresh",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_START_FRESH,
        );
        state.resume_shells_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Resume As Shells",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_RESUME_SHELLS,
        );
        state.resume_rerun_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Resume And Rerun",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_RESUME_RERUN,
        );
        let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
        for control in [
            state.body_hwnd,
            state.start_fresh_hwnd,
            state.resume_shells_hwnd,
            state.resume_rerun_hwnd,
        ] {
            unsafe {
                SendMessageW(control, WM_SETFONT, font as usize, 1);
            }
        }
        layout_controls(hwnd, state);
    }

    fn layout_controls(hwnd: HWND, state: &PromptWindowState) {
        let mut rect = unsafe { mem::zeroed() };
        unsafe {
            GetClientRect(hwnd, &mut rect);
        }
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let button_y = height - MARGIN - BUTTON_HEIGHT;
        let button_width = ((width - (MARGIN * 2) - 16) / 3).max(132);
        unsafe {
            SetWindowPos(
                state.body_hwnd,
                ptr::null_mut(),
                MARGIN,
                MARGIN,
                width - (MARGIN * 2),
                button_y - (MARGIN * 2),
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.start_fresh_hwnd,
                ptr::null_mut(),
                MARGIN,
                button_y,
                button_width,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.resume_shells_hwnd,
                ptr::null_mut(),
                MARGIN + button_width + 8,
                button_y,
                button_width,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.resume_rerun_hwnd,
                ptr::null_mut(),
                width - MARGIN - button_width,
                button_y,
                button_width,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
        }
    }

    fn prompt_body(session_count: usize, warning: Option<&str>) -> String {
        if let Some(warning) = warning {
            format!(
                "TerminalTiler found {session_count} saved workspace(s). You can rerun commands, reopen the same layouts as plain shells, or start fresh.\r\n\r\n{warning}"
            )
        } else {
            format!(
                "TerminalTiler found {session_count} saved workspace(s). You can rerun commands, reopen the same layouts as plain shells, or start fresh."
            )
        }
    }

    fn register_window_class(instance: HINSTANCE) -> Result<(), String> {
        let class_name = wide(WINDOW_CLASS);
        let mut class = unsafe { mem::zeroed::<WNDCLASSW>() };
        class.style = CS_HREDRAW | CS_VREDRAW;
        class.lpfnWndProc = Some(window_proc);
        class.hInstance = instance;
        class.hCursor = unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) };
        class.lpszClassName = class_name.as_ptr();
        let atom = unsafe { RegisterClassW(&class) };
        if atom == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(1410) {
                return Err(format!("RegisterClassW failed for restore prompt: {error}"));
            }
        }
        Ok(())
    }

    fn create_child_window(
        parent: HWND,
        class_name: &str,
        text: &str,
        style: u32,
        control_id: isize,
    ) -> HWND {
        unsafe {
            CreateWindowExW(
                0 as WINDOW_EX_STYLE,
                wide(class_name).as_ptr(),
                wide(text).as_ptr(),
                style,
                0,
                0,
                0,
                0,
                parent,
                control_id as HMENU,
                GetModuleHandleW(ptr::null()),
                ptr::null_mut(),
            )
        }
    }

    unsafe fn state_mut(hwnd: HWND) -> Option<&'static mut PromptWindowState> {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut PromptWindowState;
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
pub use imp::present;
