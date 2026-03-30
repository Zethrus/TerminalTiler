#[cfg(target_os = "windows")]
mod imp {
    use std::ffi::c_void;
    use std::mem;
    use std::ptr;
    use std::thread;

    use windows_sys::Win32::Foundation::{
        CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, HWND, LPARAM, LRESULT, RECT,
        SetHandleInformation, WPARAM,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        COLOR_WINDOW, DEFAULT_GUI_FONT, GetStockObject, UpdateWindow,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::ReadFile;
    use windows_sys::Win32::System::Console::{
        COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
        InitializeProcThreadAttributeList, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
        PROCESS_INFORMATION, STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, ES_AUTOVSCROLL,
        ES_LEFT, ES_MULTILINE, ES_READONLY, GWLP_USERDATA, GetClientRect, GetWindowLongPtrW, HMENU,
        IDC_ARROW, LoadCursorW, PostMessageW, RegisterClassW, SW_SHOW, SWP_NOZORDER, SendMessageW,
        SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, WINDOW_EX_STYLE, WM_APP,
        WM_COMMAND, WM_CREATE, WM_DESTROY, WM_NCCREATE, WM_NCDESTROY, WM_SETFONT, WM_SIZE,
        WNDCLASSW, WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE, WS_VSCROLL,
    };

    use crate::logging;
    use crate::model::layout::{LayoutNode, SplitAxis, TileSpec};
    use crate::storage::session_store::{SavedSession, SavedTab};
    use crate::windows::wsl::{self, WslLaunchCommand};

    const WINDOW_CLASS: &str = "TerminalTilerWindowsWorkspace";
    const WM_PANE_OUTPUT: u32 = WM_APP + 1;
    const WM_PANE_EXIT: u32 = WM_APP + 2;
    const HEADER_HEIGHT: i32 = 56;
    const OUTER_MARGIN: i32 = 12;
    const PANE_GAP: i32 = 8;
    const PANE_TITLE_HEIGHT: i32 = 20;
    const MIN_PANE_CHARS_X: i16 = 40;
    const MIN_PANE_CHARS_Y: i16 = 12;
    const APPROX_CELL_WIDTH: i32 = 9;
    const APPROX_CELL_HEIGHT: i32 = 18;
    const MAX_BUFFER_CHARS: usize = 64_000;

    struct WorkspaceWindowState {
        tab: SavedTab,
        distribution: String,
        title_hwnd: HWND,
        path_hwnd: HWND,
        panes: Vec<PaneState>,
    }

    struct PaneState {
        tile: TileSpec,
        title_hwnd: HWND,
        output_hwnd: HWND,
        buffer: String,
        session: Option<PaneSession>,
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

        register_window_class(instance)?;
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

    fn register_window_class(instance: HANDLE) -> Result<(), String> {
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
            let error = std::io::Error::last_os_error();
            let already_exists = error.raw_os_error() == Some(1410);
            if !already_exists {
                return Err(format!("RegisterClassW failed for workspace host: {error}"));
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
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    create_controls(hwnd, state);
                    spawn_pane_sessions(hwnd, state);
                }
                0
            }
            WM_SIZE => {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    layout_controls(hwnd, state);
                }
                0
            }
            WM_COMMAND => 0,
            WM_PANE_OUTPUT => {
                let event_ptr = lparam as *mut PaneOutputEvent;
                if !event_ptr.is_null() {
                    let event = unsafe { Box::from_raw(event_ptr) };
                    if let Some(state) = unsafe { state_mut(hwnd) } {
                        append_pane_output(state, event.pane_index, &event.text);
                    }
                }
                0
            }
            WM_PANE_EXIT => {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    append_pane_output(
                        state,
                        wparam as usize,
                        "\r\n\r\n[terminal session exited]\r\n",
                    );
                }
                0
            }
            WM_DESTROY => 0,
            WM_NCDESTROY => {
                let state_ptr = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) }
                    as *mut WorkspaceWindowState;
                if !state_ptr.is_null() {
                    let mut state = unsafe { Box::from_raw(state_ptr) };
                    for pane in &mut state.panes {
                        if let Some(session) = pane.session.as_mut() {
                            session.terminate();
                        }
                    }
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
        state.title_hwnd = create_child_window(hwnd, "STATIC", &title, WS_CHILD | WS_VISIBLE, 0, 0);
        state.path_hwnd = create_child_window(
            hwnd,
            "STATIC",
            &state.tab.workspace_root.display().to_string(),
            WS_CHILD | WS_VISIBLE,
            0,
            0,
        );

        let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
        for control in [state.title_hwnd, state.path_hwnd] {
            if !control.is_null() {
                unsafe {
                    SendMessageW(control, WM_SETFONT, font as usize, 1);
                }
            }
        }

        for tile in state.tab.preset.layout.tile_specs() {
            let title_hwnd = create_child_window(
                hwnd,
                "STATIC",
                &format!("{}  •  {}", tile.title, tile.agent_label),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
            );
            let output_hwnd = create_child_window(
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
                0,
            );

            for control in [title_hwnd, output_hwnd] {
                if !control.is_null() {
                    unsafe {
                        SendMessageW(control, WM_SETFONT, font as usize, 1);
                    }
                }
            }

            state.panes.push(PaneState {
                tile,
                title_hwnd,
                output_hwnd,
                buffer: String::new(),
                session: None,
            });
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

            if let Some(session) = pane.session.as_ref() {
                let (columns, rows) = pane_console_size(bounds.width(), output_height);
                session.resize(columns, rows);
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
                    pane.buffer = format!("Could not prepare tile launch.\r\n\r\n{error}\r\n");
                    unsafe {
                        SetWindowTextW(pane.output_hwnd, wide(&pane.buffer).as_ptr());
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
            let (columns, rows) = pane_console_size(output_bounds.width(), output_bounds.height());

            match PaneSession::spawn(hwnd, pane_index, &command, columns, rows) {
                Ok(session) => {
                    pane.session = Some(session);
                    pane.buffer = format!(
                        "[launching {} in {}]\r\n",
                        pane.tile.title, command.working_directory
                    );
                    unsafe {
                        SetWindowTextW(pane.output_hwnd, wide(&pane.buffer).as_ptr());
                    }
                }
                Err(error) => {
                    pane.buffer = format!("Could not spawn tile process.\r\n\r\n{error}\r\n");
                    unsafe {
                        SetWindowTextW(pane.output_hwnd, wide(&pane.buffer).as_ptr());
                    }
                }
            }
        }
    }

    fn append_pane_output(state: &mut WorkspaceWindowState, pane_index: usize, chunk: &str) {
        let Some(pane) = state.panes.get_mut(pane_index) else {
            return;
        };

        pane.buffer.push_str(chunk);
        if pane.buffer.len() > MAX_BUFFER_CHARS {
            let trim = pane.buffer.len() - MAX_BUFFER_CHARS;
            pane.buffer.drain(..trim);
        }
        unsafe {
            SetWindowTextW(pane.output_hwnd, wide(&pane.buffer).as_ptr());
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

    fn pane_console_size(width: i32, height: i32) -> (i16, i16) {
        let columns = (width.max(120) / APPROX_CELL_WIDTH)
            .clamp(MIN_PANE_CHARS_X as i32, i16::MAX as i32) as i16;
        let rows = (height.max(120) / APPROX_CELL_HEIGHT)
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

                let text = sanitize_terminal_output(&buffer[..bytes_read as usize]);
                if text.is_empty() {
                    continue;
                }

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

    fn sanitize_terminal_output(bytes: &[u8]) -> String {
        let raw = String::from_utf8_lossy(bytes);
        let mut cleaned = String::with_capacity(raw.len());
        let mut characters = raw.chars().peekable();

        while let Some(character) = characters.next() {
            if character == '\u{1b}' {
                if matches!(characters.peek(), Some('[')) {
                    let _ = characters.next();
                    for candidate in characters.by_ref() {
                        if ('@'..='~').contains(&candidate) {
                            break;
                        }
                    }
                    continue;
                }
                continue;
            }

            if character == '\r' {
                if !matches!(characters.peek(), Some('\n')) {
                    cleaned.push('\n');
                }
            } else {
                cleaned.push(character);
            }
        }

        cleaned
    }

    unsafe fn state_mut(hwnd: HWND) -> Option<&'static mut WorkspaceWindowState> {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut WorkspaceWindowState;
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
}

#[cfg(target_os = "windows")]
pub use imp::open_saved_workspaces;
