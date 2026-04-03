#[cfg(target_os = "windows")]
mod imp {
    use std::collections::HashMap;
    use std::mem;
    use std::ptr;
    use std::rc::Rc;

    use regex::Regex;
    use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
        EN_CHANGE, ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_LEFT, ES_MULTILINE, ES_READONLY,
        GWLP_USERDATA, GetClientRect, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
        HMENU, IDC_ARROW, LoadCursorW, RegisterClassW, SW_SHOW, SWP_NOZORDER, SendMessageW,
        SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
        WINDOW_EX_STYLE, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_NCCREATE, WM_NCDESTROY, WM_SETFONT,
        WM_SIZE, WNDCLASSW, WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
        WS_VSCROLL,
    };

    use crate::model::assets::Runbook;

    const WINDOW_CLASS: &str = "TerminalTilerWindowsRunbookDialog";
    const ID_INFO: isize = 1001;
    const ID_PREVIEW: isize = 1002;
    const ID_STATUS: isize = 1003;
    const ID_RUN: isize = 1004;
    const ID_CANCEL: isize = 1005;
    const ID_FIELD_BASE: isize = 2000;
    const MARGIN: i32 = 16;
    const BUTTON_HEIGHT: i32 = 32;
    const LABEL_HEIGHT: i32 = 18;
    const FIELD_HEIGHT: i32 = 26;

    struct VariableFieldState {
        id: String,
        label_hwnd: HWND,
        edit_hwnd: HWND,
        required: bool,
    }

    struct RunbookDialogState {
        parent_hwnd: HWND,
        runbook: Runbook,
        on_submit: Rc<dyn Fn(HashMap<String, String>)>,
        info_hwnd: HWND,
        preview_hwnd: HWND,
        status_hwnd: HWND,
        run_hwnd: HWND,
        cancel_hwnd: HWND,
        fields: Vec<VariableFieldState>,
    }

    pub fn present(
        parent_hwnd: HWND,
        runbook: Runbook,
        on_submit: Rc<dyn Fn(HashMap<String, String>)>,
    ) -> Result<(), String> {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err("could not resolve module handle for runbook dialog".into());
        }

        register_window_class(instance)?;
        let state = Box::new(RunbookDialogState {
            parent_hwnd,
            runbook,
            on_submit,
            info_hwnd: ptr::null_mut(),
            preview_hwnd: ptr::null_mut(),
            status_hwnd: ptr::null_mut(),
            run_hwnd: ptr::null_mut(),
            cancel_hwnd: ptr::null_mut(),
            fields: Vec::new(),
        });
        let state_ptr = Box::into_raw(state);

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                wide(WINDOW_CLASS).as_ptr(),
                wide("Runbook").as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                180,
                180,
                760,
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
            return Err("CreateWindowExW returned null for runbook dialog".into());
        }

        unsafe {
            EnableWindow(parent_hwnd, 0);
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
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
                let state_ptr = unsafe { (*create).lpCreateParams as *mut RunbookDialogState };
                unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize) };
                1
            }
            WM_CREATE => {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    create_controls(hwnd, state);
                    refresh_preview(state);
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
                    ID_RUN => {
                        if let Some((on_submit, values)) =
                            unsafe { state_mut(hwnd) }.and_then(|state| prepare_submit(state))
                        {
                            on_submit(values);
                            unsafe { DestroyWindow(hwnd) };
                        }
                    }
                    ID_CANCEL => unsafe {
                        DestroyWindow(hwnd);
                    },
                    id if id >= ID_FIELD_BASE && notification == EN_CHANGE => {
                        if let Some(state) = unsafe { state_mut(hwnd) } {
                            refresh_preview(state);
                        }
                    }
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
                    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) } as *mut RunbookDialogState;
                if !state_ptr.is_null() {
                    let state = unsafe { Box::from_raw(state_ptr) };
                    if !state.parent_hwnd.is_null() {
                        unsafe {
                            EnableWindow(state.parent_hwnd, 1);
                            SetForegroundWindow(state.parent_hwnd);
                            SetFocus(state.parent_hwnd);
                        }
                    }
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    fn create_controls(hwnd: HWND, state: &mut RunbookDialogState) {
        state.info_hwnd = create_child_window(hwnd, "STATIC", "", WS_CHILD | WS_VISIBLE, ID_INFO);
        state.preview_hwnd = create_child_window(
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
            ID_PREVIEW,
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
        state.cancel_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Cancel",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_CANCEL,
        );

        for (index, variable) in state.runbook.variables.iter().enumerate() {
            let label_hwnd = create_child_window(
                hwnd,
                "STATIC",
                &variable.label,
                WS_CHILD | WS_VISIBLE,
                ID_FIELD_BASE + (index as isize * 2),
            );
            let edit_hwnd = create_child_window(
                hwnd,
                "EDIT",
                variable.default_value.as_deref().unwrap_or(""),
                WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
                ID_FIELD_BASE + (index as isize * 2) + 1,
            );
            state.fields.push(VariableFieldState {
                id: variable.id.clone(),
                label_hwnd,
                edit_hwnd,
                required: variable.required,
            });
        }

        let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
        for control in std::iter::once(state.info_hwnd)
            .chain(std::iter::once(state.preview_hwnd))
            .chain(std::iter::once(state.status_hwnd))
            .chain(std::iter::once(state.run_hwnd))
            .chain(std::iter::once(state.cancel_hwnd))
            .chain(
                state
                    .fields
                    .iter()
                    .flat_map(|field| [field.label_hwnd, field.edit_hwnd]),
            )
        {
            unsafe {
                SendMessageW(control, WM_SETFONT, font as usize, 1);
            }
        }

        let info = if state.runbook.description.trim().is_empty() {
            format!(
                "Target: {}  •  Steps: {}  •  {}",
                state.runbook.target.label(),
                state.runbook.steps.len(),
                state.runbook.confirm_policy.label()
            )
        } else {
            format!(
                "{}\r\nTarget: {}  •  Steps: {}  •  {}",
                state.runbook.description,
                state.runbook.target.label(),
                state.runbook.steps.len(),
                state.runbook.confirm_policy.label()
            )
        };
        unsafe {
            SetWindowTextW(hwnd, wide(&format!("Run {}", state.runbook.name)).as_ptr());
            SetWindowTextW(state.info_hwnd, wide(&info).as_ptr());
        }
        layout_controls(hwnd, state);
    }

    fn layout_controls(hwnd: HWND, state: &RunbookDialogState) {
        let mut rect = unsafe { mem::zeroed() };
        unsafe { GetClientRect(hwnd, &mut rect) };
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let content_width = width - (MARGIN * 2);

        let mut y = MARGIN;
        unsafe {
            SetWindowPos(
                state.info_hwnd,
                ptr::null_mut(),
                MARGIN,
                y,
                content_width,
                52,
                SWP_NOZORDER,
            );
        }
        y += 64;

        for field in &state.fields {
            unsafe {
                SetWindowPos(
                    field.label_hwnd,
                    ptr::null_mut(),
                    MARGIN,
                    y,
                    content_width,
                    LABEL_HEIGHT,
                    SWP_NOZORDER,
                );
                SetWindowPos(
                    field.edit_hwnd,
                    ptr::null_mut(),
                    MARGIN,
                    y + LABEL_HEIGHT + 4,
                    content_width,
                    FIELD_HEIGHT,
                    SWP_NOZORDER,
                );
            }
            y += LABEL_HEIGHT + FIELD_HEIGHT + 14;
        }

        let button_y = height - MARGIN - BUTTON_HEIGHT;
        let status_y = button_y - 30;
        let preview_y = y + 4;
        let preview_height = (status_y - preview_y - 8).max(160);

        unsafe {
            SetWindowPos(
                state.preview_hwnd,
                ptr::null_mut(),
                MARGIN,
                preview_y,
                content_width,
                preview_height,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.status_hwnd,
                ptr::null_mut(),
                MARGIN,
                status_y,
                content_width - 200,
                22,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.run_hwnd,
                ptr::null_mut(),
                width - MARGIN - 188,
                button_y,
                88,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.cancel_hwnd,
                ptr::null_mut(),
                width - MARGIN - 92,
                button_y,
                88,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
        }
    }

    fn prepare_submit(
        state: &RunbookDialogState,
    ) -> Option<(Rc<dyn Fn(HashMap<String, String>)>, HashMap<String, String>)> {
        let values = current_values(state);
        for field in &state.fields {
            if field.required
                && values
                    .get(&field.id)
                    .map(|value| value.trim().is_empty())
                    .unwrap_or(true)
            {
                unsafe {
                    SetWindowTextW(
                        state.status_hwnd,
                        wide(&format!("'{}' is required.", field.id)).as_ptr(),
                    );
                    SetFocus(field.edit_hwnd);
                }
                return None;
            }
        }

        Some((state.on_submit.clone(), values))
    }

    fn refresh_preview(state: &RunbookDialogState) {
        let values = current_values(state);
        let rendered_steps = state
            .runbook
            .steps
            .iter()
            .map(|step| render_preview_command(&step.command, &values))
            .collect::<Vec<_>>()
            .join("\r\n");
        let preview = if rendered_steps.trim().is_empty() {
            "No runbook steps configured.".to_string()
        } else {
            format!("Preview:\r\n{rendered_steps}")
        };
        unsafe {
            SetWindowTextW(state.preview_hwnd, wide(&preview).as_ptr());
            SetWindowTextW(
                state.status_hwnd,
                wide("Fill any variables, then choose Run.").as_ptr(),
            );
        }
    }

    fn current_values(state: &RunbookDialogState) -> HashMap<String, String> {
        state
            .fields
            .iter()
            .map(|field| (field.id.clone(), read_window_text(field.edit_hwnd)))
            .collect()
    }

    fn render_preview_command(command: &str, values: &HashMap<String, String>) -> String {
        let Ok(variable_pattern) = Regex::new(r"\{\{\s*([a-zA-Z0-9_-]+)\s*\}\}") else {
            return command.to_string();
        };
        let mut rendered = String::new();
        let mut last_end = 0;
        for captures in variable_pattern.captures_iter(command) {
            let Some(variable_match) = captures.get(0) else {
                continue;
            };
            let Some(key_match) = captures.get(1) else {
                continue;
            };
            rendered.push_str(&command[last_end..variable_match.start()]);
            let key = key_match.as_str();
            if let Some(value) = values.get(key) {
                rendered.push_str(value);
            } else {
                rendered.push_str(variable_match.as_str());
            }
            last_end = variable_match.end();
        }
        rendered.push_str(&command[last_end..]);
        rendered
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
                return Err(format!("RegisterClassW failed for runbook dialog: {error}"));
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

    unsafe fn state_mut(hwnd: HWND) -> Option<&'static mut RunbookDialogState> {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut RunbookDialogState;
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
pub use imp::present;
