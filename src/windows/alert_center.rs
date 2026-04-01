#[cfg(target_os = "windows")]
mod imp {
    use std::mem;
    use std::ptr;
    use std::rc::Rc;

    use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
        ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_LEFT, ES_MULTILINE, ES_READONLY, GWLP_USERDATA,
        GetClientRect, GetWindowLongPtrW, HMENU, IDC_ARROW, LB_ADDSTRING, LB_GETCURSEL,
        LB_RESETCONTENT, LB_SETCURSEL, LBN_SELCHANGE, LoadCursorW, RegisterClassW, SW_SHOW,
        SWP_NOZORDER, SendMessageW, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
        WINDOW_EX_STYLE, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_NCCREATE, WM_NCDESTROY, WM_SETFONT,
        WM_SIZE, WNDCLASSW, WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
        WS_VSCROLL,
    };

    const WINDOW_CLASS: &str = "TerminalTilerWindowsAlertCenter";
    const ID_LIST: isize = 1001;
    const ID_DETAIL: isize = 1002;
    const ID_JUMP: isize = 1003;
    const ID_RECONNECT: isize = 1004;
    const ID_MARK_READ: isize = 1005;
    const ID_MARK_ALL: isize = 1006;
    const ID_CLOSE: isize = 1007;
    const MARGIN: i32 = 16;
    const BUTTON_HEIGHT: i32 = 32;

    #[derive(Clone)]
    pub struct AlertCenterEntry {
        pub title: String,
        pub detail: String,
        pub unread: bool,
        pub allows_reconnect: bool,
        pub on_jump: Rc<dyn Fn()>,
        pub on_reconnect: Option<Rc<dyn Fn()>>,
        pub on_mark_read: Rc<dyn Fn()>,
    }

    struct AlertCenterWindowState {
        entries: Vec<AlertCenterEntry>,
        list_hwnd: HWND,
        detail_hwnd: HWND,
        jump_hwnd: HWND,
        reconnect_hwnd: HWND,
        mark_read_hwnd: HWND,
        mark_all_hwnd: HWND,
        close_hwnd: HWND,
        on_mark_all_read: Rc<dyn Fn()>,
    }

    pub fn present(
        parent_hwnd: HWND,
        entries: Vec<AlertCenterEntry>,
        on_mark_all_read: Rc<dyn Fn()>,
    ) -> Result<(), String> {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err("could not resolve module handle for alert center".into());
        }

        register_window_class(instance)?;
        let state = Box::new(AlertCenterWindowState {
            entries,
            list_hwnd: ptr::null_mut(),
            detail_hwnd: ptr::null_mut(),
            jump_hwnd: ptr::null_mut(),
            reconnect_hwnd: ptr::null_mut(),
            mark_read_hwnd: ptr::null_mut(),
            mark_all_hwnd: ptr::null_mut(),
            close_hwnd: ptr::null_mut(),
            on_mark_all_read,
        });
        let state_ptr = Box::into_raw(state);

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                wide(WINDOW_CLASS).as_ptr(),
                wide("Alert Center").as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                200,
                200,
                920,
                620,
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
            return Err("CreateWindowExW returned null for alert center".into());
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
                let state_ptr = unsafe { (*create).lpCreateParams as *mut AlertCenterWindowState };
                unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize) };
                1
            }
            WM_CREATE => {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    create_controls(hwnd, state);
                    refresh_list(state, None);
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
                let notification = ((wparam >> 16) & 0xffff) as u32;
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    match command_id {
                        ID_LIST if notification == LBN_SELCHANGE => update_selection_detail(state),
                        ID_JUMP => activate_jump(state),
                        ID_RECONNECT => activate_reconnect(state),
                        ID_MARK_READ => activate_mark_read(state),
                        ID_MARK_ALL => activate_mark_all(state),
                        ID_CLOSE => unsafe {
                            DestroyWindow(hwnd);
                        },
                        _ => {}
                    }
                }
                0
            }
            WM_CLOSE => {
                unsafe { DestroyWindow(hwnd) };
                0
            }
            WM_NCDESTROY => {
                let state_ptr = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) }
                    as *mut AlertCenterWindowState;
                if !state_ptr.is_null() {
                    drop(unsafe { Box::from_raw(state_ptr) });
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    fn create_controls(hwnd: HWND, state: &mut AlertCenterWindowState) {
        state.list_hwnd = create_child_window(
            hwnd,
            "LISTBOX",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | WS_VSCROLL,
            ID_LIST,
        );
        state.detail_hwnd = create_child_window(
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
                | ES_AUTOHSCROLL as u32
                | ES_READONLY as u32,
            ID_DETAIL,
        );
        state.jump_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Jump",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_JUMP,
        );
        state.reconnect_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Reconnect",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_RECONNECT,
        );
        state.mark_read_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Mark Read",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_MARK_READ,
        );
        state.mark_all_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Mark All Read",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_MARK_ALL,
        );
        state.close_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Close",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_CLOSE,
        );

        let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
        for control in [
            state.list_hwnd,
            state.detail_hwnd,
            state.jump_hwnd,
            state.reconnect_hwnd,
            state.mark_read_hwnd,
            state.mark_all_hwnd,
            state.close_hwnd,
        ] {
            unsafe {
                SendMessageW(control, WM_SETFONT, font as usize, 1);
            }
        }
        layout_controls(hwnd, state);
    }

    fn layout_controls(hwnd: HWND, state: &AlertCenterWindowState) {
        let mut rect = unsafe { mem::zeroed() };
        unsafe { GetClientRect(hwnd, &mut rect) };
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let list_width = ((width - (MARGIN * 3)) * 35 / 100).max(220);
        let detail_x = MARGIN + list_width + MARGIN;
        let detail_width = width - detail_x - MARGIN;
        let button_y = height - MARGIN - BUTTON_HEIGHT;
        let content_height = button_y - MARGIN - 8;
        unsafe {
            SetWindowPos(
                state.list_hwnd,
                ptr::null_mut(),
                MARGIN,
                MARGIN,
                list_width,
                content_height,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.detail_hwnd,
                ptr::null_mut(),
                detail_x,
                MARGIN,
                detail_width,
                content_height,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.jump_hwnd,
                ptr::null_mut(),
                MARGIN,
                button_y,
                92,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.reconnect_hwnd,
                ptr::null_mut(),
                MARGIN + 100,
                button_y,
                104,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.mark_read_hwnd,
                ptr::null_mut(),
                MARGIN + 212,
                button_y,
                104,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.mark_all_hwnd,
                ptr::null_mut(),
                MARGIN + 324,
                button_y,
                116,
                BUTTON_HEIGHT,
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

    fn refresh_list(state: &AlertCenterWindowState, preferred_selection: Option<usize>) {
        unsafe {
            SendMessageW(state.list_hwnd, LB_RESETCONTENT, 0, 0);
            for entry in &state.entries {
                let label = if entry.unread {
                    format!("[Unread] {}", entry.title)
                } else {
                    entry.title.clone()
                };
                SendMessageW(
                    state.list_hwnd,
                    LB_ADDSTRING,
                    0,
                    wide(&label).as_ptr() as LPARAM,
                );
            }
            if !state.entries.is_empty() {
                let selection = preferred_selection
                    .unwrap_or(0)
                    .min(state.entries.len().saturating_sub(1));
                SendMessageW(state.list_hwnd, LB_SETCURSEL, selection, 0);
            }
        }
        update_selection_detail(state);
    }

    fn update_selection_detail(state: &AlertCenterWindowState) {
        let index = selected_index(state);
        let detail = index
            .and_then(|index| state.entries.get(index))
            .map(|entry| {
                if entry.detail.trim().is_empty() {
                    entry.title.clone()
                } else {
                    format!("{}\r\n\r\n{}", entry.title, entry.detail)
                }
            })
            .unwrap_or_else(|| "No alert selected.".to_string());
        let can_reconnect = index
            .and_then(|index| state.entries.get(index))
            .map(|entry| entry.allows_reconnect)
            .unwrap_or(false);
        unsafe {
            SetWindowTextW(state.detail_hwnd, wide(&detail).as_ptr());
            windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                state.reconnect_hwnd,
                if can_reconnect { 1 } else { 0 },
            );
            windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                state.jump_hwnd,
                if index.is_some() { 1 } else { 0 },
            );
            windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                state.mark_read_hwnd,
                if index.is_some() { 1 } else { 0 },
            );
        }
    }

    fn activate_jump(state: &mut AlertCenterWindowState) {
        let Some(index) = selected_index(state) else {
            return;
        };
        let Some(entry) = state.entries.get(index).cloned() else {
            return;
        };
        (entry.on_jump)();
        (entry.on_mark_read)();
        if let Some(current) = state.entries.get_mut(index) {
            current.unread = false;
        }
        refresh_list(state, Some(index));
    }

    fn activate_reconnect(state: &mut AlertCenterWindowState) {
        let Some(index) = selected_index(state) else {
            return;
        };
        let Some(entry) = state.entries.get(index).cloned() else {
            return;
        };
        if let Some(callback) = entry.on_reconnect {
            callback();
            (entry.on_mark_read)();
            if let Some(current) = state.entries.get_mut(index) {
                current.unread = false;
            }
            refresh_list(state, Some(index));
        }
    }

    fn activate_mark_read(state: &mut AlertCenterWindowState) {
        let Some(index) = selected_index(state) else {
            return;
        };
        let Some(entry) = state.entries.get(index).cloned() else {
            return;
        };
        (entry.on_mark_read)();
        if let Some(current) = state.entries.get_mut(index) {
            current.unread = false;
        }
        refresh_list(state, Some(index));
    }

    fn activate_mark_all(state: &mut AlertCenterWindowState) {
        (state.on_mark_all_read)();
        for entry in &mut state.entries {
            entry.unread = false;
        }
        refresh_list(state, selected_index(state));
    }

    fn selected_index(state: &AlertCenterWindowState) -> Option<usize> {
        let selected = unsafe { SendMessageW(state.list_hwnd, LB_GETCURSEL, 0, 0) };
        if selected < 0 {
            None
        } else {
            Some(selected as usize)
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
                return Err(format!("RegisterClassW failed for alert center: {error}"));
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

    unsafe fn state_mut(hwnd: HWND) -> Option<&'static mut AlertCenterWindowState> {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut AlertCenterWindowState;
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
pub use imp::{AlertCenterEntry, present};
