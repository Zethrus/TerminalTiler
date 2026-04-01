#[cfg(target_os = "windows")]
mod imp {
    use std::mem;
    use std::ptr;

    use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
        ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_LEFT, ES_MULTILINE, ES_READONLY, GWLP_USERDATA,
        GetClientRect, GetWindowLongPtrW, HMENU, IDC_ARROW, LoadCursorW, RegisterClassW, SW_SHOW,
        SWP_NOZORDER, SendMessageW, SetWindowLongPtrW, SetWindowPos, ShowWindow, WINDOW_EX_STYLE,
        WM_CLOSE, WM_COMMAND, WM_CREATE, WM_NCCREATE, WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW,
        WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
    };

    const WINDOW_CLASS: &str = "TerminalTilerWindowsTranscriptViewer";
    const ID_TEXT: isize = 1001;
    const ID_CLOSE: isize = 1002;
    const MARGIN: i32 = 16;
    const BUTTON_HEIGHT: i32 = 32;

    struct TranscriptWindowState {
        text: String,
        text_hwnd: HWND,
        close_hwnd: HWND,
    }

    pub fn present(parent_hwnd: HWND, title: &str, transcript: &str) -> Result<(), String> {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err("could not resolve module handle for transcript viewer".into());
        }

        register_window_class(instance)?;
        let state = Box::new(TranscriptWindowState {
            text: if transcript.trim().is_empty() {
                "No transcript is available yet.".into()
            } else {
                transcript.into()
            },
            text_hwnd: ptr::null_mut(),
            close_hwnd: ptr::null_mut(),
        });
        let state_ptr = Box::into_raw(state);

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                wide(WINDOW_CLASS).as_ptr(),
                wide(title).as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                220,
                220,
                900,
                640,
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
            return Err("CreateWindowExW returned null for transcript viewer".into());
        }
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
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
                let state_ptr = unsafe { (*create).lpCreateParams as *mut TranscriptWindowState };
                unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize) };
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
                if command_id == ID_CLOSE {
                    unsafe { DestroyWindow(hwnd) };
                }
                0
            }
            WM_CLOSE => {
                unsafe { DestroyWindow(hwnd) };
                0
            }
            WM_NCDESTROY => {
                let state_ptr = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) }
                    as *mut TranscriptWindowState;
                if !state_ptr.is_null() {
                    drop(unsafe { Box::from_raw(state_ptr) });
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    fn create_controls(hwnd: HWND, state: &mut TranscriptWindowState) {
        state.text_hwnd = create_child_window(
            hwnd,
            "EDIT",
            &state.text,
            WS_CHILD
                | WS_VISIBLE
                | WS_BORDER
                | WS_VSCROLL
                | ES_LEFT as u32
                | ES_MULTILINE as u32
                | ES_AUTOVSCROLL as u32
                | ES_AUTOHSCROLL as u32
                | ES_READONLY as u32,
            ID_TEXT,
        );
        state.close_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Close",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_CLOSE,
        );
        let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
        for control in [state.text_hwnd, state.close_hwnd] {
            unsafe {
                SendMessageW(control, WM_SETFONT, font as usize, 1);
            }
        }
        layout_controls(hwnd, state);
    }

    fn layout_controls(hwnd: HWND, state: &TranscriptWindowState) {
        let mut rect = unsafe { mem::zeroed() };
        unsafe { GetClientRect(hwnd, &mut rect) };
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let button_y = height - MARGIN - BUTTON_HEIGHT;
        unsafe {
            SetWindowPos(
                state.text_hwnd,
                ptr::null_mut(),
                MARGIN,
                MARGIN,
                width - (MARGIN * 2),
                button_y - MARGIN - 8,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.close_hwnd,
                ptr::null_mut(),
                width - MARGIN - 92,
                button_y,
                92,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
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
                return Err(format!(
                    "RegisterClassW failed for transcript viewer: {error}"
                ));
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

    unsafe fn state_mut(hwnd: HWND) -> Option<&'static mut TranscriptWindowState> {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut TranscriptWindowState;
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
