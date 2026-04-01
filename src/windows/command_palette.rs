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
        EN_CHANGE, ES_AUTOHSCROLL, GWLP_USERDATA, GetClientRect, GetWindowLongPtrW,
        GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, LB_ADDSTRING, LB_GETCURSEL,
        LB_RESETCONTENT, LB_SETCURSEL, LBN_DBLCLK, LoadCursorW, RegisterClassW, SW_SHOW,
        SWP_NOZORDER, SendMessageW, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
        WINDOW_EX_STYLE, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_NCCREATE, WM_NCDESTROY, WM_SETFONT,
        WM_SIZE, WNDCLASSW, WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
        WS_VSCROLL,
    };

    const WINDOW_CLASS: &str = "TerminalTilerWindowsCommandPalette";
    const ID_SEARCH: isize = 1001;
    const ID_LIST: isize = 1002;
    const ID_RUN: isize = 1003;
    const ID_CLOSE: isize = 1004;
    const ID_STATUS: isize = 1005;
    const MARGIN: i32 = 16;
    const BUTTON_HEIGHT: i32 = 32;
    const FIELD_HEIGHT: i32 = 28;
    const LIST_HEIGHT_MIN: i32 = 240;

    #[derive(Clone)]
    pub struct PaletteAction {
        pub title: String,
        pub subtitle: String,
        pub on_activate: Rc<dyn Fn()>,
    }

    struct PaletteWindowState {
        actions: Vec<PaletteAction>,
        filtered_indexes: Vec<usize>,
        search_hwnd: HWND,
        list_hwnd: HWND,
        run_hwnd: HWND,
        close_hwnd: HWND,
        status_hwnd: HWND,
    }

    pub fn present(
        parent_hwnd: HWND,
        title: &str,
        actions: Vec<PaletteAction>,
    ) -> Result<(), String> {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err("could not resolve module handle for command palette".into());
        }

        register_window_class(instance)?;
        let state = Box::new(PaletteWindowState {
            actions,
            filtered_indexes: Vec::new(),
            search_hwnd: ptr::null_mut(),
            list_hwnd: ptr::null_mut(),
            run_hwnd: ptr::null_mut(),
            close_hwnd: ptr::null_mut(),
            status_hwnd: ptr::null_mut(),
        });
        let state_ptr = Box::into_raw(state);

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                wide(WINDOW_CLASS).as_ptr(),
                wide(title).as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                120,
                120,
                760,
                560,
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
            return Err("CreateWindowExW returned null for command palette".into());
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
                let state_ptr = unsafe { (*create).lpCreateParams as *mut PaletteWindowState };
                unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize) };
                1
            }
            WM_CREATE => {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    create_controls(hwnd, state);
                    refresh_palette(state);
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
                        ID_SEARCH if notification == EN_CHANGE => {
                            refresh_palette(state);
                        }
                        ID_LIST if notification == LBN_DBLCLK => {
                            activate_selected(hwnd, state);
                        }
                        ID_RUN => activate_selected(hwnd, state),
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
                let state_ptr =
                    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) } as *mut PaletteWindowState;
                if !state_ptr.is_null() {
                    drop(unsafe { Box::from_raw(state_ptr) });
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    fn create_controls(hwnd: HWND, state: &mut PaletteWindowState) {
        state.search_hwnd = create_child_window(
            hwnd,
            "EDIT",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
            ID_SEARCH,
        );
        state.list_hwnd = create_child_window(
            hwnd,
            "LISTBOX",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | WS_VSCROLL,
            ID_LIST,
        );
        state.status_hwnd =
            create_child_window(hwnd, "STATIC", "", WS_CHILD | WS_VISIBLE, ID_STATUS);
        state.run_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Run",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_RUN,
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
            state.search_hwnd,
            state.list_hwnd,
            state.status_hwnd,
            state.run_hwnd,
            state.close_hwnd,
        ] {
            unsafe { SendMessageW(control, WM_SETFONT, font as usize, 1) };
        }
        layout_controls(hwnd, state);
    }

    fn layout_controls(hwnd: HWND, state: &PaletteWindowState) {
        let mut rect = unsafe { mem::zeroed() };
        unsafe { GetClientRect(hwnd, &mut rect) };
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let content_width = width - (MARGIN * 2);
        let list_height =
            (height - (MARGIN * 2) - FIELD_HEIGHT - BUTTON_HEIGHT - 52).max(LIST_HEIGHT_MIN);
        let list_y = MARGIN + FIELD_HEIGHT + 12;
        let button_y = MARGIN + FIELD_HEIGHT + 12 + list_height + 12;
        unsafe {
            SetWindowPos(
                state.search_hwnd,
                ptr::null_mut(),
                MARGIN,
                MARGIN,
                content_width,
                FIELD_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.list_hwnd,
                ptr::null_mut(),
                MARGIN,
                list_y,
                content_width,
                list_height,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.status_hwnd,
                ptr::null_mut(),
                MARGIN,
                button_y,
                content_width - 200,
                20,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.run_hwnd,
                ptr::null_mut(),
                width - MARGIN - 188,
                button_y - 4,
                88,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.close_hwnd,
                ptr::null_mut(),
                width - MARGIN - 92,
                button_y - 4,
                88,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
        }
    }

    fn refresh_palette(state: &mut PaletteWindowState) {
        let query = read_window_text(state.search_hwnd)
            .trim()
            .to_ascii_lowercase();
        state.filtered_indexes.clear();
        unsafe {
            SendMessageW(state.list_hwnd, LB_RESETCONTENT, 0, 0);
        }
        for (index, action) in state.actions.iter().enumerate() {
            if !query.is_empty()
                && !action.title.to_ascii_lowercase().contains(&query)
                && !action.subtitle.to_ascii_lowercase().contains(&query)
            {
                continue;
            }
            state.filtered_indexes.push(index);
            let label = if action.subtitle.trim().is_empty() {
                action.title.clone()
            } else {
                format!("{}  -  {}", action.title, action.subtitle)
            };
            unsafe {
                SendMessageW(
                    state.list_hwnd,
                    LB_ADDSTRING,
                    0,
                    wide(&label).as_ptr() as LPARAM,
                );
            }
        }
        if !state.filtered_indexes.is_empty() {
            unsafe {
                SendMessageW(state.list_hwnd, LB_SETCURSEL, 0, 0);
            }
            set_selected_status(state);
        } else {
            unsafe { SetWindowTextW(state.status_hwnd, wide("No matching actions.").as_ptr()) };
        }
    }

    fn activate_selected(hwnd: HWND, state: &mut PaletteWindowState) {
        let selected = unsafe { SendMessageW(state.list_hwnd, LB_GETCURSEL, 0, 0) };
        if selected < 0 {
            return;
        }
        let Some(action_index) = state.filtered_indexes.get(selected as usize).copied() else {
            return;
        };
        let action = state.actions[action_index].clone();
        (action.on_activate)();
        unsafe { DestroyWindow(hwnd) };
    }

    fn set_selected_status(state: &PaletteWindowState) {
        let selected = unsafe { SendMessageW(state.list_hwnd, LB_GETCURSEL, 0, 0) };
        if selected < 0 {
            unsafe { SetWindowTextW(state.status_hwnd, wide("").as_ptr()) };
            return;
        }
        let Some(action_index) = state.filtered_indexes.get(selected as usize).copied() else {
            return;
        };
        unsafe {
            SetWindowTextW(
                state.status_hwnd,
                wide(&state.actions[action_index].subtitle).as_ptr(),
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
                    "RegisterClassW failed for command palette: {error}"
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

    unsafe fn state_mut(hwnd: HWND) -> Option<&'static mut PaletteWindowState> {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut PaletteWindowState;
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *ptr })
        }
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

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(target_os = "windows")]
pub use imp::{PaletteAction, present};
