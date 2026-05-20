#[cfg(target_os = "windows")]
mod imp {
    use std::mem;
    use std::ptr;
    use std::rc::Rc;

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, EN_CHANGE, ES_AUTOHSCROLL,
        GWLP_USERDATA, GetClientRect, GetWindowLongPtrW, LB_ADDSTRING, LB_GETCURSEL,
        LB_RESETCONTENT, LB_SETCURSEL, LBN_DBLCLK, SW_SHOW, SWP_NOZORDER, SendMessageW,
        SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, WM_CLOSE, WM_COMMAND,
        WM_CREATE, WM_NCCREATE, WM_NCDESTROY, WM_SETFONT, WM_SIZE, WS_BORDER, WS_CHILD,
        WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
    };

    use crate::windows::win32_helpers::{
        create_child_window, read_window_text, register_window_class, wide,
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

        register_window_class(instance, WINDOW_CLASS, Some(window_proc), "command palette")?;
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
                match command_id {
                    ID_SEARCH if notification == EN_CHANGE => {
                        if let Some(state) = unsafe { state_mut(hwnd) } {
                            refresh_palette(state);
                        }
                    }
                    ID_LIST if notification == LBN_DBLCLK => {
                        if let Some(action) =
                            unsafe { state_mut(hwnd) }.and_then(|state| selected_action(state))
                        {
                            action();
                            unsafe { DestroyWindow(hwnd) };
                        }
                    }
                    ID_RUN => {
                        if let Some(action) =
                            unsafe { state_mut(hwnd) }.and_then(|state| selected_action(state))
                        {
                            action();
                            unsafe { DestroyWindow(hwnd) };
                        }
                    }
                    ID_CLOSE => unsafe {
                        DestroyWindow(hwnd);
                    },
                    _ => {}
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

    fn selected_action(state: &PaletteWindowState) -> Option<Rc<dyn Fn()>> {
        let selected = unsafe { SendMessageW(state.list_hwnd, LB_GETCURSEL, 0, 0) };
        if selected < 0 {
            return None;
        }
        let action_index = state.filtered_indexes.get(selected as usize).copied()?;
        Some(state.actions[action_index].on_activate.clone())
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

    unsafe fn state_mut(hwnd: HWND) -> Option<&'static mut PaletteWindowState> {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut PaletteWindowState;
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *ptr })
        }
    }
}

#[cfg(target_os = "windows")]
pub use imp::{PaletteAction, present};
