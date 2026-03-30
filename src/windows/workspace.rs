#[cfg(target_os = "windows")]
mod imp {
    use std::ffi::c_void;
    use std::mem;
    use std::ptr;
    use std::thread;

    use windows_sys::Win32::Foundation::{
        COLORREF, CloseHandle, GlobalFree, HANDLE, HANDLE_FLAG_INHERIT, HINSTANCE, HWND, LPARAM,
        LRESULT, POINT, RECT, SIZE, SetHandleInformation, WPARAM,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_WINDOW, ClientToScreen,
        CreateFontW, CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_GUI_FONT, DeleteObject, EndPaint,
        FF_MODERN, FIXED_PITCH, FW_NORMAL, FillRect, GetDC, GetDeviceCaps, GetStockObject,
        GetTextExtentPoint32W, GetTextMetricsW, HBRUSH, HFONT, HGDIOBJ, InvalidateRect, LOGPIXELSY,
        OUT_DEFAULT_PRECIS, PAINTSTRUCT, ReleaseDC, ScreenToClient, SelectObject, SetBkColor,
        SetTextColor, TEXTMETRICW, TextOutW, UpdateWindow,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
    use windows_sys::Win32::System::Console::{
        COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
    };
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
        OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
        InitializeProcThreadAttributeList, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
        PROCESS_INFORMATION, STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute,
    };
    use windows_sys::Win32::UI::Controls::SetScrollInfo;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, SetFocus, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_HOME, VK_INSERT, VK_LEFT,
        VK_NEXT, VK_PRIOR, VK_RIGHT, VK_SHIFT, VK_UP,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CREATESTRUCTW, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, CreatePopupMenu,
        CreateWindowExW, DefWindowProcW, DestroyMenu, GWLP_USERDATA, GetClientRect,
        GetWindowLongPtrW, HMENU, IDC_ARROW, LoadCursorW, MF_GRAYED, MF_STRING, PostMessageW,
        RegisterClassW, SB_BOTTOM, SB_LINEDOWN, SB_LINEUP, SB_PAGEDOWN, SB_PAGEUP,
        SB_THUMBPOSITION, SB_THUMBTRACK, SB_TOP, SB_VERT, SCROLLINFO, SIF_PAGE, SIF_POS, SIF_RANGE,
        SW_SHOW, SWP_NOZORDER, SendMessageW, SetWindowLongPtrW, SetWindowPos, SetWindowTextW,
        ShowWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WINDOW_EX_STYLE, WM_APP,
        WM_CHAR, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDBLCLK,
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY,
        WM_PAINT, WM_RBUTTONUP, WM_SETFOCUS, WM_SETFONT, WM_SIZE, WM_VSCROLL, WNDCLASSW, WS_BORDER,
        WS_CHILD, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
    };

    use crate::logging;
    use crate::model::layout::{LayoutNode, SplitAxis, TileSpec};
    use crate::model::preset::ApplicationDensity;
    use crate::storage::session_store::{SavedSession, SavedTab};
    use crate::windows::vt::{MouseTrackingMode, VtBuffer, VtColor, VtPosition, VtStyle};
    use crate::windows::wsl::{self, WslLaunchCommand};

    const WINDOW_CLASS: &str = "TerminalTilerWindowsWorkspace";
    const PANE_CLASS: &str = "TerminalTilerWindowsPane";
    const WM_PANE_OUTPUT: u32 = WM_APP + 1;
    const WM_PANE_EXIT: u32 = WM_APP + 2;
    const HEADER_HEIGHT: i32 = 56;
    const OUTER_MARGIN: i32 = 12;
    const PANE_GAP: i32 = 8;
    const PANE_TITLE_HEIGHT: i32 = 20;
    const MIN_PANE_CHARS_X: i16 = 40;
    const MIN_PANE_CHARS_Y: i16 = 12;
    const DEFAULT_FONT_FACE: &str = "JetBrains Mono";
    const CF_UNICODETEXT: u32 = 13;
    const MIN_TERMINAL_FONT_POINTS: i32 = 7;
    const MAX_TERMINAL_FONT_POINTS: i32 = 20;
    const MENU_COPY_SELECTION: usize = 1;
    const MENU_PASTE_CLIPBOARD: usize = 2;

    struct WorkspaceWindowState {
        tab: SavedTab,
        distribution: String,
        title_hwnd: HWND,
        path_hwnd: HWND,
        panes: Vec<Box<PaneState>>,
    }

    struct PaneState {
        tile: TileSpec,
        title_hwnd: HWND,
        output_hwnd: HWND,
        terminal: VtBuffer,
        focused: bool,
        font: HFONT,
        cell_width: i32,
        cell_height: i32,
        baseline_offset: i32,
        line_height_scale: f64,
        selection_anchor: Option<VtPosition>,
        selection_focus: Option<VtPosition>,
        selecting: bool,
        pressed_mouse_button: Option<u8>,
        session: Option<PaneSession>,
    }

    impl Drop for PaneState {
        fn drop(&mut self) {
            if !self.font.is_null() {
                unsafe {
                    DeleteObject(self.font as HGDIOBJ);
                }
                self.font = ptr::null_mut();
            }
        }
    }

    struct PaneSession {
        pseudo_console: HPCON,
        process_handle: HANDLE,
        input_write: HANDLE,
    }

    struct PaneOutputEvent {
        pane_index: usize,
        text: String,
    }

    #[derive(Clone, Copy)]
    struct Bounds {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    impl Bounds {
        fn width(self) -> i32 {
            self.right - self.left
        }

        fn height(self) -> i32 {
            self.bottom - self.top
        }
    }

    impl PaneSession {
        fn spawn(
            window_hwnd: HWND,
            pane_index: usize,
            command: &WslLaunchCommand,
            columns: i16,
            rows: i16,
        ) -> Result<Self, String> {
            let mut input_read = ptr::null_mut();
            let mut input_write = ptr::null_mut();
            let mut output_read = ptr::null_mut();
            let mut output_write = ptr::null_mut();
            let security = SECURITY_ATTRIBUTES {
                nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: ptr::null_mut(),
                bInheritHandle: 1,
            };

            unsafe {
                if CreatePipe(&mut input_read, &mut input_write, &security, 0) == 0 {
                    return Err(format!(
                        "CreatePipe for pseudo console input failed: {}",
                        std::io::Error::last_os_error()
                    ));
                }
                if CreatePipe(&mut output_read, &mut output_write, &security, 0) == 0 {
                    CloseHandle(input_read);
                    CloseHandle(input_write);
                    return Err(format!(
                        "CreatePipe for pseudo console output failed: {}",
                        std::io::Error::last_os_error()
                    ));
                }
                if SetHandleInformation(input_write, HANDLE_FLAG_INHERIT, 0) == 0 {
                    CloseHandle(input_read);
                    CloseHandle(input_write);
                    CloseHandle(output_read);
                    CloseHandle(output_write);
                    return Err(format!(
                        "SetHandleInformation for pseudo console input failed: {}",
                        std::io::Error::last_os_error()
                    ));
                }
                if SetHandleInformation(output_read, HANDLE_FLAG_INHERIT, 0) == 0 {
                    CloseHandle(input_read);
                    CloseHandle(input_write);
                    CloseHandle(output_read);
                    CloseHandle(output_write);
                    return Err(format!(
                        "SetHandleInformation for pseudo console output failed: {}",
                        std::io::Error::last_os_error()
                    ));
                }
            }

            let mut pseudo_console = 0;
            let create_hr = unsafe {
                CreatePseudoConsole(
                    COORD {
                        X: columns,
                        Y: rows,
                    },
                    input_read,
                    output_write,
                    0,
                    &mut pseudo_console,
                )
            };
            unsafe {
                CloseHandle(input_read);
                CloseHandle(output_write);
            }
            if create_hr < 0 {
                unsafe {
                    CloseHandle(input_write);
                    CloseHandle(output_read);
                }
                return Err(format!(
                    "CreatePseudoConsole failed with HRESULT {create_hr:#x}"
                ));
            }

            let mut attribute_list_bytes = 0usize;
            unsafe {
                InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut attribute_list_bytes);
            }
            let mut attribute_list = vec![0u8; attribute_list_bytes];
            let attribute_list_ptr = attribute_list.as_mut_ptr().cast();
            if unsafe {
                InitializeProcThreadAttributeList(
                    attribute_list_ptr,
                    1,
                    0,
                    &mut attribute_list_bytes,
                )
            } == 0
            {
                unsafe {
                    ClosePseudoConsole(pseudo_console);
                    CloseHandle(input_write);
                    CloseHandle(output_read);
                }
                return Err(format!(
                    "InitializeProcThreadAttributeList failed: {}",
                    std::io::Error::last_os_error()
                ));
            }

            let update_ok = unsafe {
                UpdateProcThreadAttribute(
                    attribute_list_ptr,
                    0,
                    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                    &pseudo_console as *const _ as *const c_void,
                    mem::size_of::<HPCON>(),
                    ptr::null_mut(),
                    ptr::null(),
                )
            };
            if update_ok == 0 {
                unsafe {
                    DeleteProcThreadAttributeList(attribute_list_ptr);
                    ClosePseudoConsole(pseudo_console);
                    CloseHandle(input_write);
                    CloseHandle(output_read);
                }
                return Err(format!(
                    "UpdateProcThreadAttribute failed: {}",
                    std::io::Error::last_os_error()
                ));
            }

            let mut startup_info = STARTUPINFOEXW::default();
            startup_info.StartupInfo.cb = mem::size_of::<STARTUPINFOEXW>() as u32;
            startup_info.lpAttributeList = attribute_list_ptr;
            let mut process_info = PROCESS_INFORMATION::default();
            let mut command_line = wide_mut(&build_windows_command_line(command));

            let created = unsafe {
                CreateProcessW(
                    ptr::null(),
                    command_line.as_mut_ptr(),
                    ptr::null(),
                    ptr::null(),
                    0,
                    EXTENDED_STARTUPINFO_PRESENT,
                    ptr::null(),
                    ptr::null(),
                    &startup_info.StartupInfo,
                    &mut process_info,
                )
            };
            unsafe {
                DeleteProcThreadAttributeList(attribute_list_ptr);
            }
            if created == 0 {
                unsafe {
                    ClosePseudoConsole(pseudo_console);
                    CloseHandle(input_write);
                    CloseHandle(output_read);
                }
                return Err(format!(
                    "CreateProcessW failed: {}",
                    std::io::Error::last_os_error()
                ));
            }

            unsafe {
                CloseHandle(process_info.hThread);
            }

            spawn_output_reader(window_hwnd, pane_index, output_read);

            Ok(Self {
                pseudo_console,
                process_handle: process_info.hProcess,
                input_write,
            })
        }

        fn resize(&self, columns: i16, rows: i16) {
            let result = unsafe {
                ResizePseudoConsole(
                    self.pseudo_console,
                    COORD {
                        X: columns,
                        Y: rows,
                    },
                )
            };
            if result < 0 {
                logging::error(format!(
                    "ResizePseudoConsole failed with HRESULT {result:#x}"
                ));
            }
        }

        fn write_input(&self, bytes: &[u8]) -> Result<(), String> {
            let mut written = 0u32;
            if unsafe {
                WriteFile(
                    self.input_write,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    &mut written,
                    ptr::null_mut(),
                )
            } == 0
            {
                return Err(format!(
                    "WriteFile to pseudo console input failed: {}",
                    std::io::Error::last_os_error()
                ));
            }

            Ok(())
        }

        fn terminate(&mut self) {
            unsafe {
                if !self.process_handle.is_null() {
                    let _ = TerminateProcess(self.process_handle, 1);
                    CloseHandle(self.process_handle);
                    self.process_handle = ptr::null_mut();
                }
                if !self.input_write.is_null() {
                    CloseHandle(self.input_write);
                    self.input_write = ptr::null_mut();
                }
                if self.pseudo_console != 0 {
                    ClosePseudoConsole(self.pseudo_console);
                    self.pseudo_console = 0;
                }
            }
        }
    }

    impl Drop for PaneSession {
        fn drop(&mut self) {
            self.terminate();
        }
    }

    pub fn open_saved_workspaces(
        session: &SavedSession,
        distribution: &str,
    ) -> Result<(usize, usize), String> {
        let mut window_count = 0usize;
        let mut pane_count = 0usize;
        for tab in &session.tabs {
            open_workspace_window(tab.clone(), distribution)?;
            window_count += 1;
            pane_count += tab.preset.layout.tile_specs().len();
        }
        Ok((window_count, pane_count))
    }

    fn open_workspace_window(tab: SavedTab, distribution: &str) -> Result<(), String> {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err("could not resolve module handle for workspace window".into());
        }

        register_window_classes(instance)?;
        let window_title = tab
            .custom_title
            .clone()
            .unwrap_or_else(|| tab.preset.name.clone());
        let state = Box::new(WorkspaceWindowState {
            tab,
            distribution: distribution.to_string(),
            title_hwnd: ptr::null_mut(),
            path_hwnd: ptr::null_mut(),
            panes: Vec::new(),
        });
        let state_ptr = Box::into_raw(state);

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                wide(WINDOW_CLASS).as_ptr(),
                wide(&window_title).as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                i32::MIN,
                i32::MIN,
                1240,
                800,
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
            return Err("CreateWindowExW returned null for workspace host".into());
        }

        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);
        }

        Ok(())
    }

    fn register_window_classes(instance: HINSTANCE) -> Result<(), String> {
        register_class(instance, WINDOW_CLASS, window_proc)?;
        register_class(instance, PANE_CLASS, pane_window_proc)
    }

    fn register_class(
        instance: HINSTANCE,
        class_name: &str,
        window_proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
    ) -> Result<(), String> {
        let class_name_wide = wide(class_name);
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class_name_wide.as_ptr(),
            hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
            hbrBackground: (COLOR_WINDOW as isize + 1) as _,
            ..unsafe { mem::zeroed() }
        };

        let atom = unsafe { RegisterClassW(&window_class) };
        if atom == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(1410) {
                return Err(format!("RegisterClassW failed for {class_name}: {error}"));
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

                let state_ptr = unsafe { (*create).lpCreateParams as *mut WorkspaceWindowState };
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
                }
                1
            }
            WM_CREATE => {
                if let Some(state) = unsafe { window_state_mut(hwnd) } {
                    create_controls(hwnd, state);
                    spawn_pane_sessions(hwnd, state);
                }
                0
            }
            WM_SIZE => {
                if let Some(state) = unsafe { window_state_mut(hwnd) } {
                    layout_controls(hwnd, state);
                }
                0
            }
            WM_COMMAND => 0,
            WM_PANE_OUTPUT => {
                let event_ptr = lparam as *mut PaneOutputEvent;
                if !event_ptr.is_null() {
                    let event = unsafe { Box::from_raw(event_ptr) };
                    if let Some(state) = unsafe { window_state_mut(hwnd) } {
                        append_pane_output(state, event.pane_index, &event.text);
                    }
                }
                0
            }
            WM_PANE_EXIT => {
                if let Some(state) = unsafe { window_state_mut(hwnd) } {
                    mark_pane_exited(state, wparam as usize);
                }
                0
            }
            WM_DESTROY => 0,
            WM_NCDESTROY => {
                let state_ptr = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) }
                    as *mut WorkspaceWindowState;
                if !state_ptr.is_null() {
                    drop(unsafe { Box::from_raw(state_ptr) });
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    unsafe extern "system" fn pane_window_proc(
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

                let pane_ptr = unsafe { (*create).lpCreateParams as *mut PaneState };
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, pane_ptr as isize);
                }
                1
            }
            WM_PAINT => {
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    render_pane(hwnd, pane);
                    return 0;
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            WM_SETFOCUS => {
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    pane.focused = true;
                    unsafe {
                        InvalidateRect(hwnd, ptr::null(), 1);
                    }
                }
                0
            }
            WM_KILLFOCUS => {
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    pane.focused = false;
                    unsafe {
                        InvalidateRect(hwnd, ptr::null(), 1);
                    }
                }
                0
            }
            WM_LBUTTONDOWN => {
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    unsafe { SetFocus(hwnd) };
                    if forward_mouse_event(pane, lparam, MouseEvent::ButtonPress(0)) {
                        pane.pressed_mouse_button = Some(0);
                        return 0;
                    }
                    let position = pane_position_from_lparam(pane, lparam);
                    pane.selection_anchor = Some(position);
                    pane.selection_focus = Some(position);
                    pane.selecting = true;
                    unsafe { InvalidateRect(hwnd, ptr::null(), 1) };
                }
                0
            }
            WM_LBUTTONDBLCLK => {
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    unsafe { SetFocus(hwnd) };
                    let position = pane_position_from_lparam(pane, lparam);
                    let (start, end) = pane.terminal.word_selection_at(position);
                    pane.selection_anchor = Some(start);
                    pane.selection_focus = Some(end);
                    pane.selecting = false;
                    unsafe { InvalidateRect(hwnd, ptr::null(), 1) };
                }
                0
            }
            WM_MOUSEMOVE => {
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    if pane.pressed_mouse_button.is_some()
                        && (pane.terminal.mouse_tracking() == MouseTrackingMode::Drag
                            || pane.terminal.mouse_tracking() == MouseTrackingMode::Click)
                        && (wparam & 0x0001) != 0
                        && forward_mouse_event(pane, lparam, MouseEvent::Motion)
                    {
                        return 0;
                    }
                    if pane.selecting {
                        pane.selection_focus = Some(pane_position_from_lparam(pane, lparam));
                        unsafe { InvalidateRect(hwnd, ptr::null(), 1) };
                    }
                }
                0
            }
            WM_MOUSEWHEEL => {
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    if pane.terminal.mouse_tracking() != MouseTrackingMode::Disabled {
                        let delta = (((wparam >> 16) & 0xffff) as i16) as i32;
                        let event = if delta > 0 {
                            MouseEvent::WheelUp
                        } else {
                            MouseEvent::WheelDown
                        };
                        let _ = forward_mouse_event_from_screen(hwnd, pane, lparam, event);
                        return 0;
                    }
                    let delta = (((wparam >> 16) & 0xffff) as i16) as i32;
                    let rows = (delta / 120) * 3;
                    if rows != 0 && pane.terminal.scroll_viewport(rows as isize) {
                        update_pane_scrollbar(pane);
                        unsafe { InvalidateRect(hwnd, ptr::null(), 1) };
                    }
                }
                0
            }
            WM_LBUTTONUP => {
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    if pane.pressed_mouse_button.take().is_some()
                        && forward_mouse_event(pane, lparam, MouseEvent::ButtonRelease)
                    {
                        return 0;
                    }
                    if pane.selecting {
                        pane.selection_focus = Some(pane_position_from_lparam(pane, lparam));
                        pane.selecting = false;
                        unsafe { InvalidateRect(hwnd, ptr::null(), 1) };
                    }
                }
                0
            }
            WM_RBUTTONUP => {
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    unsafe { SetFocus(hwnd) };
                    show_pane_context_menu(hwnd, pane, lparam);
                }
                0
            }
            WM_CHAR => {
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    handle_char_input(pane, wparam as u32);
                }
                0
            }
            WM_KEYDOWN => {
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    handle_key_input(pane, wparam as u16);
                }
                0
            }
            WM_VSCROLL => {
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    handle_scrollbar_input(hwnd, pane, wparam);
                }
                0
            }
            WM_NCDESTROY => {
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    fn create_controls(hwnd: HWND, state: &mut WorkspaceWindowState) {
        let title = state
            .tab
            .custom_title
            .clone()
            .unwrap_or_else(|| state.tab.preset.name.clone());
        state.title_hwnd = create_child_window(
            hwnd,
            "STATIC",
            &title,
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            ptr::null_mut(),
        );
        state.path_hwnd = create_child_window(
            hwnd,
            "STATIC",
            &state.tab.workspace_root.display().to_string(),
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            ptr::null_mut(),
        );

        let ui_font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
        for control in [state.title_hwnd, state.path_hwnd] {
            if !control.is_null() {
                unsafe {
                    SendMessageW(control, WM_SETFONT, ui_font as usize, 1);
                }
            }
        }

        let tile_specs = state.tab.preset.layout.tile_specs();
        let font_points =
            effective_terminal_font_points(state.tab.preset.density, state.tab.terminal_zoom_steps);
        let line_height_scale = state.tab.preset.density.terminal_line_height_scale();
        state.panes = Vec::with_capacity(tile_specs.len());
        for tile in tile_specs {
            let mut pane = Box::new(PaneState {
                tile,
                title_hwnd: ptr::null_mut(),
                output_hwnd: ptr::null_mut(),
                terminal: VtBuffer::new(80, 24),
                focused: false,
                font: create_terminal_font(font_points),
                cell_width: 9,
                cell_height: 18,
                baseline_offset: 1,
                line_height_scale,
                selection_anchor: None,
                selection_focus: None,
                selecting: false,
                pressed_mouse_button: None,
                session: None,
            });
            pane.title_hwnd = create_child_window(
                hwnd,
                "STATIC",
                &format!("{}  •  {}", pane.tile.title, pane.tile.agent_label),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                ptr::null_mut(),
            );
            let pane_ptr: *mut PaneState = &mut *pane;
            pane.output_hwnd = create_child_window(
                hwnd,
                PANE_CLASS,
                "",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | WS_VSCROLL,
                0,
                0,
                pane_ptr.cast(),
            );
            if !pane.title_hwnd.is_null() {
                unsafe {
                    SendMessageW(pane.title_hwnd, WM_SETFONT, ui_font as usize, 1);
                }
            }
            update_terminal_metrics(&mut pane);
            update_pane_scrollbar(&pane);
            state.panes.push(pane);
        }

        layout_controls(hwnd, state);
    }

    fn layout_controls(hwnd: HWND, state: &mut WorkspaceWindowState) {
        let bounds = match client_bounds(hwnd) {
            Some(bounds) => bounds,
            None => return,
        };

        let title_width = (bounds.width() - (OUTER_MARGIN * 2)).max(320);
        unsafe {
            SetWindowPos(
                state.title_hwnd,
                ptr::null_mut(),
                OUTER_MARGIN,
                OUTER_MARGIN,
                title_width,
                22,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.path_hwnd,
                ptr::null_mut(),
                OUTER_MARGIN,
                OUTER_MARGIN + 24,
                title_width,
                18,
                SWP_NOZORDER,
            );
        }

        let layout_bounds = Bounds {
            left: OUTER_MARGIN,
            top: OUTER_MARGIN + HEADER_HEIGHT,
            right: bounds.right - OUTER_MARGIN,
            bottom: bounds.bottom - OUTER_MARGIN,
        };
        let mut pane_bounds = Vec::with_capacity(state.panes.len());
        collect_tile_bounds(&state.tab.preset.layout, layout_bounds, &mut pane_bounds);
        for (pane, bounds) in state.panes.iter_mut().zip(pane_bounds.into_iter()) {
            let output_top = bounds.top + PANE_TITLE_HEIGHT + 4;
            let output_height = (bounds.bottom - output_top).max(48);
            unsafe {
                SetWindowPos(
                    pane.title_hwnd,
                    ptr::null_mut(),
                    bounds.left,
                    bounds.top,
                    bounds.width().max(120),
                    PANE_TITLE_HEIGHT,
                    SWP_NOZORDER,
                );
                SetWindowPos(
                    pane.output_hwnd,
                    ptr::null_mut(),
                    bounds.left,
                    output_top,
                    bounds.width().max(120),
                    output_height,
                    SWP_NOZORDER,
                );
            }

            let (columns, rows) = pane_console_size(bounds.width(), output_height, pane);
            pane.terminal.resize(columns as usize, rows as usize);
            update_pane_scrollbar(pane);
            if let Some(session) = pane.session.as_ref() {
                session.resize(columns, rows);
            }
            unsafe {
                InvalidateRect(pane.output_hwnd, ptr::null(), 1);
            }
        }
    }

    fn spawn_pane_sessions(hwnd: HWND, state: &mut WorkspaceWindowState) {
        for (pane_index, pane) in state.panes.iter_mut().enumerate() {
            let command = match wsl::build_launch_command(
                &pane.tile,
                &state.tab.workspace_root,
                &state.distribution,
            ) {
                Ok(command) => command,
                Err(error) => {
                    pane.terminal.process(&format!(
                        "Could not prepare tile launch.\r\n\r\n{error}\r\n"
                    ));
                    unsafe {
                        InvalidateRect(pane.output_hwnd, ptr::null(), 1);
                    }
                    continue;
                }
            };

            let output_bounds = client_bounds(pane.output_hwnd).unwrap_or(Bounds {
                left: 0,
                top: 0,
                right: 720,
                bottom: 420,
            });
            let (columns, rows) =
                pane_console_size(output_bounds.width(), output_bounds.height(), pane);
            pane.terminal.resize(columns as usize, rows as usize);

            match PaneSession::spawn(hwnd, pane_index, &command, columns, rows) {
                Ok(session) => {
                    pane.session = Some(session);
                    pane.terminal.process(&format!(
                        "[launching {} in {}]\r\n",
                        pane.tile.title, command.working_directory
                    ));
                }
                Err(error) => {
                    pane.terminal
                        .process(&format!("Could not spawn tile process.\r\n\r\n{error}\r\n"));
                }
            }
            update_pane_scrollbar(pane);

            unsafe {
                InvalidateRect(pane.output_hwnd, ptr::null(), 1);
            }
        }
    }

    fn append_pane_output(state: &mut WorkspaceWindowState, pane_index: usize, chunk: &str) {
        let Some(pane) = state.panes.get_mut(pane_index) else {
            return;
        };

        pane.terminal.process(chunk);
        let pending_input = pane.terminal.take_pending_input();
        if !pending_input.is_empty()
            && let Some(session) = pane.session.as_ref()
            && let Err(error) = session.write_input(&pending_input)
        {
            logging::error(format!("pane control response write failed: {error}"));
        }
        update_pane_scrollbar(pane);
        let header = if let Some(title) = pane.terminal.window_title() {
            format!("{}  •  {}", pane.tile.title, title)
        } else if let Some(cwd) = pane.terminal.current_working_directory() {
            format!("{}  •  {}", pane.tile.title, cwd)
        } else {
            format!("{}  •  {}", pane.tile.title, pane.tile.agent_label)
        };
        unsafe {
            SetWindowTextW(pane.title_hwnd, wide(&header).as_ptr());
        }
        unsafe {
            InvalidateRect(pane.output_hwnd, ptr::null(), 1);
        }
    }

    fn mark_pane_exited(state: &mut WorkspaceWindowState, pane_index: usize) {
        let Some(pane) = state.panes.get_mut(pane_index) else {
            return;
        };

        pane.session = None;
        pane.terminal
            .process("\r\n\r\n[terminal session exited]\r\n");
        update_pane_scrollbar(pane);
        unsafe {
            InvalidateRect(pane.output_hwnd, ptr::null(), 1);
        }
    }

    fn render_pane(hwnd: HWND, pane: &PaneState) {
        let mut paint = PAINTSTRUCT::default();
        let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
        if hdc.is_null() {
            return;
        }

        let rect = client_bounds(hwnd).unwrap_or(Bounds {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        });
        fill_rect_color(
            hdc,
            RECT {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            },
            palette_color(0),
        );

        let old_font = unsafe { SelectObject(hdc, pane.font as HGDIOBJ) };
        for row in 0..pane.terminal.rows() {
            let mut column = 0usize;
            while column < pane.terminal.columns() {
                let mut text = String::new();
                let cell = pane.terminal.visible_cell(row, column);
                let style =
                    resolved_style(cell.style, selection_contains(pane, row, column), false);
                let start_column = column;
                while column < pane.terminal.columns() {
                    let next = pane.terminal.visible_cell(row, column);
                    if resolved_style(next.style, selection_contains(pane, row, column), false)
                        != style
                    {
                        break;
                    }
                    text.push(next.ch);
                    column += 1;
                }

                let run_rect = RECT {
                    left: (start_column as i32) * pane.cell_width,
                    top: (row as i32) * pane.cell_height,
                    right: (column as i32) * pane.cell_width,
                    bottom: ((row + 1) as i32) * pane.cell_height,
                };
                fill_rect_color(hdc, run_rect, style.bg);
                unsafe {
                    SetTextColor(hdc, style.fg);
                    SetBkColor(hdc, style.bg);
                }
                let text_wide = wide_no_nul(&text);
                unsafe {
                    TextOutW(
                        hdc,
                        run_rect.left,
                        run_rect.top + pane.baseline_offset,
                        text_wide.as_ptr(),
                        text_wide.len() as i32,
                    );
                }
            }
        }

        if pane.focused
            && pane.terminal.cursor_visible()
            && let Some((cursor_col, cursor_row)) = pane.terminal.cursor_in_view()
        {
            let cell = pane.terminal.visible_cell(cursor_row, cursor_col);
            let cursor_style = resolved_style(
                cell.style,
                selection_contains(pane, cursor_row, cursor_col),
                true,
            );
            let cursor_rect = RECT {
                left: (cursor_col as i32) * pane.cell_width,
                top: (cursor_row as i32) * pane.cell_height,
                right: ((cursor_col + 1) as i32) * pane.cell_width,
                bottom: ((cursor_row + 1) as i32) * pane.cell_height,
            };
            fill_rect_color(hdc, cursor_rect, cursor_style.bg);
            unsafe {
                SetTextColor(hdc, cursor_style.fg);
                SetBkColor(hdc, cursor_style.bg);
            }
            let text_wide = wide_no_nul(&cell.ch.to_string());
            unsafe {
                TextOutW(
                    hdc,
                    cursor_rect.left,
                    cursor_rect.top + pane.baseline_offset,
                    text_wide.as_ptr(),
                    text_wide.len() as i32,
                );
            }
        }

        unsafe {
            SelectObject(hdc, old_font);
            EndPaint(hwnd, &paint);
        }
    }

    fn handle_char_input(pane: &mut PaneState, value: u32) {
        if is_modifier_pressed(VK_CONTROL) && is_modifier_pressed(VK_SHIFT) {
            if value == 'C' as u32 {
                let _ = copy_pane_selection_to_clipboard(pane);
                return;
            }
            if value == 'V' as u32 {
                let _ = paste_clipboard_into_pane(pane);
                return;
            }
        }

        let bytes = match value {
            8 => vec![0x7f],
            13 => vec![b'\r'],
            value if value < 32 => vec![value as u8],
            value => {
                let Some(character) = char::from_u32(value) else {
                    return;
                };
                let mut encoded = [0u8; 4];
                character.encode_utf8(&mut encoded).as_bytes().to_vec()
            }
        };
        let _ = pane.terminal.reset_viewport();
        clear_selection(pane);
        let Some(session) = pane.session.as_ref() else {
            return;
        };
        if let Err(error) = session.write_input(&bytes) {
            logging::error(format!("pane input write failed: {error}"));
        }
    }

    fn handle_key_input(pane: &mut PaneState, virtual_key: u16) {
        if virtual_key == VK_INSERT && is_modifier_pressed(VK_SHIFT) {
            let _ = paste_clipboard_into_pane(pane);
            return;
        }
        if virtual_key == VK_INSERT && is_modifier_pressed(VK_CONTROL) {
            let _ = copy_pane_selection_to_clipboard(pane);
            return;
        }

        let sequence = match virtual_key {
            VK_LEFT => Some(terminal_key_sequence(pane, "\u{1b}[D", "\u{1b}OD")),
            VK_RIGHT => Some(terminal_key_sequence(pane, "\u{1b}[C", "\u{1b}OC")),
            VK_UP => Some(terminal_key_sequence(pane, "\u{1b}[A", "\u{1b}OA")),
            VK_DOWN => Some(terminal_key_sequence(pane, "\u{1b}[B", "\u{1b}OB")),
            VK_HOME => Some(terminal_key_sequence(pane, "\u{1b}[H", "\u{1b}OH")),
            VK_END => Some(terminal_key_sequence(pane, "\u{1b}[F", "\u{1b}OF")),
            VK_INSERT => Some("\u{1b}[2~"),
            VK_DELETE => Some("\u{1b}[3~"),
            VK_PRIOR => Some("\u{1b}[5~"),
            VK_NEXT => Some("\u{1b}[6~"),
            _ => None,
        };

        if let Some(sequence) = sequence {
            let _ = pane.terminal.reset_viewport();
            clear_selection(pane);
            let Some(session) = pane.session.as_ref() else {
                return;
            };
            if let Err(error) = session.write_input(sequence.as_bytes()) {
                logging::error(format!("pane special key write failed: {error}"));
            }
        }
    }

    fn fill_rect_color(hdc: windows_sys::Win32::Graphics::Gdi::HDC, rect: RECT, color: COLORREF) {
        let brush = unsafe { CreateSolidBrush(color) };
        if brush.is_null() {
            return;
        }
        unsafe {
            FillRect(hdc, &rect, brush as HBRUSH);
            DeleteObject(brush as HGDIOBJ);
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    struct ResolvedStyle {
        fg: COLORREF,
        bg: COLORREF,
    }

    fn resolved_style(style: VtStyle, selected: bool, cursor: bool) -> ResolvedStyle {
        let mut fg = style.fg;
        let mut bg = style.bg;

        if style.inverse {
            std::mem::swap(&mut fg, &mut bg);
        }
        if selected {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cursor {
            std::mem::swap(&mut fg, &mut bg);
            bg = match bg {
                VtColor::DefaultBackground => VtColor::Indexed(8),
                VtColor::DefaultForeground => VtColor::Indexed(15),
                other => other,
            };
        }

        ResolvedStyle {
            fg: resolve_terminal_color(fg, true, style.bold),
            bg: resolve_terminal_color(bg, false, false),
        }
    }

    fn resolve_terminal_color(color: VtColor, foreground: bool, bold: bool) -> COLORREF {
        match color {
            VtColor::DefaultForeground => palette_color(7),
            VtColor::DefaultBackground => palette_color(0),
            VtColor::Indexed(index) => {
                let index = if foreground && bold && index < 8 {
                    index + 8
                } else {
                    index
                };
                palette_color(index)
            }
            VtColor::Rgb(red, green, blue) => rgb(red, green, blue),
        }
    }

    fn palette_color(index: u8) -> COLORREF {
        const PALETTE: [COLORREF; 16] = [
            rgb(30, 31, 41),
            rgb(232, 95, 111),
            rgb(144, 190, 109),
            rgb(229, 192, 123),
            rgb(97, 175, 239),
            rgb(198, 120, 221),
            rgb(86, 182, 194),
            rgb(220, 223, 228),
            rgb(92, 99, 112),
            rgb(255, 123, 114),
            rgb(152, 195, 121),
            rgb(241, 196, 15),
            rgb(97, 175, 239),
            rgb(209, 154, 255),
            rgb(86, 182, 194),
            rgb(255, 255, 255),
        ];

        match index {
            0..=15 => PALETTE[index as usize],
            16..=231 => {
                let color = index - 16;
                let red = color / 36;
                let green = (color % 36) / 6;
                let blue = color % 6;
                rgb(
                    cube_component(red),
                    cube_component(green),
                    cube_component(blue),
                )
            }
            232..=255 => {
                let gray = 8 + ((index - 232) * 10);
                rgb(gray, gray, gray)
            }
        }
    }

    const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
        red as u32 | ((green as u32) << 8) | ((blue as u32) << 16)
    }

    const fn cube_component(value: u8) -> u8 {
        if value == 0 { 0 } else { 55 + (value * 40) }
    }

    fn clamp_terminal_zoom_steps(density: ApplicationDensity, zoom_steps: i32) -> i32 {
        let base_points = density.terminal_font_points();
        (base_points + zoom_steps).clamp(MIN_TERMINAL_FONT_POINTS, MAX_TERMINAL_FONT_POINTS)
            - base_points
    }

    fn effective_terminal_font_points(density: ApplicationDensity, zoom_steps: i32) -> i32 {
        density.terminal_font_points() + clamp_terminal_zoom_steps(density, zoom_steps)
    }

    fn create_terminal_font(font_points: i32) -> HFONT {
        let hdc = unsafe { GetDC(ptr::null_mut()) };
        let dpi_y = if hdc.is_null() {
            96
        } else {
            let dpi = unsafe { GetDeviceCaps(hdc, LOGPIXELSY as i32) };
            unsafe {
                ReleaseDC(ptr::null_mut(), hdc);
            }
            dpi.max(96)
        };
        let pixel_height = -((font_points.max(1) * dpi_y + 36) / 72);

        unsafe {
            CreateFontW(
                pixel_height,
                0,
                0,
                0,
                FW_NORMAL as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32,
                (FIXED_PITCH as u32) | (FF_MODERN as u32),
                wide(DEFAULT_FONT_FACE).as_ptr(),
            )
        }
    }

    fn update_terminal_metrics(pane: &mut PaneState) {
        let hdc = unsafe { GetDC(ptr::null_mut()) };
        if hdc.is_null() {
            return;
        }

        let old_font = unsafe { SelectObject(hdc, pane.font as HGDIOBJ) };
        let mut metrics = TEXTMETRICW::default();
        let mut size = SIZE::default();
        let sample = wide_no_nul("MMMMMMMM");

        if unsafe { GetTextMetricsW(hdc, &mut metrics) } != 0
            && unsafe {
                GetTextExtentPoint32W(hdc, sample.as_ptr(), sample.len() as i32, &mut size)
            } != 0
        {
            let average_width = (size.cx / (sample.len() as i32)).max(metrics.tmAveCharWidth);
            let draw_height = size.cy.max(metrics.tmHeight - metrics.tmInternalLeading);
            pane.cell_width = average_width.max(1);
            pane.cell_height = ((draw_height as f64 * pane.line_height_scale).round() as i32)
                .max(draw_height + metrics.tmExternalLeading.max(1))
                .max(1);
            pane.baseline_offset = ((pane.cell_height - draw_height) / 2).max(0);
        }

        unsafe {
            SelectObject(hdc, old_font);
            ReleaseDC(ptr::null_mut(), hdc);
        }
    }

    fn create_child_window(
        hwnd: HWND,
        class_name: &str,
        text: &str,
        style: u32,
        ex_style: WINDOW_EX_STYLE,
        control_id: isize,
        lp_param: *mut c_void,
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
                lp_param,
            )
        }
    }

    fn selection_contains(pane: &PaneState, row: usize, column: usize) -> bool {
        let (Some(anchor), Some(focus)) = (pane.selection_anchor, pane.selection_focus) else {
            return false;
        };
        let current = VtPosition { row, column };
        let (start, end) = normalize_selection(anchor, focus);
        current >= start && current <= end
    }

    fn normalize_selection(start: VtPosition, end: VtPosition) -> (VtPosition, VtPosition) {
        if start <= end {
            (start, end)
        } else {
            (end, start)
        }
    }

    fn pane_position_from_lparam(pane: &PaneState, lparam: LPARAM) -> VtPosition {
        let x = ((lparam as i32) & 0xffff) as i16 as i32;
        let y = (((lparam as i32) >> 16) & 0xffff) as i16 as i32;
        let row = (y.max(0) / pane.cell_height.max(1)) as usize;
        let column = (x.max(0) / pane.cell_width.max(1)) as usize;
        VtPosition {
            row: row.min(pane.terminal.rows().saturating_sub(1)),
            column: column.min(pane.terminal.columns().saturating_sub(1)),
        }
    }

    fn is_modifier_pressed(virtual_key: u16) -> bool {
        (unsafe { GetKeyState(virtual_key as i32) }) < 0
    }

    fn terminal_key_sequence<'a>(
        pane: &PaneState,
        normal_sequence: &'a str,
        application_sequence: &'a str,
    ) -> &'a str {
        if pane.terminal.application_cursor_keys() {
            application_sequence
        } else {
            normal_sequence
        }
    }

    #[derive(Clone, Copy)]
    enum MouseEvent {
        ButtonPress(u8),
        ButtonRelease,
        Motion,
        WheelUp,
        WheelDown,
    }

    fn forward_mouse_event(pane: &mut PaneState, lparam: LPARAM, event: MouseEvent) -> bool {
        if pane.terminal.mouse_tracking() == MouseTrackingMode::Disabled {
            return false;
        }
        if matches!(event, MouseEvent::Motion)
            && pane.terminal.mouse_tracking() != MouseTrackingMode::Drag
        {
            return false;
        }

        let position = pane_position_from_lparam(pane, lparam);
        let Some(report) = encode_mouse_report(
            &pane.terminal,
            event,
            position.column + 1,
            position.row + 1,
            pane.pressed_mouse_button,
        ) else {
            return false;
        };

        let Some(session) = pane.session.as_ref() else {
            return false;
        };

        if let Err(error) = session.write_input(report.as_bytes()) {
            logging::error(format!("pane mouse report write failed: {error}"));
        }
        true
    }

    fn forward_mouse_event_from_screen(
        hwnd: HWND,
        pane: &mut PaneState,
        lparam: LPARAM,
        event: MouseEvent,
    ) -> bool {
        let mut point = POINT {
            x: ((lparam as i32) & 0xffff) as i16 as i32,
            y: (((lparam as i32) >> 16) & 0xffff) as i16 as i32,
        };
        unsafe {
            ScreenToClient(hwnd, &mut point);
        }
        let client_lparam = ((point.y as u32) << 16 | (point.x as u32 & 0xffff)) as isize;
        forward_mouse_event(pane, client_lparam, event)
    }

    fn encode_mouse_report(
        terminal: &VtBuffer,
        event: MouseEvent,
        column: usize,
        row: usize,
        pressed_button: Option<u8>,
    ) -> Option<String> {
        let mut code = match event {
            MouseEvent::ButtonPress(button) => button,
            MouseEvent::ButtonRelease => 3,
            MouseEvent::Motion => pressed_button.unwrap_or(0).saturating_add(32),
            MouseEvent::WheelUp => 64,
            MouseEvent::WheelDown => 65,
        };

        if matches!(event, MouseEvent::Motion) && pressed_button.is_none() {
            return None;
        }

        if terminal.sgr_mouse_mode() {
            let suffix = match event {
                MouseEvent::ButtonRelease => "m",
                _ => "M",
            };
            return Some(format!("\u{1b}[<{code};{column};{row}{suffix}"));
        }

        if matches!(event, MouseEvent::Motion) {
            code = code.saturating_add(0);
        }
        let encoded_column = (column.min(223) as u8).saturating_add(32) as char;
        let encoded_row = (row.min(223) as u8).saturating_add(32) as char;
        Some(format!(
            "\u{1b}[M{}{}{}",
            (code.saturating_add(32)) as char,
            encoded_column,
            encoded_row
        ))
    }

    fn update_pane_scrollbar(pane: &PaneState) {
        let total_rows = pane.terminal.history_len() + pane.terminal.rows();
        let top_position = pane
            .terminal
            .history_len()
            .saturating_sub(pane.terminal.viewport_offset()) as i32;
        let info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: total_rows.saturating_sub(1) as i32,
            nPage: pane.terminal.rows() as u32,
            nPos: top_position,
            nTrackPos: top_position,
        };
        unsafe {
            SetScrollInfo(pane.output_hwnd, SB_VERT, &info, 1);
        }
    }

    fn handle_scrollbar_input(hwnd: HWND, pane: &mut PaneState, wparam: WPARAM) {
        let action = (wparam & 0xffff) as i32;
        let thumb_pos = ((wparam >> 16) & 0xffff) as i32;
        let max_top = pane.terminal.history_len() as i32;
        let current_top = pane
            .terminal
            .history_len()
            .saturating_sub(pane.terminal.viewport_offset()) as i32;
        let page = pane.terminal.rows() as i32;

        let next_top = match action {
            SB_TOP => 0,
            SB_BOTTOM => max_top,
            SB_LINEUP => (current_top - 1).max(0),
            SB_LINEDOWN => (current_top + 1).min(max_top),
            SB_PAGEUP => (current_top - page).max(0),
            SB_PAGEDOWN => (current_top + page).min(max_top),
            SB_THUMBPOSITION | SB_THUMBTRACK => thumb_pos.clamp(0, max_top),
            _ => return,
        };

        let offset = pane
            .terminal
            .history_len()
            .saturating_sub(next_top as usize);
        if pane
            .terminal
            .scroll_viewport(offset as isize - pane.terminal.viewport_offset() as isize)
        {
            update_pane_scrollbar(pane);
            unsafe {
                InvalidateRect(hwnd, ptr::null(), 1);
            }
        }
    }

    fn show_pane_context_menu(hwnd: HWND, pane: &mut PaneState, lparam: LPARAM) {
        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            return;
        }

        let has_selection = pane
            .selection_anchor
            .zip(pane.selection_focus)
            .is_some_and(|(anchor, focus)| pane.terminal.has_selection(anchor, focus));
        unsafe {
            AppendMenuW(
                menu,
                MF_STRING | if has_selection { 0 } else { MF_GRAYED },
                MENU_COPY_SELECTION,
                wide("Copy").as_ptr(),
            );
            AppendMenuW(
                menu,
                MF_STRING,
                MENU_PASTE_CLIPBOARD,
                wide("Paste").as_ptr(),
            );
        }

        let mut point = POINT {
            x: ((lparam as i32) & 0xffff) as i16 as i32,
            y: (((lparam as i32) >> 16) & 0xffff) as i16 as i32,
        };
        unsafe {
            ClientToScreen(hwnd, &mut point);
        }
        let command = unsafe {
            TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                0,
                hwnd,
                ptr::null(),
            )
        };
        match command as usize {
            MENU_COPY_SELECTION => {
                let _ = copy_pane_selection_to_clipboard(pane);
            }
            MENU_PASTE_CLIPBOARD => {
                let _ = paste_clipboard_into_pane(pane);
            }
            _ => {}
        }
        unsafe {
            DestroyMenu(menu);
        }
    }

    fn clear_selection(pane: &mut PaneState) {
        pane.selection_anchor = None;
        pane.selection_focus = None;
        pane.selecting = false;
        unsafe {
            InvalidateRect(pane.output_hwnd, ptr::null(), 1);
        }
    }

    fn copy_pane_selection_to_clipboard(pane: &PaneState) -> Result<(), String> {
        let (Some(anchor), Some(focus)) = (pane.selection_anchor, pane.selection_focus) else {
            return Ok(());
        };
        let text = pane.terminal.selection_text(anchor, focus);
        if text.is_empty() {
            return Ok(());
        }
        write_clipboard_text(&text)
    }

    fn paste_clipboard_into_pane(pane: &mut PaneState) -> Result<(), String> {
        let Some(text) = read_clipboard_text()? else {
            return Ok(());
        };
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let _ = pane.terminal.reset_viewport();
        clear_selection(pane);
        let Some(session) = pane.session.as_ref() else {
            return Ok(());
        };
        if pane.terminal.bracketed_paste() {
            let wrapped = format!("\u{1b}[200~{normalized}\u{1b}[201~");
            session.write_input(wrapped.as_bytes())
        } else {
            session.write_input(normalized.as_bytes())
        }
    }

    fn write_clipboard_text(text: &str) -> Result<(), String> {
        let data = wide(text);
        let bytes = data.len() * std::mem::size_of::<u16>();
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) };
        if handle.is_null() {
            return Err(format!(
                "GlobalAlloc for clipboard failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        let target = unsafe { GlobalLock(handle) } as *mut u16;
        if target.is_null() {
            unsafe {
                GlobalFree(handle);
            }
            return Err(format!(
                "GlobalLock for clipboard failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), target, data.len());
            GlobalUnlock(handle);
        }

        if unsafe { OpenClipboard(ptr::null_mut()) } == 0 {
            unsafe {
                GlobalFree(handle);
            }
            return Err(format!(
                "OpenClipboard failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        let mut result = Ok(());
        unsafe {
            if EmptyClipboard() == 0 {
                result = Err(format!(
                    "EmptyClipboard failed: {}",
                    std::io::Error::last_os_error()
                ));
            } else if SetClipboardData(CF_UNICODETEXT, handle).is_null() {
                result = Err(format!(
                    "SetClipboardData failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            CloseClipboard();
        }
        if result.is_err() {
            unsafe {
                GlobalFree(handle);
            }
        }
        result
    }

    fn read_clipboard_text() -> Result<Option<String>, String> {
        if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) } == 0 {
            return Ok(None);
        }
        if unsafe { OpenClipboard(ptr::null_mut()) } == 0 {
            return Err(format!(
                "OpenClipboard failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        let handle = unsafe { GetClipboardData(CF_UNICODETEXT) };
        if handle.is_null() {
            unsafe {
                CloseClipboard();
            }
            return Ok(None);
        }

        let text = unsafe {
            let size_bytes = GlobalSize(handle);
            let source = GlobalLock(handle) as *const u16;
            if source.is_null() || size_bytes == 0 {
                None
            } else {
                let length = size_bytes / std::mem::size_of::<u16>();
                let slice = std::slice::from_raw_parts(source, length);
                let nul = slice
                    .iter()
                    .position(|value| *value == 0)
                    .unwrap_or(slice.len());
                let string = String::from_utf16_lossy(&slice[..nul]);
                GlobalUnlock(handle);
                Some(string)
            }
        };
        unsafe {
            CloseClipboard();
        }

        Ok(text)
    }

    fn client_bounds(hwnd: HWND) -> Option<Bounds> {
        let mut rect = unsafe { mem::zeroed::<RECT>() };
        if unsafe { GetClientRect(hwnd, &mut rect) } == 0 {
            None
        } else {
            Some(Bounds {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            })
        }
    }

    fn collect_tile_bounds(node: &LayoutNode, bounds: Bounds, output: &mut Vec<Bounds>) {
        match node {
            LayoutNode::Tile(_) => output.push(bounds),
            LayoutNode::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let ratio = ratio.clamp(0.1, 0.9);
                match axis {
                    SplitAxis::Horizontal => {
                        let available = (bounds.width() - PANE_GAP).max(2);
                        let first_width = ((available as f32) * ratio).round() as i32;
                        let second_left = bounds.left + first_width + PANE_GAP;
                        collect_tile_bounds(
                            first,
                            Bounds {
                                left: bounds.left,
                                top: bounds.top,
                                right: bounds.left + first_width,
                                bottom: bounds.bottom,
                            },
                            output,
                        );
                        collect_tile_bounds(
                            second,
                            Bounds {
                                left: second_left,
                                top: bounds.top,
                                right: bounds.right,
                                bottom: bounds.bottom,
                            },
                            output,
                        );
                    }
                    SplitAxis::Vertical => {
                        let available = (bounds.height() - PANE_GAP).max(2);
                        let first_height = ((available as f32) * ratio).round() as i32;
                        let second_top = bounds.top + first_height + PANE_GAP;
                        collect_tile_bounds(
                            first,
                            Bounds {
                                left: bounds.left,
                                top: bounds.top,
                                right: bounds.right,
                                bottom: bounds.top + first_height,
                            },
                            output,
                        );
                        collect_tile_bounds(
                            second,
                            Bounds {
                                left: bounds.left,
                                top: second_top,
                                right: bounds.right,
                                bottom: bounds.bottom,
                            },
                            output,
                        );
                    }
                }
            }
        }
    }

    fn pane_console_size(width: i32, height: i32, pane: &PaneState) -> (i16, i16) {
        let columns = (width.max(pane.cell_width) / pane.cell_width.max(1))
            .clamp(MIN_PANE_CHARS_X as i32, i16::MAX as i32) as i16;
        let rows = (height.max(pane.cell_height) / pane.cell_height.max(1))
            .clamp(MIN_PANE_CHARS_Y as i32, i16::MAX as i32) as i16;
        (columns, rows)
    }

    fn build_windows_command_line(command: &WslLaunchCommand) -> String {
        let mut command_line = quote_windows_arg(&command.program);
        for arg in &command.args {
            command_line.push(' ');
            command_line.push_str(&quote_windows_arg(arg));
        }
        command_line
    }

    fn quote_windows_arg(value: &str) -> String {
        if !value.contains([' ', '\t', '"']) {
            return value.to_string();
        }

        let mut quoted = String::from("\"");
        let mut backslashes = 0usize;
        for character in value.chars() {
            match character {
                '\\' => {
                    backslashes += 1;
                    quoted.push('\\');
                }
                '"' => {
                    quoted.push_str(&"\\".repeat(backslashes));
                    backslashes = 0;
                    quoted.push('\\');
                    quoted.push('"');
                }
                _ => {
                    backslashes = 0;
                    quoted.push(character);
                }
            }
        }
        if backslashes > 0 {
            quoted.push_str(&"\\".repeat(backslashes));
        }
        quoted.push('"');
        quoted
    }

    fn spawn_output_reader(window_hwnd: HWND, pane_index: usize, output_read: HANDLE) {
        let window_hwnd = window_hwnd as isize;
        let output_read = output_read as isize;
        thread::spawn(move || {
            let window_hwnd = window_hwnd as HWND;
            let output_read = output_read as HANDLE;
            let mut buffer = [0u8; 4096];
            loop {
                let mut bytes_read = 0u32;
                let read_ok = unsafe {
                    ReadFile(
                        output_read,
                        buffer.as_mut_ptr(),
                        buffer.len() as u32,
                        &mut bytes_read,
                        ptr::null_mut(),
                    )
                };
                if read_ok == 0 || bytes_read == 0 {
                    break;
                }

                let text = String::from_utf8_lossy(&buffer[..bytes_read as usize]).into_owned();
                let event = Box::new(PaneOutputEvent { pane_index, text });
                let event_ptr = Box::into_raw(event);
                if unsafe { PostMessageW(window_hwnd, WM_PANE_OUTPUT, 0, event_ptr as LPARAM) } == 0
                {
                    unsafe {
                        drop(Box::from_raw(event_ptr));
                    }
                    break;
                }
            }

            unsafe {
                CloseHandle(output_read);
            }
            let _ = unsafe { PostMessageW(window_hwnd, WM_PANE_EXIT, pane_index, 0) };
        });
    }

    unsafe fn window_state_mut(hwnd: HWND) -> Option<&'static mut WorkspaceWindowState> {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut WorkspaceWindowState;
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *ptr })
        }
    }

    unsafe fn pane_state_mut(hwnd: HWND) -> Option<&'static mut PaneState> {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut PaneState;
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *ptr })
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn wide_mut(value: &str) -> Vec<u16> {
        wide(value)
    }

    fn wide_no_nul(value: &str) -> Vec<u16> {
        value.encode_utf16().collect()
    }
}

#[cfg(target_os = "windows")]
pub use imp::open_saved_workspaces;
