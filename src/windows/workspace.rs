#[cfg(target_os = "windows")]
mod imp {
    use std::collections::BTreeMap;
    use std::ffi::c_void;
    use std::mem;
    use std::sync::mpsc;
    use std::ptr;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{
        COLORREF, CloseHandle, GlobalFree, HANDLE, HANDLE_FLAG_INHERIT, HINSTANCE, HWND, LPARAM,
        LRESULT, POINT, RECT, SIZE, SetHandleInformation, WPARAM,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_WINDOW, ClientToScreen,
        CreateFontW, CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_GUI_FONT, DeleteObject, EndPaint,
        FF_MODERN, FIXED_PITCH, FW_NORMAL, FillRect, GetDC, GetDeviceCaps, GetStockObject,
        GetTextExtentPoint32W, GetTextMetricsW, HBRUSH, HFONT, HGDIOBJ, InvalidateRect, LOGPIXELSY,
        MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, OUT_DEFAULT_PRECIS, PAINTSTRUCT,
        ReleaseDC, ScreenToClient, SelectObject, SetBkColor, SetTextColor, TEXTMETRICW, TextOutW,
        UpdateWindow,
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
        GetCapture, GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_DELETE,
        VK_DOWN, VK_END, VK_HOME, VK_INSERT, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT, VK_SHIFT, VK_UP,
    };
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, BN_DBLCLK, CREATESTRUCTW, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, CreatePopupMenu,
        CreateWindowExW, DefWindowProcW, DestroyMenu, EN_CHANGE, GWL_STYLE, GWLP_USERDATA,
        GetClientRect, GetCursorPos, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW,
        GetWindowTextW, HMENU, IDC_ARROW, IDC_HAND, LoadCursorW, MB_OK, MF_GRAYED, MF_STRING,
        MessageBoxW, PostMessageW, RegisterClassW, SB_BOTTOM, SB_LINEDOWN, SB_LINEUP, SB_PAGEDOWN,
        SB_PAGEUP, SB_THUMBPOSITION, SB_THUMBTRACK, SB_TOP, SB_VERT, SCROLLINFO, SIF_PAGE, SIF_POS,
        SIF_RANGE, SW_SHOW, SWP_FRAMECHANGED, SWP_NOZORDER, SendMessageW, SetCursor,
        SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, TPM_RETURNCMD,
        TPM_RIGHTBUTTON, TrackPopupMenu, WINDOW_EX_STYLE, WM_APP, WM_CHAR, WM_COMMAND, WM_CREATE,
        WM_DESTROY, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
        WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_RBUTTONUP,
        WM_SETCURSOR, WM_SETFOCUS, WM_SETFONT, WM_SIZE, WM_VSCROLL, WNDCLASSW, WS_BORDER, WS_CHILD,
        WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
    };
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        CreateCoreWebView2EnvironmentWithOptions, COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC,
        ICoreWebView2, ICoreWebView2Controller, ICoreWebView2CreateCoreWebView2ControllerCompletedHandler,
        ICoreWebView2Environment, ICoreWebView2NewWindowRequestedEventArgs, ICoreWebView2_11,
    };
    use webview2_com::{
        CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
        ContextMenuRequestedEventHandler, DocumentTitleChangedEventHandler,
        NavigationCompletedEventHandler, NewWindowRequestedEventHandler, take_pwstr, wait_with_pump,
    };
    use windows::Win32::Foundation::{E_POINTER, E_UNEXPECTED, HWND as Win32Hwnd, RECT as WinRect};
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows::Win32::System::WinRT::EventRegistrationToken;
    use windows::core::{Error as WindowsError, HSTRING, Interface, PCWSTR, PWSTR};

    use crate::logging;
    use crate::model::assets::WorkspaceAssets;
    use crate::model::layout::{LayoutNode, SplitAxis, TileKind, TileSpec};
    use crate::model::preset::ApplicationDensity;
    use crate::services::alerts::{AlertEventInput, AlertSeverity, AlertSourceKind, AlertStore};
    use crate::services::broadcast::{BroadcastTarget, saved_groups_for_tiles};
    use crate::services::layout_editor::split_tile_with_kind;
    use crate::services::launch_resolution::resolve_tile_launch;
    use crate::services::output_helpers::{helper_summary_text, scan_output};
    use crate::services::runbooks::resolve_runbook;
    use crate::storage::asset_store::AssetStore;
    use crate::storage::preference_store::PreferenceStore;
    use crate::storage::session_store::{SavedSession, SavedTab, SessionStore};
    use crate::transcript::TranscriptBuffer;
    use crate::windows::vt::{
        MouseTrackingMode, ShellIntegrationPhase, VtBuffer, VtColor, VtPosition, VtStyle,
    };
    use crate::windows::wsl::{self, WindowsLaunchCommand, WindowsRuntime};
    use crate::windows::{
        alert_center, assets_manager, command_palette, runbook_dialog, shortcut_capture,
        transcript_viewer,
    };

    const WINDOW_CLASS: &str = "TerminalTilerWindowsWorkspace";
    const PANE_CLASS: &str = "TerminalTilerWindowsPane";
    const PANE_HEADER_CLASS: &str = "TerminalTilerWindowsPaneHeader";
    const TAB_BUTTON_CLASS: &str = "TerminalTilerWindowsTabButton";
    const WM_PANE_OUTPUT: u32 = WM_APP + 1;
    const WM_PANE_EXIT: u32 = WM_APP + 2;
    const WM_RECONNECT_PANE: u32 = WM_APP + 3;
    const WM_WEBVIEW_URI_CHANGED: u32 = WM_APP + 4;
    const WM_WEBVIEW_TITLE_CHANGED: u32 = WM_APP + 5;
    const WM_WEBVIEW_AUTO_REFRESH: u32 = WM_APP + 6;
    const HEADER_HEIGHT: i32 = 152;
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
    const MENU_OPEN_LINK: usize = 3;
    const MENU_COPY_LINK: usize = 4;
    const MENU_RECONNECT: usize = 5;
    const MENU_SHOW_TRANSCRIPT: usize = 6;
    const MENU_WEB_RELOAD: usize = 21;
    const MENU_WEB_OPEN_EXTERNAL: usize = 22;
    const MENU_WEB_COPY_URL: usize = 23;
    const ID_WORKSPACE_TITLE: isize = 1001;
    const ID_WORKSPACE_ZOOM_OUT: isize = 1002;
    const ID_WORKSPACE_ZOOM_IN: isize = 1003;
    const ID_WORKSPACE_DENSITY: isize = 1004;
    const ID_WORKSPACE_FULLSCREEN: isize = 1005;
    const ID_WORKSPACE_MOVE_LEFT: isize = 1006;
    const ID_WORKSPACE_MOVE_RIGHT: isize = 1007;
    const ID_WORKSPACE_CLOSE_TAB: isize = 1008;
    const ID_WORKSPACE_SHOW_LAUNCHER: isize = 1009;
    const ID_WORKSPACE_BROADCAST_TARGET: isize = 1010;
    const ID_WORKSPACE_BROADCAST_ENTRY: isize = 1011;
    const ID_WORKSPACE_BROADCAST_SEND: isize = 1012;
    const ID_WORKSPACE_RUNBOOK: isize = 1013;
    const ID_WORKSPACE_ALERTS: isize = 1014;
    const ID_WORKSPACE_COMMAND_PALETTE: isize = 1015;
    const ID_WORKSPACE_URL: isize = 1016;
    const ID_WORKSPACE_URL_RELOAD: isize = 1017;
    const ID_WORKSPACE_ADD_WEB: isize = 1018;
    const ID_TAB_BUTTON_BASE: isize = 3000;
    const HEADER_BUTTON_WIDTH: i32 = 90;
    const HEADER_BUTTON_HEIGHT: i32 = 28;
    const EM_SETSEL_MESSAGE: u32 = 0x00B1;
    const SS_NOTIFY_STYLE: u32 = 0x0000_0100;
    const AUTO_RECONNECT_DELAYS_SECONDS: [u64; 3] = [1, 3, 10];
    static NEXT_WINDOW_ID: AtomicUsize = AtomicUsize::new(1);
    static NEXT_PANE_ID: AtomicUsize = AtomicUsize::new(1);
    static SESSION_REGISTRY: OnceLock<Mutex<WorkspaceSessionRegistry>> = OnceLock::new();

    struct TabDragState {
        dragged_index: usize,
        target_index: usize,
        insert_after: bool,
    }

    struct PaneDragState {
        dragged_pane_id: usize,
        target_pane_id: usize,
    }

    struct WorkspaceWindowState {
        window_id: usize,
        session_store: SessionStore,
        preference_store: PreferenceStore,
        tabs: Vec<SavedTab>,
        active_tab_index: usize,
        runtime: WindowsRuntime,
        asset_store: AssetStore,
        assets: WorkspaceAssets,
        asset_warning: Option<String>,
        alert_store: AlertStore,
        broadcast_target: BroadcastTarget,
        suppress_title_events: bool,
        title_hwnd: HWND,
        path_hwnd: HWND,
        url_hwnd: HWND,
        url_reload_hwnd: HWND,
        zoom_out_hwnd: HWND,
        zoom_in_hwnd: HWND,
        density_hwnd: HWND,
        fullscreen_hwnd: HWND,
        move_left_hwnd: HWND,
        move_right_hwnd: HWND,
        close_tab_hwnd: HWND,
        show_launcher_hwnd: HWND,
        broadcast_target_hwnd: HWND,
        broadcast_entry_hwnd: HWND,
        broadcast_send_hwnd: HWND,
        add_web_hwnd: HWND,
        runbook_hwnd: HWND,
        alerts_hwnd: HWND,
        command_palette_hwnd: HWND,
        tab_button_hwnds: Vec<HWND>,
        tab_drag: Option<TabDragState>,
        pane_drag: Option<PaneDragState>,
        is_fullscreen: bool,
        saved_window_rect: RECT,
        saved_window_style: isize,
        focused_web_pane_id: Option<usize>,
        webview_environment: Option<ICoreWebView2Environment>,
        panes: Vec<Box<PaneState>>,
    }

    #[derive(Default)]
    struct WorkspaceSessionRegistry {
        windows: BTreeMap<usize, SavedSession>,
        active_window_id: Option<usize>,
    }

    struct TabButtonState {
        parent_hwnd: HWND,
        index: usize,
        active: bool,
        press_origin: Option<POINT>,
    }

    struct PaneState {
        id: usize,
        parent_hwnd: HWND,
        tile: TileSpec,
        title_hwnd: HWND,
        output_hwnd: HWND,
        terminal: VtBuffer,
        webview_controller: Option<ICoreWebView2Controller>,
        webview: Option<ICoreWebView2>,
        webview_uri: Option<String>,
        webview_title: Option<String>,
        auto_refresh_stop: Option<Arc<std::sync::atomic::AtomicBool>>,
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
        header_press_origin: Option<POINT>,
        transcript: TranscriptBuffer,
        last_helper_signature: String,
        reconnect_attempts: u8,
        termination_requested: bool,
        session: Option<PaneSession>,
    }

    impl Drop for PaneState {
        fn drop(&mut self) {
            if let Some(stop) = self.auto_refresh_stop.take() {
                stop.store(true, Ordering::Relaxed);
            }
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
        pane_id: usize,
        text: String,
    }

    struct WebViewStringEvent {
        pane_id: usize,
        value: String,
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
            pane_id: usize,
            command: &WindowsLaunchCommand,
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

            spawn_output_reader(window_hwnd, pane_id, output_read);

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
        runtime: &WindowsRuntime,
    ) -> Result<(usize, usize), String> {
        if session.tabs.is_empty() {
            return Ok((0, 0));
        }

        let pane_count = session
            .tabs
            .iter()
            .map(|tab| tab.preset.layout.tile_specs().len())
            .sum::<usize>();
        open_workspace_window(session.tabs.clone(), session.active_tab_index, runtime)?;
        Ok((1, pane_count))
    }

    fn open_workspace_window(
        tabs: Vec<SavedTab>,
        active_tab_index: usize,
        runtime: &WindowsRuntime,
    ) -> Result<(), String> {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err("could not resolve module handle for workspace window".into());
        }

        register_window_classes(instance)?;
        let active_tab_index = active_tab_index.min(tabs.len().saturating_sub(1));
        let window_title = tabs
            .get(active_tab_index)
            .and_then(|tab| {
                tab.custom_title
                    .clone()
                    .or_else(|| Some(tab.preset.name.clone()))
            })
            .unwrap_or_else(|| "TerminalTiler Workspace".to_string());
        let state = Box::new(WorkspaceWindowState {
            window_id: NEXT_WINDOW_ID.fetch_add(1, Ordering::Relaxed),
            session_store: SessionStore::new(),
            preference_store: PreferenceStore::new(),
            tabs,
            active_tab_index,
            runtime: runtime.clone(),
            asset_store: AssetStore::new(),
            assets: WorkspaceAssets::default(),
            asset_warning: None,
            alert_store: AlertStore::default(),
            broadcast_target: BroadcastTarget::Off,
            suppress_title_events: false,
            title_hwnd: ptr::null_mut(),
            path_hwnd: ptr::null_mut(),
            url_hwnd: ptr::null_mut(),
            url_reload_hwnd: ptr::null_mut(),
            zoom_out_hwnd: ptr::null_mut(),
            zoom_in_hwnd: ptr::null_mut(),
            density_hwnd: ptr::null_mut(),
            fullscreen_hwnd: ptr::null_mut(),
            move_left_hwnd: ptr::null_mut(),
            move_right_hwnd: ptr::null_mut(),
            close_tab_hwnd: ptr::null_mut(),
            show_launcher_hwnd: ptr::null_mut(),
            broadcast_target_hwnd: ptr::null_mut(),
            broadcast_entry_hwnd: ptr::null_mut(),
            broadcast_send_hwnd: ptr::null_mut(),
            add_web_hwnd: ptr::null_mut(),
            runbook_hwnd: ptr::null_mut(),
            alerts_hwnd: ptr::null_mut(),
            command_palette_hwnd: ptr::null_mut(),
            tab_button_hwnds: Vec::new(),
            tab_drag: None,
            pane_drag: None,
            is_fullscreen: false,
            saved_window_rect: unsafe { mem::zeroed() },
            saved_window_style: 0,
            focused_web_pane_id: None,
            webview_environment: None,
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

    fn ensure_webview_com_initialized() -> Result<(), String> {
        static WEBVIEW_COM_INIT: OnceLock<Result<(), String>> = OnceLock::new();

        WEBVIEW_COM_INIT
            .get_or_init(|| {
                unsafe {
                    CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                        .ok()
                        .map_err(|error| format!("CoInitializeEx failed for WebView2: {error}"))
                }
            })
            .clone()
    }

    fn create_webview_environment() -> Result<ICoreWebView2Environment, String> {
        ensure_webview_com_initialized()?;

        let (tx, rx) = mpsc::channel();
        unsafe {
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                PCWSTR::null(),
                None::<&webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2EnvironmentOptions>,
                &CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
                    move |error_code, environment| {
                        error_code?;
                        tx.send(environment.ok_or_else(|| WindowsError::from(E_POINTER)))
                            .map_err(|_| WindowsError::from(E_UNEXPECTED))
                    },
                )),
            )
            .map_err(|error| format!("CreateCoreWebView2EnvironmentWithOptions failed: {error}"))?;
        }

        wait_with_pump(rx)
            .map_err(|error| format!("Waiting for WebView2 environment failed: {error}"))?
            .map_err(|error| format!("Creating WebView2 environment failed: {error}"))
    }

    fn ensure_webview_environment(
        state: &mut WorkspaceWindowState,
    ) -> Result<ICoreWebView2Environment, String> {
        if let Some(environment) = state.webview_environment.as_ref() {
            return Ok(environment.clone());
        }

        let environment = create_webview_environment()?;
        state.webview_environment = Some(environment.clone());
        Ok(environment)
    }

    fn create_webview_controller(
        parent_hwnd: HWND,
        environment: &ICoreWebView2Environment,
    ) -> Result<ICoreWebView2Controller, String> {
        let (tx, rx) = mpsc::channel();
        let handler: ICoreWebView2CreateCoreWebView2ControllerCompletedHandler =
            CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
                move |error_code, controller| {
                    error_code?;
                    tx.send(controller.ok_or_else(|| WindowsError::from(E_POINTER)))
                        .map_err(|_| WindowsError::from(E_UNEXPECTED))
                },
            ));

        unsafe {
            environment
                .CreateCoreWebView2Controller(Win32Hwnd(parent_hwnd as _), &handler)
                .map_err(|error| format!("CreateCoreWebView2Controller failed: {error}"))?;
        }

        wait_with_pump(rx)
            .map_err(|error| format!("Waiting for WebView2 controller failed: {error}"))?
            .map_err(|error| format!("Creating WebView2 controller failed: {error}"))
    }

    fn register_window_classes(instance: HINSTANCE) -> Result<(), String> {
        register_class(instance, WINDOW_CLASS, window_proc)?;
        register_class(instance, PANE_CLASS, pane_window_proc)?;
        register_class(instance, PANE_HEADER_CLASS, pane_header_window_proc)?;
        register_class(instance, TAB_BUTTON_CLASS, tab_button_window_proc)
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
                }
                0
            }
            WM_SETFOCUS => {
                if let Some(state) = unsafe { window_state_mut(hwnd) } {
                    save_workspace_session_state(state);
                }
                0
            }
            WM_SIZE => {
                if let Some(state) = unsafe { window_state_mut(hwnd) } {
                    layout_controls(hwnd, state);
                }
                0
            }
            WM_KEYDOWN => {
                if let Some(state) = unsafe { window_state_mut(hwnd) }
                    && handle_workspace_shortcuts(hwnd, state, wparam as u32)
                {
                    return 0;
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            WM_COMMAND => {
                let command_id = (wparam & 0xffff) as isize;
                if let Some(state) = unsafe { window_state_mut(hwnd) } {
                    match command_id {
                        ID_WORKSPACE_TITLE if ((wparam >> 16) & 0xffff) as u32 == EN_CHANGE => {
                            sync_workspace_title(hwnd, state);
                        }
                        ID_WORKSPACE_URL_RELOAD => activate_web_navigation_control(state),
                        ID_WORKSPACE_ZOOM_OUT => adjust_terminal_zoom(hwnd, state, -1),
                        ID_WORKSPACE_ZOOM_IN => adjust_terminal_zoom(hwnd, state, 1),
                        ID_WORKSPACE_DENSITY => cycle_workspace_density(hwnd, state),
                        ID_WORKSPACE_FULLSCREEN => toggle_workspace_fullscreen(hwnd, state),
                        ID_WORKSPACE_MOVE_LEFT => move_active_tab(hwnd, state, -1),
                        ID_WORKSPACE_MOVE_RIGHT => move_active_tab(hwnd, state, 1),
                        ID_WORKSPACE_CLOSE_TAB => close_active_tab(hwnd, state),
                        ID_WORKSPACE_SHOW_LAUNCHER => {
                            let _ = crate::windows::app::show_primary_shell_window();
                        }
                        ID_WORKSPACE_BROADCAST_TARGET => cycle_broadcast_target(state),
                        ID_WORKSPACE_BROADCAST_SEND => send_broadcast_command(state),
                        ID_WORKSPACE_ADD_WEB => add_web_tile_to_active_workspace(hwnd, state),
                        ID_WORKSPACE_RUNBOOK => open_runbook_palette(hwnd, state),
                        ID_WORKSPACE_ALERTS => open_alert_center(hwnd, state),
                        ID_WORKSPACE_COMMAND_PALETTE => open_workspace_command_palette(hwnd, state),
                        id if id >= ID_TAB_BUTTON_BASE => {
                            let index = (id - ID_TAB_BUTTON_BASE) as usize;
                            let notification = ((wparam >> 16) & 0xffff) as u32;
                            if notification == BN_DBLCLK {
                                begin_tab_rename(hwnd, state, index);
                            } else {
                                switch_active_tab(hwnd, state, index);
                            }
                        }
                        _ => {}
                    }
                }
                0
            }
            WM_WEBVIEW_URI_CHANGED => {
                let event_ptr = lparam as *mut WebViewStringEvent;
                if !event_ptr.is_null() {
                    let event = unsafe { Box::from_raw(event_ptr) };
                    if let Some(state) = unsafe { window_state_mut(hwnd) } {
                        apply_webview_uri_update(state, event.pane_id, &event.value);
                    }
                }
                0
            }
            WM_WEBVIEW_TITLE_CHANGED => {
                let event_ptr = lparam as *mut WebViewStringEvent;
                if !event_ptr.is_null() {
                    let event = unsafe { Box::from_raw(event_ptr) };
                    if let Some(state) = unsafe { window_state_mut(hwnd) } {
                        apply_webview_title_update(state, event.pane_id, &event.value);
                    }
                }
                0
            }
            WM_WEBVIEW_AUTO_REFRESH => {
                if let Some(state) = unsafe { window_state_mut(hwnd) } {
                    let _ = reload_web_pane_by_id(state, wparam as usize);
                }
                0
            }
            WM_PANE_OUTPUT => {
                let event_ptr = lparam as *mut PaneOutputEvent;
                if !event_ptr.is_null() {
                    let event = unsafe { Box::from_raw(event_ptr) };
                    if let Some(state) = unsafe { window_state_mut(hwnd) } {
                        append_pane_output(state, event.pane_id, &event.text);
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
            WM_RECONNECT_PANE => {
                if let Some(state) = unsafe { window_state_mut(hwnd) } {
                    let pane_id = wparam as usize;
                    let expected_attempt = lparam as u8;
                    let _ = reconnect_pane(state, pane_id, Some(expected_attempt));
                }
                0
            }
            WM_DESTROY => {
                if let Some(state) = unsafe { window_state_mut(hwnd) } {
                    remove_workspace_session_state(state.window_id, &state.session_store);
                }
                0
            }
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
        let is_web_pane = unsafe {
            pane_state_mut(hwnd)
                .map(|pane| pane.tile.tile_kind == TileKind::WebView)
                .unwrap_or(false)
        };
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
                if is_web_pane {
                    let mut paint = PAINTSTRUCT::default();
                    unsafe {
                        BeginPaint(hwnd, &mut paint);
                        EndPaint(hwnd, &paint);
                    }
                    return 0;
                }
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    render_pane(hwnd, pane);
                    return 0;
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            WM_SETFOCUS => {
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    if let Some(state) = unsafe { window_state_mut(pane.parent_hwnd) } {
                        set_focused_web_pane(
                            state,
                            (pane.tile.tile_kind == TileKind::WebView).then_some(pane.id),
                        );
                    }
                    if pane.tile.tile_kind == TileKind::WebView {
                        if let Some(controller) = pane.webview_controller.as_ref() {
                            let _ = unsafe {
                                controller.MoveFocus(
                                    COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC,
                                )
                            };
                        }
                        return 0;
                    }
                    pane.focused = true;
                    if pane.terminal.focus_reporting()
                        && let Some(session) = pane.session.as_ref()
                        && let Err(error) = session.write_input(b"\x1b[I")
                    {
                        logging::error(format!("pane focus-in report failed: {error}"));
                    }
                    unsafe {
                        InvalidateRect(hwnd, ptr::null(), 1);
                    }
                }
                0
            }
            WM_KILLFOCUS => {
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    if pane.tile.tile_kind == TileKind::WebView {
                        return 0;
                    }
                    pane.focused = false;
                    if pane.terminal.focus_reporting()
                        && let Some(session) = pane.session.as_ref()
                        && let Err(error) = session.write_input(b"\x1b[O")
                    {
                        logging::error(format!("pane focus-out report failed: {error}"));
                    }
                    unsafe {
                        InvalidateRect(hwnd, ptr::null(), 1);
                    }
                }
                0
            }
            WM_LBUTTONDOWN => {
                if is_web_pane {
                    unsafe { SetFocus(hwnd) };
                    return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
                }
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    unsafe { SetFocus(hwnd) };
                    if forward_mouse_event(pane, lparam, MouseEvent::ButtonPress(0)) {
                        pane.pressed_mouse_button = Some(0);
                        return 0;
                    }
                    if pane.terminal.mouse_tracking() == MouseTrackingMode::Disabled
                        && is_modifier_pressed(VK_CONTROL)
                        && let Some(link) = hyperlink_at_lparam(pane, lparam)
                    {
                        let _ = open_url(link);
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
                if is_web_pane {
                    return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
                }
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
                if is_web_pane {
                    return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
                }
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
            WM_SETCURSOR => {
                if is_web_pane {
                    return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
                }
                if let Some(pane) = unsafe { pane_state_mut(hwnd) }
                    && pane.terminal.mouse_tracking() == MouseTrackingMode::Disabled
                    && is_modifier_pressed(VK_CONTROL)
                    && hyperlink_under_pointer(hwnd, pane).is_some()
                {
                    unsafe {
                        SetCursor(LoadCursorW(ptr::null_mut(), IDC_HAND));
                    }
                    return 1;
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            WM_MOUSEWHEEL => {
                if is_web_pane {
                    return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
                }
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
                if is_web_pane {
                    return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
                }
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
                if is_web_pane {
                    return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
                }
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    unsafe { SetFocus(hwnd) };
                    show_pane_context_menu(hwnd, pane, lparam);
                }
                0
            }
            WM_CHAR => {
                if is_web_pane {
                    return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
                }
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    handle_char_input(pane, wparam as u32);
                }
                0
            }
            WM_KEYDOWN => {
                let parent_hwnd = if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    pane.parent_hwnd
                } else {
                    return 0;
                };
                if let Some(state) = unsafe { window_state_mut(parent_hwnd) }
                    && handle_workspace_shortcuts(parent_hwnd, state, wparam as u32)
                {
                    return 0;
                }
                if is_web_pane {
                    return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
                }
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    handle_key_input(pane, wparam as u16);
                }
                0
            }
            WM_VSCROLL => {
                if is_web_pane {
                    return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
                }
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

    unsafe extern "system" fn pane_header_window_proc(
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
                    render_pane_header(hwnd, pane);
                    return 0;
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            WM_LBUTTONDOWN => {
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    pane.header_press_origin = Some(screen_point_from_lparam(hwnd, lparam));
                    unsafe {
                        SetCapture(hwnd);
                    }
                }
                0
            }
            WM_MOUSEMOVE => {
                if unsafe { GetCapture() } == hwnd
                    && let Some(pane) = unsafe { pane_state_mut(hwnd) }
                    && let Some(origin) = pane.header_press_origin
                {
                    let point = screen_point_from_lparam(hwnd, lparam);
                    if drag_threshold_exceeded(origin, point)
                        && let Some(state) = unsafe { window_state_mut(pane.parent_hwnd) }
                    {
                        update_pane_drag(state, pane.id, point);
                    }
                }
                0
            }
            WM_LBUTTONUP => {
                if unsafe { GetCapture() } == hwnd {
                    unsafe {
                        ReleaseCapture();
                    }
                }
                if let Some(pane) = unsafe { pane_state_mut(hwnd) } {
                    pane.header_press_origin = None;
                    if let Some(state) = unsafe { window_state_mut(pane.parent_hwnd) } {
                        finish_pane_drag(pane.parent_hwnd, state, pane.id);
                    }
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

    unsafe extern "system" fn tab_button_window_proc(
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

                let state_ptr = unsafe { (*create).lpCreateParams as *mut TabButtonState };
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
                }
                1
            }
            WM_PAINT => {
                if let Some(button) = unsafe { tab_button_state_mut(hwnd) } {
                    render_tab_button(hwnd, button);
                    return 0;
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            WM_LBUTTONDOWN => {
                if let Some(button) = unsafe { tab_button_state_mut(hwnd) } {
                    button.press_origin = Some(screen_point_from_lparam(hwnd, lparam));
                    unsafe {
                        SetCapture(hwnd);
                    }
                }
                0
            }
            WM_MOUSEMOVE => {
                if unsafe { GetCapture() } == hwnd
                    && let Some(button) = unsafe { tab_button_state_mut(hwnd) }
                    && let Some(origin) = button.press_origin
                {
                    let point = screen_point_from_lparam(hwnd, lparam);
                    if drag_threshold_exceeded(origin, point)
                        && let Some(state) = unsafe { window_state_mut(button.parent_hwnd) }
                    {
                        update_tab_drag(state, button.index, point);
                    }
                }
                0
            }
            WM_LBUTTONUP => {
                if unsafe { GetCapture() } == hwnd {
                    unsafe {
                        ReleaseCapture();
                    }
                }
                if let Some(button) = unsafe { tab_button_state_mut(hwnd) } {
                    button.press_origin = None;
                    if let Some(state) = unsafe { window_state_mut(button.parent_hwnd) } {
                        finish_tab_drag(button.parent_hwnd, state, button.index);
                    }
                }
                0
            }
            WM_LBUTTONDBLCLK => {
                if let Some(button) = unsafe { tab_button_state_mut(hwnd) }
                    && let Some(state) = unsafe { window_state_mut(button.parent_hwnd) }
                {
                    begin_tab_rename(button.parent_hwnd, state, button.index);
                }
                0
            }
            WM_NCDESTROY => {
                let state_ptr =
                    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) } as *mut TabButtonState;
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

    fn create_controls(hwnd: HWND, state: &mut WorkspaceWindowState) {
        let title = active_tab(state)
            .custom_title
            .clone()
            .unwrap_or_else(|| active_tab(state).preset.name.clone());
        state.title_hwnd = create_child_window(
            hwnd,
            "EDIT",
            &title,
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP,
            0,
            ID_WORKSPACE_TITLE,
            ptr::null_mut(),
        );
        state.path_hwnd = create_child_window(
            hwnd,
            "STATIC",
            &active_tab(state).workspace_root.display().to_string(),
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            ptr::null_mut(),
        );
        state.url_hwnd = create_child_window(
            hwnd,
            "EDIT",
            "",
            WS_CHILD | WS_BORDER | WS_TABSTOP,
            0,
            ID_WORKSPACE_URL,
            ptr::null_mut(),
        );
        state.url_reload_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Reload",
            WS_CHILD | WS_TABSTOP,
            0,
            ID_WORKSPACE_URL_RELOAD,
            ptr::null_mut(),
        );
        state.zoom_out_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Zoom -",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            ID_WORKSPACE_ZOOM_OUT,
            ptr::null_mut(),
        );
        state.zoom_in_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Zoom +",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            ID_WORKSPACE_ZOOM_IN,
            ptr::null_mut(),
        );
        state.density_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            density_button_label(active_tab(state).preset.density),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            ID_WORKSPACE_DENSITY,
            ptr::null_mut(),
        );
        state.fullscreen_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Fullscreen",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            ID_WORKSPACE_FULLSCREEN,
            ptr::null_mut(),
        );
        state.move_left_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Move Left",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            ID_WORKSPACE_MOVE_LEFT,
            ptr::null_mut(),
        );
        state.move_right_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Move Right",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            ID_WORKSPACE_MOVE_RIGHT,
            ptr::null_mut(),
        );
        state.close_tab_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Close Tab",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            ID_WORKSPACE_CLOSE_TAB,
            ptr::null_mut(),
        );
        state.show_launcher_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Show Launcher",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            ID_WORKSPACE_SHOW_LAUNCHER,
            ptr::null_mut(),
        );
        state.broadcast_target_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Broadcast Off",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            ID_WORKSPACE_BROADCAST_TARGET,
            ptr::null_mut(),
        );
        state.broadcast_entry_hwnd = create_child_window(
            hwnd,
            "EDIT",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP,
            0,
            ID_WORKSPACE_BROADCAST_ENTRY,
            ptr::null_mut(),
        );
        state.broadcast_send_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Send",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            ID_WORKSPACE_BROADCAST_SEND,
            ptr::null_mut(),
        );
        state.add_web_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Add Web",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            ID_WORKSPACE_ADD_WEB,
            ptr::null_mut(),
        );
        state.runbook_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Runbook",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            ID_WORKSPACE_RUNBOOK,
            ptr::null_mut(),
        );
        state.alerts_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Alerts (0)",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            ID_WORKSPACE_ALERTS,
            ptr::null_mut(),
        );
        state.command_palette_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Palette",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            ID_WORKSPACE_COMMAND_PALETTE,
            ptr::null_mut(),
        );

        let ui_font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
        for control in [
            state.title_hwnd,
            state.path_hwnd,
            state.url_hwnd,
            state.url_reload_hwnd,
            state.zoom_out_hwnd,
            state.zoom_in_hwnd,
            state.density_hwnd,
            state.fullscreen_hwnd,
            state.move_left_hwnd,
            state.move_right_hwnd,
            state.close_tab_hwnd,
            state.show_launcher_hwnd,
            state.broadcast_target_hwnd,
            state.broadcast_entry_hwnd,
            state.broadcast_send_hwnd,
            state.add_web_hwnd,
            state.runbook_hwnd,
            state.alerts_hwnd,
            state.command_palette_hwnd,
        ] {
            if !control.is_null() {
                unsafe {
                    SendMessageW(control, WM_SETFONT, ui_font as usize, 1);
                }
            }
        }

        rebuild_tab_buttons(hwnd, state);
        rebuild_active_tab_content(hwnd, state);
    }

    fn layout_controls(hwnd: HWND, state: &mut WorkspaceWindowState) {
        let bounds = match client_bounds(hwnd) {
            Some(bounds) => bounds,
            None => return,
        };

        let button_gap = 8;
        let buttons_width = (HEADER_BUTTON_WIDTH * 4) + (button_gap * 3);
        let title_width = (bounds.width() - (OUTER_MARGIN * 2) - buttons_width - 12).max(240);
        let tab_action_width = HEADER_BUTTON_WIDTH * 4 + (button_gap * 3);
        let show_web_controls = active_tab_has_web_tiles(state);
        unsafe {
            SetWindowPos(
                state.title_hwnd,
                ptr::null_mut(),
                OUTER_MARGIN,
                OUTER_MARGIN,
                title_width,
                26,
                SWP_NOZORDER,
            );
            let button_left = OUTER_MARGIN + title_width + 12;
            if show_web_controls {
                SetWindowPos(
                    state.url_hwnd,
                    ptr::null_mut(),
                    OUTER_MARGIN,
                    OUTER_MARGIN + 32,
                    (title_width - 84).max(120),
                    24,
                    SWP_NOZORDER,
                );
                SetWindowPos(
                    state.url_reload_hwnd,
                    ptr::null_mut(),
                    OUTER_MARGIN + (title_width - 76).max(128),
                    OUTER_MARGIN + 30,
                    76,
                    HEADER_BUTTON_HEIGHT,
                    SWP_NOZORDER,
                );
            } else {
                SetWindowPos(
                    state.path_hwnd,
                    ptr::null_mut(),
                    OUTER_MARGIN,
                    OUTER_MARGIN + 32,
                    title_width,
                    18,
                    SWP_NOZORDER,
                );
            }
            SetWindowPos(
                state.zoom_out_hwnd,
                ptr::null_mut(),
                button_left,
                OUTER_MARGIN,
                HEADER_BUTTON_WIDTH,
                HEADER_BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.zoom_in_hwnd,
                ptr::null_mut(),
                button_left + HEADER_BUTTON_WIDTH + button_gap,
                OUTER_MARGIN,
                HEADER_BUTTON_WIDTH,
                HEADER_BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.density_hwnd,
                ptr::null_mut(),
                button_left,
                OUTER_MARGIN + HEADER_BUTTON_HEIGHT + 6,
                HEADER_BUTTON_WIDTH * 2 + button_gap,
                HEADER_BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.fullscreen_hwnd,
                ptr::null_mut(),
                button_left + (HEADER_BUTTON_WIDTH * 2) + (button_gap * 2),
                OUTER_MARGIN + HEADER_BUTTON_HEIGHT + 6,
                HEADER_BUTTON_WIDTH * 2 - button_gap,
                HEADER_BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            let tab_button_width = 148;
            let tab_row_y = OUTER_MARGIN + HEADER_BUTTON_HEIGHT + 40;
            let max_tab_button_width =
                (bounds.width() - (OUTER_MARGIN * 2) - tab_action_width - 12).max(tab_button_width);
            for (index, tab_button) in state.tab_button_hwnds.iter().enumerate() {
                let left = OUTER_MARGIN + (index as i32 * (tab_button_width + 8));
                let width = (max_tab_button_width - (index as i32 * (tab_button_width + 8)))
                    .min(tab_button_width)
                    .max(96);
                SetWindowPos(
                    *tab_button,
                    ptr::null_mut(),
                    left,
                    tab_row_y,
                    width,
                    HEADER_BUTTON_HEIGHT,
                    SWP_NOZORDER,
                );
            }
            let tab_action_left = bounds.right - OUTER_MARGIN - tab_action_width;
            SetWindowPos(
                state.move_left_hwnd,
                ptr::null_mut(),
                tab_action_left,
                tab_row_y,
                HEADER_BUTTON_WIDTH,
                HEADER_BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.move_right_hwnd,
                ptr::null_mut(),
                tab_action_left + HEADER_BUTTON_WIDTH + button_gap,
                tab_row_y,
                HEADER_BUTTON_WIDTH,
                HEADER_BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.close_tab_hwnd,
                ptr::null_mut(),
                tab_action_left + (HEADER_BUTTON_WIDTH + button_gap) * 2,
                tab_row_y,
                HEADER_BUTTON_WIDTH,
                HEADER_BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.show_launcher_hwnd,
                ptr::null_mut(),
                tab_action_left + (HEADER_BUTTON_WIDTH + button_gap) * 3,
                tab_row_y,
                HEADER_BUTTON_WIDTH,
                HEADER_BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            let controls_y = tab_row_y + HEADER_BUTTON_HEIGHT + 10;
            SetWindowPos(
                state.broadcast_target_hwnd,
                ptr::null_mut(),
                OUTER_MARGIN,
                controls_y,
                140,
                HEADER_BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.broadcast_entry_hwnd,
                ptr::null_mut(),
                OUTER_MARGIN + 148,
                controls_y,
                (bounds.width() - OUTER_MARGIN * 2 - 148 - 88 * 5 - 32).max(180),
                HEADER_BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            let right_controls_left = bounds.right - OUTER_MARGIN - (88 * 5) - (button_gap * 4);
            SetWindowPos(
                state.broadcast_send_hwnd,
                ptr::null_mut(),
                right_controls_left,
                controls_y,
                88,
                HEADER_BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.add_web_hwnd,
                ptr::null_mut(),
                right_controls_left + 88 + button_gap,
                controls_y,
                88,
                HEADER_BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.runbook_hwnd,
                ptr::null_mut(),
                right_controls_left + (88 + button_gap) * 2,
                controls_y,
                88,
                HEADER_BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.alerts_hwnd,
                ptr::null_mut(),
                right_controls_left + (88 + button_gap) * 3,
                controls_y,
                88,
                HEADER_BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.command_palette_hwnd,
                ptr::null_mut(),
                right_controls_left + (88 + button_gap) * 4,
                controls_y,
                88,
                HEADER_BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            ShowWindow(state.path_hwnd, if show_web_controls { 0 } else { SW_SHOW });
            ShowWindow(state.url_hwnd, if show_web_controls { SW_SHOW } else { 0 });
            ShowWindow(state.url_reload_hwnd, if show_web_controls { SW_SHOW } else { 0 });
        }

        let layout_bounds = Bounds {
            left: OUTER_MARGIN,
            top: OUTER_MARGIN + HEADER_HEIGHT,
            right: bounds.right - OUTER_MARGIN,
            bottom: bounds.bottom - OUTER_MARGIN,
        };
        let mut pane_bounds = Vec::with_capacity(state.panes.len());
        collect_tile_bounds(
            &active_tab(state).preset.layout,
            layout_bounds,
            &mut pane_bounds,
        );
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

            if pane.tile.tile_kind == TileKind::WebView {
                if let Some(controller) = pane.webview_controller.as_ref() {
                    let _ = unsafe {
                        controller.SetBounds(WinRect {
                            left: 0,
                            top: 0,
                            right: bounds.width().max(120),
                            bottom: output_height,
                        })
                    };
                }
            } else {
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

        sync_web_navigation_controls(state);
    }

    fn spawn_pane_sessions(hwnd: HWND, state: &mut WorkspaceWindowState) {
        let workspace_root = active_tab(state).workspace_root.clone();
        let asset_store = AssetStore::new();
        let asset_outcome = asset_store.load_assets_for_workspace_root(&workspace_root);
        if let Some(warning) = asset_outcome.warning.as_deref() {
            logging::error(format!(
                "workspace asset warning for '{}': {}",
                workspace_root.display(),
                warning
            ));
        }
        for pane in state.panes.iter_mut() {
            if pane.tile.tile_kind == TileKind::WebView {
                continue;
            }
            let resolved_launch =
                match resolve_tile_launch(&pane.tile, &workspace_root, &asset_outcome.assets) {
                    Ok(resolved_launch) => resolved_launch,
                    Err(error) => {
                        pane.terminal.process(&format!(
                            "Could not resolve tile launch.\r\n\r\n{error}\r\n"
                        ));
                        unsafe {
                            InvalidateRect(pane.output_hwnd, ptr::null(), 1);
                        }
                        continue;
                    }
                };
            let command = match wsl::build_launch_command(
                &pane.tile,
                &workspace_root,
                &resolved_launch,
                &state.runtime,
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

            match PaneSession::spawn(hwnd, pane.id, &command, columns, rows) {
                Ok(session) => {
                    pane.session = Some(session);
                    pane.termination_requested = false;
                    pane.reconnect_attempts = 0;
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

    fn refresh_broadcast_controls(state: &WorkspaceWindowState) {
        unsafe {
            SetWindowTextW(
                state.broadcast_target_hwnd,
                wide(&state.broadcast_target.label()).as_ptr(),
            );
            SetWindowTextW(
                state.runbook_hwnd,
                wide(if state.assets.runbooks.is_empty() {
                    "Runbook"
                } else {
                    "Runbook..."
                })
                .as_ptr(),
            );
        }
    }

    fn refresh_alert_button(state: &WorkspaceWindowState) {
        unsafe {
            SetWindowTextW(
                state.alerts_hwnd,
                wide(&format!("Alerts ({})", state.alert_store.unread_count())).as_ptr(),
            );
        }
    }

    fn cycle_broadcast_target(state: &mut WorkspaceWindowState) {
        let mut options = vec![BroadcastTarget::Off, BroadcastTarget::AllPanes];
        options.extend(
            saved_groups_for_tiles(&active_tab(state).preset.layout.tile_specs())
                .into_iter()
                .map(BroadcastTarget::SavedGroup),
        );
        let current_index = options
            .iter()
            .position(|candidate| candidate == &state.broadcast_target)
            .unwrap_or(0);
        state.broadcast_target = options[(current_index + 1) % options.len()].clone();
        refresh_broadcast_controls(state);
    }

    fn send_text_to_pane(pane: &mut PaneState, text: &str) -> bool {
        let Some(session) = pane.session.as_ref() else {
            return false;
        };
        pane.transcript.push_input(text);
        if session.write_input(text.as_bytes()).is_ok() {
            true
        } else {
            false
        }
    }

    fn send_text_to_target(
        state: &mut WorkspaceWindowState,
        target: &BroadcastTarget,
        text: &str,
    ) -> usize {
        let mut sent = 0usize;
        for pane in state.panes.iter_mut() {
            if target.includes(&pane.tile) && send_text_to_pane(pane, text) {
                sent += 1;
            }
        }
        sent
    }

    fn push_alert(state: &WorkspaceWindowState, input: AlertEventInput) {
        state.alert_store.push(input);
        refresh_alert_button(state);
    }

    fn send_broadcast_command(state: &mut WorkspaceWindowState) {
        let command = read_window_text(state.broadcast_entry_hwnd);
        let command = command.trim();
        if command.is_empty() {
            return;
        }
        let payload = if command.ends_with('\n') {
            command.to_string()
        } else {
            format!("{command}\n")
        };
        let target = state.broadcast_target.clone();
        let sent = send_text_to_target(state, &target, &payload);
        let mut alert = AlertEventInput::new(
            AlertSourceKind::Runbook,
            AlertSeverity::Info,
            "Quick send executed",
        );
        alert.detail = format!(
            "Sent quick command to {} pane(s) via {}.",
            sent,
            target.label()
        );
        push_alert(state, alert);
    }

    fn execute_runbook(
        state: &mut WorkspaceWindowState,
        runbook: &crate::model::assets::Runbook,
        variables: &std::collections::HashMap<String, String>,
    ) {
        match resolve_runbook(
            runbook,
            variables,
            &active_tab(state).preset.layout.tile_specs(),
        ) {
            Ok(resolved) => {
                let mut sent = 0usize;
                for command in &resolved.commands {
                    sent += send_text_to_target(state, &resolved.target, command);
                }
                let mut alert = AlertEventInput::new(
                    AlertSourceKind::Runbook,
                    AlertSeverity::Info,
                    format!("Runbook '{}' executed", runbook.name),
                );
                alert.detail = format!(
                    "Targeted {} pane(s) with {} step(s), {} send(s) via {}.",
                    resolved.matching_tile_ids.len(),
                    resolved.commands.len(),
                    sent,
                    resolved.target_label
                );
                push_alert(state, alert);
            }
            Err(error) => {
                let mut alert = AlertEventInput::new(
                    AlertSourceKind::Runbook,
                    AlertSeverity::Error,
                    format!("Runbook '{}' failed", runbook.name),
                );
                alert.detail = error;
                push_alert(state, alert);
            }
        }
    }

    fn open_workspace_assets_manager(hwnd: HWND, state: &mut WorkspaceWindowState) {
        let workspace_root = Some(active_tab(state).workspace_root.clone());
        let on_saved = Rc::new(move || {
            if let Some(state) = unsafe { window_state_mut(hwnd) } {
                rebuild_active_tab_content(hwnd, state);
            }
        });
        let _ = assets_manager::present(hwnd, state.asset_store.clone(), workspace_root, on_saved);
    }

    fn open_runbook_palette(hwnd: HWND, state: &mut WorkspaceWindowState) {
        let mut actions = Vec::new();
        for runbook in state.assets.runbooks.iter().cloned() {
            let subtitle = if runbook.description.trim().is_empty() {
                runbook.target.label()
            } else {
                runbook.description.clone()
            };
            actions.push(command_palette::PaletteAction {
                title: format!("Run {}", runbook.name),
                subtitle,
                on_activate: Rc::new(move || {
                    if let Some(state) = unsafe { window_state_mut(hwnd) } {
                        if runbook.variables.is_empty()
                            && runbook.confirm_policy
                                == crate::model::assets::RunbookConfirmPolicy::Never
                        {
                            execute_runbook(state, &runbook, &std::collections::HashMap::new());
                            return;
                        }

                        let runbook_for_dialog = runbook.clone();
                        let runbook_for_submit = runbook.clone();
                        let on_submit = Rc::new(
                            move |variables: std::collections::HashMap<String, String>| {
                                if let Some(state) = unsafe { window_state_mut(hwnd) } {
                                    execute_runbook(state, &runbook_for_submit, &variables);
                                }
                            },
                        );
                        if let Err(error) =
                            runbook_dialog::present(hwnd, runbook_for_dialog, on_submit)
                        {
                            if let Some(state) = unsafe { window_state_mut(hwnd) } {
                                let mut alert = AlertEventInput::new(
                                    AlertSourceKind::Runbook,
                                    AlertSeverity::Error,
                                    format!("Runbook '{}' failed", runbook.name),
                                );
                                alert.detail = error;
                                push_alert(state, alert);
                            }
                        }
                    }
                }),
            });
        }
        if actions.is_empty() {
            let mut alert = AlertEventInput::new(
                AlertSourceKind::Runbook,
                AlertSeverity::Info,
                "No runbooks available",
            );
            alert.detail = "The active workspace has no saved runbooks.".into();
            push_alert(state, alert);
            return;
        }
        let _ = command_palette::present(hwnd, "Runbooks", actions);
    }

    fn open_alert_center(hwnd: HWND, state: &mut WorkspaceWindowState) {
        let alerts = state.alert_store.snapshot();
        if alerts.is_empty() {
            unsafe {
                MessageBoxW(
                    hwnd,
                    wide("There are no workspace alerts.").as_ptr(),
                    wide("Alert Center").as_ptr(),
                    MB_OK,
                );
            }
            return;
        }
        let entries = alerts
            .into_iter()
            .rev()
            .map(|alert| {
                let pane_id = alert.pane_id.clone();
                let alert_id = alert.id;
                let allows_reconnect = alert.allows_reconnect;
                alert_center::AlertCenterEntry {
                    title: alert.title,
                    detail: if alert.detail.trim().is_empty() {
                        "No detail available.".into()
                    } else {
                        alert.detail
                    },
                    unread: alert.unread,
                    allows_reconnect,
                    on_jump: Rc::new({
                        let pane_id = pane_id.clone();
                        move || {
                            if let Some(state) = unsafe { window_state_mut(hwnd) }
                                && let Some(pane_id) = pane_id.as_deref()
                            {
                                focus_pane(state, pane_id);
                            }
                        }
                    }),
                    on_reconnect: if allows_reconnect {
                        Some(Rc::new({
                            let pane_id = pane_id.clone();
                            move || {
                                if let Some(state) = unsafe { window_state_mut(hwnd) }
                                    && let Some(pane_id) = pane_id.as_deref()
                                    && let Ok(pane_id) = pane_id.parse::<usize>()
                                    && let Err(error) = reconnect_pane(state, pane_id, None)
                                {
                                    let mut alert = AlertEventInput::new(
                                        AlertSourceKind::Reconnect,
                                        AlertSeverity::Error,
                                        "Reconnect failed",
                                    );
                                    alert.detail = error;
                                    alert.pane_id = Some(pane_id.to_string());
                                    alert.allows_reconnect = true;
                                    push_alert(state, alert);
                                }
                            }
                        }))
                    } else {
                        None
                    },
                    on_mark_read: Rc::new(move || {
                        if let Some(state) = unsafe { window_state_mut(hwnd) } {
                            state.alert_store.mark_read(alert_id);
                            refresh_alert_button(state);
                        }
                    }),
                }
            })
            .collect::<Vec<_>>();
        let on_mark_all_read = Rc::new(move || {
            if let Some(state) = unsafe { window_state_mut(hwnd) } {
                state.alert_store.mark_all_read();
                refresh_alert_button(state);
            }
        });
        let _ = alert_center::present(hwnd, entries, on_mark_all_read);
    }

    fn open_workspace_command_palette(hwnd: HWND, state: &mut WorkspaceWindowState) {
        let mut actions = Vec::new();
        actions.push(command_palette::PaletteAction {
            title: "Open Alerts".into(),
            subtitle: "Inspect unread workspace alerts.".into(),
            on_activate: Rc::new(move || {
                if let Some(state) = unsafe { window_state_mut(hwnd) } {
                    open_alert_center(hwnd, state);
                }
            }),
        });
        actions.push(command_palette::PaletteAction {
            title: "Open Runbooks".into(),
            subtitle: "Execute a saved runbook against the current workspace.".into(),
            on_activate: Rc::new(move || {
                if let Some(state) = unsafe { window_state_mut(hwnd) } {
                    open_runbook_palette(hwnd, state);
                }
            }),
        });
        actions.push(command_palette::PaletteAction {
            title: "Add Web Tile".into(),
            subtitle: "Insert a new browser pane beside the focused pane.".into(),
            on_activate: Rc::new(move || {
                if let Some(state) = unsafe { window_state_mut(hwnd) } {
                    add_web_tile_to_active_workspace(hwnd, state);
                }
            }),
        });
        actions.push(command_palette::PaletteAction {
            title: "Open Assets Manager".into(),
            subtitle: "Edit connection profiles, inventory, roles, and runbooks.".into(),
            on_activate: Rc::new(move || {
                if let Some(state) = unsafe { window_state_mut(hwnd) } {
                    open_workspace_assets_manager(hwnd, state);
                }
            }),
        });
        if let Some(first_unread) = state
            .alert_store
            .snapshot()
            .into_iter()
            .find(|alert| alert.unread && alert.pane_id.is_some())
        {
            let pane_id = first_unread.pane_id.clone().unwrap_or_default();
            let alert_id = first_unread.id;
            actions.push(command_palette::PaletteAction {
                title: "Focus Next Alert".into(),
                subtitle: "Jump to the next unread pane alert.".into(),
                on_activate: Rc::new(move || {
                    if let Some(state) = unsafe { window_state_mut(hwnd) } {
                        focus_pane(state, &pane_id);
                        state.alert_store.mark_read(alert_id);
                        refresh_alert_button(state);
                    }
                }),
            });
        }
        let _ = command_palette::present(hwnd, "Workspace Commands", actions);
    }

    fn focus_pane(state: &WorkspaceWindowState, pane_id: &str) {
        if let Some(pane) = state
            .panes
            .iter()
            .find(|pane| pane.tile.id == pane_id || pane.id.to_string() == pane_id)
        {
            unsafe {
                SetFocus(pane.output_hwnd);
            }
        }
    }

    fn add_web_tile_to_active_workspace(hwnd: HWND, state: &mut WorkspaceWindowState) {
        let target_tile_id = state
            .panes
            .iter()
            .find(|pane| pane.focused)
            .map(|pane| pane.tile.id.clone())
            .or_else(|| {
                state
                    .focused_web_pane_id
                    .and_then(|pane_id| pane_by_id(state, pane_id))
                    .map(|pane| pane.tile.id.clone())
            })
            .or_else(|| state.panes.first().map(|pane| pane.tile.id.clone()));
        let Some(target_tile_id) = target_tile_id else {
            return;
        };

        let current_layout = active_tab(state).preset.layout.clone();
        let Some((next_layout, new_tile_id)) = split_tile_with_kind(
            &current_layout,
            &target_tile_id,
            SplitAxis::Horizontal,
            false,
            TileKind::WebView,
        ) else {
            return;
        };

        active_tab_mut(state).preset.layout = next_layout;
        rebuild_active_tab_content(hwnd, state);
        focus_pane(state, &new_tile_id);
        unsafe {
            SetFocus(state.url_hwnd);
            SendMessageW(state.url_hwnd, EM_SETSEL_MESSAGE, 0, -1isize as LPARAM);
        }
    }

    fn handle_workspace_shortcuts(
        hwnd: HWND,
        state: &mut WorkspaceWindowState,
        virtual_key: u32,
    ) -> bool {
        let preferences = state.preference_store.load();
        if shortcut_capture::matches_keydown(&preferences.command_palette_shortcut, virtual_key) {
            open_workspace_command_palette(hwnd, state);
            return true;
        }
        if shortcut_capture::matches_keydown(
            &preferences.workspace_fullscreen_shortcut,
            virtual_key,
        ) {
            toggle_workspace_fullscreen(hwnd, state);
            return true;
        }
        if shortcut_capture::matches_keydown(&preferences.workspace_density_shortcut, virtual_key) {
            cycle_workspace_density(hwnd, state);
            return true;
        }
        if shortcut_capture::matches_keydown(&preferences.workspace_zoom_in_shortcut, virtual_key) {
            adjust_terminal_zoom(hwnd, state, 1);
            return true;
        }
        if shortcut_capture::matches_keydown(&preferences.workspace_zoom_out_shortcut, virtual_key)
        {
            adjust_terminal_zoom(hwnd, state, -1);
            return true;
        }
        false
    }

    fn reconnect_pane(
        state: &mut WorkspaceWindowState,
        pane_id: usize,
        expected_attempt: Option<u8>,
    ) -> Result<(), String> {
        let workspace_root = active_tab(state).workspace_root.clone();
        let asset_outcome = state
            .asset_store
            .load_assets_for_workspace_root(&workspace_root);
        let runtime = state.runtime.clone();
        let Some(pane) = pane_mut_by_id(state, pane_id) else {
            return Err(format!("Pane {pane_id} is missing."));
        };
        if let Some(expected_attempt) = expected_attempt
            && pane.reconnect_attempts != expected_attempt
        {
            return Ok(());
        }
        if let Some(mut session) = pane.session.take() {
            pane.termination_requested = true;
            session.terminate();
        }
        let resolved = resolve_tile_launch(&pane.tile, &workspace_root, &asset_outcome.assets)?;
        let command = wsl::build_launch_command(&pane.tile, &workspace_root, &resolved, &runtime)?;
        let output_bounds = client_bounds(pane.output_hwnd).unwrap_or(Bounds {
            left: 0,
            top: 0,
            right: 720,
            bottom: 420,
        });
        let (columns, rows) =
            pane_console_size(output_bounds.width(), output_bounds.height(), pane);
        pane.terminal.resize(columns as usize, rows as usize);
        pane.termination_requested = false;
        match PaneSession::spawn(pane.parent_hwnd, pane.id, &command, columns, rows) {
            Ok(session) => {
                pane.session = Some(session);
                pane.reconnect_attempts = 0;
                let pane_title = pane.tile.title.clone();
                let pane_id_string = pane.id.to_string();
                let working_directory = command.working_directory.clone();
                pane.terminal.process(&format!(
                    "\r\n[reconnected {} in {}]\r\n",
                    pane.tile.title, command.working_directory
                ));
                update_pane_scrollbar(pane);
                unsafe {
                    InvalidateRect(pane.title_hwnd, ptr::null(), 1);
                    InvalidateRect(pane.output_hwnd, ptr::null(), 1);
                }
                let mut reconnect_alert = AlertEventInput::new(
                    AlertSourceKind::Reconnect,
                    AlertSeverity::Info,
                    format!("{pane_title} reconnected"),
                );
                reconnect_alert.detail = format!("Pane restarted in {working_directory}.");
                reconnect_alert.pane_id = Some(pane_id_string);
                reconnect_alert.allows_reconnect = true;
                push_alert(state, reconnect_alert);
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn active_tab(state: &WorkspaceWindowState) -> &SavedTab {
        &state.tabs[state.active_tab_index]
    }

    fn active_tab_mut(state: &mut WorkspaceWindowState) -> &mut SavedTab {
        &mut state.tabs[state.active_tab_index]
    }

    fn pane_mut_by_id(state: &mut WorkspaceWindowState, pane_id: usize) -> Option<&mut PaneState> {
        state
            .panes
            .iter_mut()
            .find(|pane| pane.id == pane_id)
            .map(Box::as_mut)
    }

    fn pane_by_id(state: &WorkspaceWindowState, pane_id: usize) -> Option<&PaneState> {
        state
            .panes
            .iter()
            .find(|pane| pane.id == pane_id)
            .map(Box::as_ref)
    }

    fn active_tab_has_web_tiles(state: &WorkspaceWindowState) -> bool {
        if !state.panes.is_empty() {
            return state
                .panes
                .iter()
                .any(|pane| pane.tile.tile_kind == TileKind::WebView);
        }

        active_tab(state)
            .preset
            .layout
            .tile_specs()
            .iter()
            .any(|tile| tile.tile_kind == TileKind::WebView)
    }

    fn first_web_pane_id(state: &WorkspaceWindowState) -> Option<usize> {
        state
            .panes
            .iter()
            .find(|pane| pane.tile.tile_kind == TileKind::WebView)
            .map(|pane| pane.id)
    }

    fn set_focused_web_pane(state: &mut WorkspaceWindowState, pane_id: Option<usize>) {
        state.focused_web_pane_id = pane_id.filter(|pane_id| {
            pane_by_id(state, *pane_id)
                .map(|pane| pane.tile.tile_kind == TileKind::WebView)
                .unwrap_or(false)
        });
        sync_web_navigation_controls(state);
    }

    fn current_web_entry_text(state: &WorkspaceWindowState) -> String {
        read_window_text(state.url_hwnd).trim().to_string()
    }

    fn normalize_web_url(value: &str) -> String {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            String::new()
        } else if trimmed.contains("://") {
            trimmed.to_string()
        } else {
            format!("https://{trimmed}")
        }
    }

    fn sync_web_navigation_controls(state: &WorkspaceWindowState) {
        let has_web_tiles = active_tab_has_web_tiles(state);
        let focused_pane = state
            .focused_web_pane_id
            .and_then(|pane_id| pane_by_id(state, pane_id));
        let url_text = focused_pane
            .and_then(|pane| pane.webview_uri.clone().or_else(|| pane.tile.url.clone()))
            .unwrap_or_default();

        unsafe {
            ShowWindow(state.path_hwnd, if has_web_tiles { 0 } else { SW_SHOW });
            ShowWindow(state.url_hwnd, if has_web_tiles { SW_SHOW } else { 0 });
            ShowWindow(state.url_reload_hwnd, if has_web_tiles { SW_SHOW } else { 0 });
            windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                state.url_hwnd,
                focused_pane.is_some() as i32,
            );
            windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                state.url_reload_hwnd,
                focused_pane.is_some() as i32,
            );
            if has_web_tiles {
                SetWindowTextW(state.url_hwnd, wide(&url_text).as_ptr());
            } else {
                SetWindowTextW(
                    state.path_hwnd,
                    wide(&active_tab(state).workspace_root.display().to_string()).as_ptr(),
                );
            }
        }
    }

    fn post_webview_string_message(window_hwnd: HWND, message: u32, pane_id: usize, value: String) {
        let event_ptr = Box::into_raw(Box::new(WebViewStringEvent { pane_id, value }));
        let posted = unsafe { PostMessageW(window_hwnd, message, 0, event_ptr as LPARAM) };
        if posted == 0 {
            unsafe {
                drop(Box::from_raw(event_ptr));
            }
        }
    }

    fn apply_webview_uri_update(
        state: &mut WorkspaceWindowState,
        pane_id: usize,
        value: &str,
    ) {
        if let Some(pane) = pane_mut_by_id(state, pane_id) {
            pane.webview_uri = Some(value.to_string());
        }
        if state.focused_web_pane_id == Some(pane_id) {
            sync_web_navigation_controls(state);
        }
    }

    fn apply_webview_title_update(
        state: &mut WorkspaceWindowState,
        pane_id: usize,
        value: &str,
    ) {
        if let Some(pane) = pane_mut_by_id(state, pane_id) {
            pane.webview_title = (!value.trim().is_empty()).then(|| value.to_string());
            unsafe {
                InvalidateRect(pane.title_hwnd, ptr::null(), 1);
            }
        }
    }

    fn navigate_web_pane_by_id(
        state: &mut WorkspaceWindowState,
        pane_id: usize,
        url: &str,
    ) -> Result<(), String> {
        let webview = pane_by_id(state, pane_id)
            .and_then(|pane| pane.webview.clone())
            .ok_or_else(|| format!("Web pane {pane_id} is unavailable."))?;
        let url = normalize_web_url(url);
        if url.is_empty() {
            return Ok(());
        }

        unsafe {
            webview
                .Navigate(&HSTRING::from(url.as_str()))
                .map_err(|error| format!("WebView2 navigation failed: {error}"))?;
        }

        if let Some(pane) = pane_mut_by_id(state, pane_id) {
            pane.webview_uri = Some(url.clone());
        }
        if state.focused_web_pane_id == Some(pane_id) {
            sync_web_navigation_controls(state);
        }
        Ok(())
    }

    fn reload_web_pane_by_id(state: &mut WorkspaceWindowState, pane_id: usize) -> Result<(), String> {
        let webview = pane_by_id(state, pane_id)
            .and_then(|pane| pane.webview.clone())
            .ok_or_else(|| format!("Web pane {pane_id} is unavailable."))?;
        unsafe {
            webview
                .Reload()
                .map_err(|error| format!("WebView2 reload failed: {error}"))?;
        }
        Ok(())
    }

    fn activate_web_navigation_control(state: &mut WorkspaceWindowState) {
        let Some(pane_id) = state.focused_web_pane_id else {
            return;
        };

        let entered_url = normalize_web_url(&current_web_entry_text(state));
        let current_url = pane_by_id(state, pane_id)
            .and_then(|pane| pane.webview_uri.clone())
            .unwrap_or_default();

        let result = if !entered_url.is_empty() && entered_url != current_url {
            navigate_web_pane_by_id(state, pane_id, &entered_url)
        } else {
            reload_web_pane_by_id(state, pane_id)
        };

        if let Err(error) = result {
            logging::error(error);
        }
    }

    fn start_webview_auto_refresh(pane: &mut PaneState, interval_seconds: u32) {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_signal = stop.clone();
        let window_hwnd = pane.parent_hwnd as isize;
        let pane_id = pane.id;
        thread::spawn(move || {
            while !stop_signal.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_secs(interval_seconds as u64));
                if stop_signal.load(Ordering::Relaxed) {
                    break;
                }
                let posted = unsafe {
                    PostMessageW(window_hwnd as HWND, WM_WEBVIEW_AUTO_REFRESH, pane_id, 0)
                };
                if posted == 0 {
                    break;
                }
            }
        });
        pane.auto_refresh_stop = Some(stop);
    }

    fn current_web_pane_url(pane: &PaneState) -> String {
        pane.webview_uri
            .clone()
            .or_else(|| pane.tile.url.clone())
            .unwrap_or_default()
    }

    fn show_web_pane_context_menu(pane: &PaneState, screen_point: POINT) {
        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            return;
        }

        let current_url = current_web_pane_url(pane);
        let has_url = !current_url.trim().is_empty();
        unsafe {
            AppendMenuW(menu, MF_STRING, MENU_WEB_RELOAD, wide("Reload").as_ptr());
            AppendMenuW(
                menu,
                MF_STRING | if has_url { 0 } else { MF_GRAYED },
                MENU_WEB_OPEN_EXTERNAL,
                wide("Open in Browser").as_ptr(),
            );
            AppendMenuW(
                menu,
                MF_STRING | if has_url { 0 } else { MF_GRAYED },
                MENU_WEB_COPY_URL,
                wide("Copy URL").as_ptr(),
            );
        }

        let command = unsafe {
            TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                screen_point.x,
                screen_point.y,
                0,
                pane.parent_hwnd,
                ptr::null(),
            )
        };

        match command as usize {
            MENU_WEB_RELOAD => {
                if let Some(webview) = pane.webview.as_ref()
                    && let Err(error) = unsafe { webview.Reload() }
                {
                    logging::error(format!("WebView2 reload from context menu failed: {error}"));
                }
            }
            MENU_WEB_OPEN_EXTERNAL => {
                if has_url && let Err(error) = open_url(&current_url) {
                    logging::error(format!("Opening web tile URL externally failed: {error}"));
                }
            }
            MENU_WEB_COPY_URL => {
                if has_url && let Err(error) = write_clipboard_text(&current_url) {
                    logging::error(format!("Copying web tile URL failed: {error}"));
                }
            }
            _ => {}
        }

        unsafe {
            DestroyMenu(menu);
        }
    }

    fn show_web_pane_context_menu_for_point(
        parent_hwnd: HWND,
        pane_id: usize,
        mut point: POINT,
    ) {
        let Some(state) = (unsafe { window_state_mut(parent_hwnd) }) else {
            return;
        };
        let Some(pane) = pane_by_id(state, pane_id) else {
            return;
        };

        unsafe {
            ClientToScreen(pane.output_hwnd, &mut point);
        }
        show_web_pane_context_menu(pane, point);
    }

    fn handle_webview_new_window_request(
        parent_hwnd: HWND,
        pane_id: usize,
        args: &ICoreWebView2NewWindowRequestedEventArgs,
    ) -> windows::core::Result<()> {
        let mut requested_uri = PWSTR::null();
        unsafe {
            args.Uri(&mut requested_uri)?;
        }
        let requested_uri = take_pwstr(requested_uri);

        let mut is_user_initiated = windows::Win32::Foundation::BOOL::default();
        unsafe {
            args.IsUserInitiated(&mut is_user_initiated)?;
            args.SetHandled(true)?;
        }

        if !is_user_initiated.as_bool() || requested_uri.trim().is_empty() {
            return Ok(());
        }

        if let Some(state) = unsafe { window_state_mut(parent_hwnd) }
            && let Some(pane) = pane_mut_by_id(state, pane_id)
        {
            pane.webview_uri = Some(requested_uri.clone());
        }

        if let Err(error) = open_url(&requested_uri) {
            logging::error(format!("Opening popup request externally failed: {error}"));
        }

        Ok(())
    }

    fn initialize_web_pane(
        state: &mut WorkspaceWindowState,
        pane_index: usize,
    ) -> Result<(), String> {
        let environment = ensure_webview_environment(state)?;
        let (pane_id, parent_hwnd, output_hwnd, initial_url, auto_refresh_seconds) = {
            let pane = &state.panes[pane_index];
            (
                pane.id,
                pane.parent_hwnd,
                pane.output_hwnd,
                pane.tile
                    .url
                    .clone()
                    .unwrap_or_else(|| "about:blank".to_string()),
                pane.tile.auto_refresh_seconds,
            )
        };
        let controller = create_webview_controller(output_hwnd, &environment)?;
        let webview = unsafe {
            controller
                .CoreWebView2()
                .map_err(|error| format!("CoreWebView2 controller access failed: {error}"))?
        };

        unsafe {
            let settings = webview
                .Settings()
                .map_err(|error| format!("WebView2 settings access failed: {error}"))?;
            settings
                .SetIsStatusBarEnabled(false)
                .map_err(|error| format!("Disabling WebView2 status bar failed: {error}"))?;
            settings
                .SetIsZoomControlEnabled(false)
                .map_err(|error| format!("Disabling WebView2 zoom controls failed: {error}"))?;
        }

        let mut token = EventRegistrationToken::default();
        unsafe {
            webview
                .add_DocumentTitleChanged(
                    &DocumentTitleChangedEventHandler::create(Box::new(move |webview, _| {
                        let Some(webview) = webview else {
                            return Ok(());
                        };
                        let mut title = PWSTR::null();
                        webview.DocumentTitle(&mut title)?;
                        post_webview_string_message(
                            parent_hwnd,
                            WM_WEBVIEW_TITLE_CHANGED,
                            pane_id,
                            take_pwstr(title),
                        );
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| format!("Registering WebView2 title handler failed: {error}"))?;
            webview
                .add_NewWindowRequested(
                    &NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else {
                            return Ok(());
                        };
                        handle_webview_new_window_request(parent_hwnd, pane_id, &args)
                    })),
                    &mut token,
                )
                .map_err(|error| format!("Registering WebView2 popup handler failed: {error}"))?;
            webview
                .add_NavigationCompleted(
                    &NavigationCompletedEventHandler::create(Box::new(move |webview, _| {
                        let Some(webview) = webview else {
                            return Ok(());
                        };
                        let mut source = PWSTR::null();
                        webview.Source(&mut source)?;
                        post_webview_string_message(
                            parent_hwnd,
                            WM_WEBVIEW_URI_CHANGED,
                            pane_id,
                            take_pwstr(source),
                        );
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| format!("Registering WebView2 navigation handler failed: {error}"))?;
            if let Ok(webview11) = webview.cast::<ICoreWebView2_11>() {
                let mut context_menu_token = EventRegistrationToken::default();
                webview11
                    .add_ContextMenuRequested(
                        &ContextMenuRequestedEventHandler::create(Box::new(move |_, args| {
                            let Some(args) = args else {
                                return Ok(());
                            };
                            let mut point = windows::Win32::Foundation::POINT::default();
                            args.Location(&mut point)?;
                            args.SetHandled(true)?;
                            show_web_pane_context_menu_for_point(
                                parent_hwnd,
                                pane_id,
                                POINT {
                                    x: point.x,
                                    y: point.y,
                                },
                            );
                            Ok(())
                        })),
                        &mut context_menu_token,
                    )
                    .map_err(|error| format!("Registering WebView2 context menu handler failed: {error}"))?;
            }
            let _ = controller.SetBounds(WinRect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            });
            webview
                .Navigate(&HSTRING::from(initial_url.as_str()))
                .map_err(|error| format!("Initial WebView2 navigation failed: {error}"))?;
        }

        let pane = &mut state.panes[pane_index];
        pane.webview_controller = Some(controller);
        pane.webview = Some(webview);
        pane.webview_uri = Some(initial_url);
        if let Some(interval_seconds) = auto_refresh_seconds {
            start_webview_auto_refresh(pane, interval_seconds);
        }
        Ok(())
    }

    fn session_registry() -> &'static Mutex<WorkspaceSessionRegistry> {
        SESSION_REGISTRY.get_or_init(|| Mutex::new(WorkspaceSessionRegistry::default()))
    }

    fn current_saved_session(state: &WorkspaceWindowState) -> SavedSession {
        SavedSession {
            tabs: state.tabs.clone(),
            active_tab_index: state.active_tab_index,
        }
    }

    fn persist_workspace_registry(
        registry: &WorkspaceSessionRegistry,
        session_store: &SessionStore,
    ) {
        if registry.windows.is_empty() {
            session_store.clear();
            return;
        }

        let active_window_id = registry
            .active_window_id
            .filter(|id| registry.windows.contains_key(id))
            .or_else(|| registry.windows.keys().next().copied());

        let mut tabs = Vec::new();
        let mut active_tab_index = 0usize;
        let mut current_offset = 0usize;

        for (window_id, session) in &registry.windows {
            if Some(*window_id) == active_window_id {
                active_tab_index = current_offset
                    + session
                        .active_tab_index
                        .min(session.tabs.len().saturating_sub(1));
            }
            current_offset += session.tabs.len();
            tabs.extend(session.tabs.clone());
        }

        session_store.save(&SavedSession {
            tabs,
            active_tab_index,
        });
    }

    fn save_workspace_session_state(state: &WorkspaceWindowState) {
        let registry_lock = session_registry().lock();
        let Ok(mut registry) = registry_lock else {
            logging::error("workspace session registry lock poisoned while saving");
            return;
        };
        registry
            .windows
            .insert(state.window_id, current_saved_session(state));
        registry.active_window_id = Some(state.window_id);
        persist_workspace_registry(&registry, &state.session_store);
    }

    fn remove_workspace_session_state(window_id: usize, session_store: &SessionStore) {
        let registry_lock = session_registry().lock();
        let Ok(mut registry) = registry_lock else {
            logging::error("workspace session registry lock poisoned while removing");
            return;
        };
        registry.windows.remove(&window_id);
        if registry.active_window_id == Some(window_id) {
            registry.active_window_id = registry.windows.keys().next_back().copied();
        }
        persist_workspace_registry(&registry, session_store);
    }

    fn rebuild_tab_buttons(hwnd: HWND, state: &mut WorkspaceWindowState) {
        state.tab_drag = None;
        for button in state.tab_button_hwnds.drain(..) {
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(button);
            }
        }

        for (index, _) in state.tabs.iter().enumerate() {
            let button_state = Box::new(TabButtonState {
                parent_hwnd: hwnd,
                index,
                active: index == state.active_tab_index,
                press_origin: None,
            });
            let button = create_child_window(
                hwnd,
                TAB_BUTTON_CLASS,
                "",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                0,
                ID_TAB_BUTTON_BASE + index as isize,
                Box::into_raw(button_state).cast(),
            );
            if !button.is_null() {
                state.tab_button_hwnds.push(button);
            }
        }
    }

    fn destroy_active_panes(state: &mut WorkspaceWindowState) {
        state.pane_drag = None;
        for pane in &state.panes {
            unsafe {
                if !pane.title_hwnd.is_null() {
                    windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(pane.title_hwnd);
                }
                if !pane.output_hwnd.is_null() {
                    windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(pane.output_hwnd);
                }
            }
        }
        state.panes.clear();
    }

    fn rebuild_active_tab_content(hwnd: HWND, state: &mut WorkspaceWindowState) {
        destroy_active_panes(state);
        let asset_outcome = state
            .asset_store
            .load_assets_for_workspace_root(&active_tab(state).workspace_root);
        state.assets = asset_outcome.assets;
        state.asset_warning = asset_outcome.warning;
        state.broadcast_target = BroadcastTarget::Off;

        let title = active_tab(state)
            .custom_title
            .clone()
            .unwrap_or_else(|| active_tab(state).preset.name.clone());
        state.suppress_title_events = true;
        unsafe {
            SetWindowTextW(state.title_hwnd, wide(&title).as_ptr());
            SetWindowTextW(
                state.path_hwnd,
                wide(&active_tab(state).workspace_root.display().to_string()).as_ptr(),
            );
            SetWindowTextW(
                state.density_hwnd,
                wide(density_button_label(active_tab(state).preset.density)).as_ptr(),
            );
            SetWindowTextW(hwnd, wide(&title).as_ptr());
        }
        state.suppress_title_events = false;

        let tile_specs = active_tab(state).preset.layout.tile_specs();
        let font_points = effective_terminal_font_points(
            active_tab(state).preset.density,
            active_tab(state).terminal_zoom_steps,
        );
        let line_height_scale = active_tab(state)
            .preset
            .density
            .terminal_line_height_scale();
        let ui_font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
        state.panes = Vec::with_capacity(tile_specs.len());
        for tile in tile_specs {
            let initial_web_uri = tile.url.clone();
            let is_web_tile = tile.tile_kind == TileKind::WebView;
            let mut pane = Box::new(PaneState {
                id: NEXT_PANE_ID.fetch_add(1, Ordering::Relaxed),
                parent_hwnd: hwnd,
                tile,
                title_hwnd: ptr::null_mut(),
                output_hwnd: ptr::null_mut(),
                terminal: VtBuffer::new(80, 24),
                webview_controller: None,
                webview: None,
                webview_uri: initial_web_uri,
                webview_title: None,
                auto_refresh_stop: None,
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
                header_press_origin: None,
                transcript: TranscriptBuffer::default(),
                last_helper_signature: String::new(),
                reconnect_attempts: 0,
                termination_requested: false,
                session: None,
            });
            pane.title_hwnd = create_child_window(
                hwnd,
                PANE_HEADER_CLASS,
                "",
                WS_CHILD | WS_VISIBLE | SS_NOTIFY_STYLE,
                0,
                0,
                (&mut *pane as *mut PaneState).cast(),
            );
            let pane_ptr: *mut PaneState = &mut *pane;
            pane.output_hwnd = create_child_window(
                hwnd,
                PANE_CLASS,
                "",
                if is_web_tile {
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER
                } else {
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | WS_VSCROLL
                },
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
            if !is_web_tile {
                update_pane_scrollbar(&pane);
            }
            state.panes.push(pane);
        }

        for pane_index in 0..state.panes.len() {
            if state.panes[pane_index].tile.tile_kind != TileKind::WebView {
                continue;
            }
            let result = initialize_web_pane(state, pane_index);
            if let Err(error) = result {
                let pane = &mut state.panes[pane_index];
                logging::error(format!(
                    "failed to initialize web tile '{}': {error}",
                    pane.tile.title
                ));
                pane.terminal.process(&format!(
                    "Could not initialize embedded browser.\r\n\r\n{error}\r\n"
                ));
            }
        }

        state.focused_web_pane_id = first_web_pane_id(state);

        rebuild_tab_buttons(hwnd, state);
        update_tab_action_buttons(state);
        refresh_broadcast_controls(state);
        refresh_alert_button(state);
        layout_controls(hwnd, state);
        spawn_pane_sessions(hwnd, state);
        save_workspace_session_state(state);
    }

    fn switch_active_tab(hwnd: HWND, state: &mut WorkspaceWindowState, index: usize) {
        if index >= state.tabs.len() || index == state.active_tab_index {
            return;
        }
        state.active_tab_index = index;
        rebuild_active_tab_content(hwnd, state);
    }

    fn begin_tab_rename(hwnd: HWND, state: &mut WorkspaceWindowState, index: usize) {
        if index >= state.tabs.len() {
            return;
        }
        if index != state.active_tab_index {
            state.active_tab_index = index;
            rebuild_active_tab_content(hwnd, state);
        }
        unsafe {
            SetFocus(state.title_hwnd);
            SendMessageW(state.title_hwnd, EM_SETSEL_MESSAGE, 0, -1isize as LPARAM);
        }
    }

    fn drag_threshold_exceeded(origin: POINT, current: POINT) -> bool {
        (origin.x - current.x).abs() >= 4 || (origin.y - current.y).abs() >= 4
    }

    fn screen_point_from_lparam(hwnd: HWND, lparam: LPARAM) -> POINT {
        let mut point = POINT {
            x: ((lparam as i32) & 0xffff) as i16 as i32,
            y: (((lparam as i32) >> 16) & 0xffff) as i16 as i32,
        };
        unsafe {
            ClientToScreen(hwnd, &mut point);
        }
        point
    }

    fn invalidate_tab_buttons(state: &WorkspaceWindowState) {
        for hwnd in &state.tab_button_hwnds {
            unsafe {
                InvalidateRect(*hwnd, ptr::null(), 1);
            }
        }
    }

    fn invalidate_pane_headers(state: &WorkspaceWindowState) {
        for pane in &state.panes {
            unsafe {
                InvalidateRect(pane.title_hwnd, ptr::null(), 1);
            }
        }
    }

    fn update_tab_drag(state: &mut WorkspaceWindowState, dragged_index: usize, point: POINT) {
        let mut next_drag = None;
        for (index, hwnd) in state.tab_button_hwnds.iter().enumerate() {
            let mut rect = unsafe { mem::zeroed::<RECT>() };
            unsafe {
                GetWindowRect(*hwnd, &mut rect);
            }
            if unsafe { windows_sys::Win32::Graphics::Gdi::PtInRect(&rect, point) } != 0 {
                let midpoint = rect.left + ((rect.right - rect.left) / 2);
                next_drag = Some(TabDragState {
                    dragged_index,
                    target_index: index,
                    insert_after: point.x >= midpoint,
                });
                break;
            }
        }

        let changed = match (&state.tab_drag, &next_drag) {
            (Some(current), Some(next)) => {
                current.dragged_index != next.dragged_index
                    || current.target_index != next.target_index
                    || current.insert_after != next.insert_after
            }
            (None, Some(_)) | (Some(_), None) => true,
            (None, None) => false,
        };
        state.tab_drag = next_drag;
        if changed {
            invalidate_tab_buttons(state);
        }
    }

    fn finish_tab_drag(hwnd: HWND, state: &mut WorkspaceWindowState, clicked_index: usize) {
        let drag = state.tab_drag.take();
        invalidate_tab_buttons(state);

        let Some(drag) = drag else {
            switch_active_tab(hwnd, state, clicked_index);
            return;
        };

        if reorder_tab_index(
            state,
            drag.dragged_index,
            drag.target_index,
            drag.insert_after,
        ) {
            rebuild_tab_buttons(hwnd, state);
            update_tab_action_buttons(state);
            layout_controls(hwnd, state);
            save_workspace_session_state(state);
        }
    }

    fn reorder_tab_index(
        state: &mut WorkspaceWindowState,
        dragged_index: usize,
        target_index: usize,
        insert_after: bool,
    ) -> bool {
        if dragged_index >= state.tabs.len() || target_index >= state.tabs.len() {
            return false;
        }

        let mut insert_index = if insert_after {
            target_index + 1
        } else {
            target_index
        };
        if dragged_index < insert_index {
            insert_index = insert_index.saturating_sub(1);
        }
        if insert_index == dragged_index {
            return false;
        }

        let active_index = state.active_tab_index;
        let tab = state.tabs.remove(dragged_index);
        let insert_index = insert_index.min(state.tabs.len());
        state.tabs.insert(insert_index, tab);
        state.active_tab_index =
            remap_active_index_after_move(active_index, dragged_index, insert_index);
        true
    }

    fn remap_active_index_after_move(
        active_index: usize,
        dragged_index: usize,
        insert_index: usize,
    ) -> usize {
        if active_index == dragged_index {
            insert_index
        } else if dragged_index < active_index && active_index <= insert_index {
            active_index - 1
        } else if insert_index <= active_index && active_index < dragged_index {
            active_index + 1
        } else {
            active_index
        }
    }

    fn update_pane_drag(state: &mut WorkspaceWindowState, dragged_pane_id: usize, point: POINT) {
        let mut next_drag = None;
        for pane in &state.panes {
            let mut title_rect = unsafe { mem::zeroed::<RECT>() };
            let mut output_rect = unsafe { mem::zeroed::<RECT>() };
            unsafe {
                GetWindowRect(pane.title_hwnd, &mut title_rect);
                GetWindowRect(pane.output_hwnd, &mut output_rect);
            }
            if unsafe { windows_sys::Win32::Graphics::Gdi::PtInRect(&title_rect, point) } != 0
                || unsafe { windows_sys::Win32::Graphics::Gdi::PtInRect(&output_rect, point) } != 0
            {
                next_drag = Some(PaneDragState {
                    dragged_pane_id,
                    target_pane_id: pane.id,
                });
                break;
            }
        }

        let changed = match (&state.pane_drag, &next_drag) {
            (Some(current), Some(next)) => {
                current.dragged_pane_id != next.dragged_pane_id
                    || current.target_pane_id != next.target_pane_id
            }
            (None, Some(_)) | (Some(_), None) => true,
            (None, None) => false,
        };
        state.pane_drag = next_drag;
        if changed {
            invalidate_pane_headers(state);
        }
    }

    fn finish_pane_drag(hwnd: HWND, state: &mut WorkspaceWindowState, pane_id: usize) {
        let drag = state.pane_drag.take();
        invalidate_pane_headers(state);

        let Some(drag) = drag else {
            if let Some(pane) = pane_by_id(state, pane_id) {
                unsafe {
                    SetFocus(pane.output_hwnd);
                }
            }
            return;
        };

        if drag.dragged_pane_id != drag.target_pane_id {
            swap_active_panes(hwnd, state, drag.dragged_pane_id, drag.target_pane_id);
        } else if let Some(pane) = pane_by_id(state, pane_id) {
            unsafe {
                SetFocus(pane.output_hwnd);
            }
        }
    }

    fn swap_active_panes(
        hwnd: HWND,
        state: &mut WorkspaceWindowState,
        dragged_pane_id: usize,
        target_pane_id: usize,
    ) {
        let Some(dragged_index) = state
            .panes
            .iter()
            .position(|pane| pane.id == dragged_pane_id)
        else {
            return;
        };
        let Some(target_index) = state
            .panes
            .iter()
            .position(|pane| pane.id == target_pane_id)
        else {
            return;
        };
        if dragged_index == target_index {
            return;
        }

        let dragged_tile_id = state.panes[dragged_index].tile.id.clone();
        let target_tile_id = state.panes[target_index].tile.id.clone();
        let Some(next_layout) = active_tab(state)
            .preset
            .layout
            .swap_tile_positions(&dragged_tile_id, &target_tile_id)
        else {
            return;
        };

        state.panes.swap(dragged_index, target_index);
        active_tab_mut(state).preset.layout = next_layout;
        layout_controls(hwnd, state);
        save_workspace_session_state(state);
        invalidate_pane_headers(state);
    }

    fn move_active_tab(hwnd: HWND, state: &mut WorkspaceWindowState, direction: isize) {
        let current = state.active_tab_index as isize;
        let target = current + direction;
        if target < 0 || target >= state.tabs.len() as isize {
            return;
        }
        state.tabs.swap(current as usize, target as usize);
        state.active_tab_index = target as usize;
        rebuild_tab_buttons(hwnd, state);
        update_tab_action_buttons(state);
        layout_controls(hwnd, state);
        save_workspace_session_state(state);
    }

    fn close_active_tab(hwnd: HWND, state: &mut WorkspaceWindowState) {
        if state.tabs.is_empty() {
            return;
        }
        let closing_title = active_tab(state)
            .custom_title
            .clone()
            .unwrap_or_else(|| active_tab(state).preset.name.clone());
        logging::info(format!("closing Windows workspace tab '{closing_title}'"));
        state.tabs.remove(state.active_tab_index);
        if state.tabs.is_empty() {
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
            }
            return;
        }
        if state.active_tab_index >= state.tabs.len() {
            state.active_tab_index = state.tabs.len() - 1;
        }
        rebuild_active_tab_content(hwnd, state);
    }

    fn update_tab_action_buttons(state: &WorkspaceWindowState) {
        let can_move_left = state.active_tab_index > 0;
        let can_move_right = state.active_tab_index + 1 < state.tabs.len();
        let can_close = !state.tabs.is_empty();
        unsafe {
            windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                state.move_left_hwnd,
                can_move_left as i32,
            );
            windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                state.move_right_hwnd,
                can_move_right as i32,
            );
            windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                state.close_tab_hwnd,
                can_close as i32,
            );
        }
    }

    fn sync_workspace_title(hwnd: HWND, state: &mut WorkspaceWindowState) {
        if state.suppress_title_events {
            return;
        }
        let title = read_window_text(state.title_hwnd);
        let trimmed = title.trim();
        active_tab_mut(state).custom_title = (!trimmed.is_empty()
            && trimmed != active_tab(state).preset.name)
            .then(|| trimmed.to_string());
        let window_title = active_tab(state)
            .custom_title
            .as_deref()
            .unwrap_or(&active_tab(state).preset.name);
        unsafe {
            SetWindowTextW(hwnd, wide(window_title).as_ptr());
        }
        rebuild_tab_buttons(hwnd, state);
        layout_controls(hwnd, state);
        save_workspace_session_state(state);
    }

    fn adjust_terminal_zoom(hwnd: HWND, state: &mut WorkspaceWindowState, delta: i32) {
        let next_steps = clamp_terminal_zoom_steps(
            active_tab(state).preset.density,
            active_tab(state).terminal_zoom_steps + delta,
        );
        if next_steps == active_tab(state).terminal_zoom_steps {
            return;
        }
        active_tab_mut(state).terminal_zoom_steps = next_steps;
        apply_workspace_terminal_presentation(hwnd, state);
        save_workspace_session_state(state);
    }

    fn cycle_workspace_density(hwnd: HWND, state: &mut WorkspaceWindowState) {
        let next_density = active_tab(state).preset.density.next();
        active_tab_mut(state).preset.density = next_density;
        unsafe {
            SetWindowTextW(
                state.density_hwnd,
                wide(density_button_label(active_tab(state).preset.density)).as_ptr(),
            );
        }
        active_tab_mut(state).terminal_zoom_steps = clamp_terminal_zoom_steps(
            active_tab(state).preset.density,
            active_tab(state).terminal_zoom_steps,
        );
        apply_workspace_terminal_presentation(hwnd, state);
        rebuild_tab_buttons(hwnd, state);
        save_workspace_session_state(state);
    }

    fn apply_workspace_terminal_presentation(hwnd: HWND, state: &mut WorkspaceWindowState) {
        let font_points = effective_terminal_font_points(
            active_tab(state).preset.density,
            active_tab(state).terminal_zoom_steps,
        );
        let line_height_scale = active_tab(state)
            .preset
            .density
            .terminal_line_height_scale();
        for pane in state.panes.iter_mut() {
            if !pane.font.is_null() {
                unsafe {
                    DeleteObject(pane.font as HGDIOBJ);
                }
            }
            pane.font = create_terminal_font(font_points);
            pane.line_height_scale = line_height_scale;
            update_terminal_metrics(pane);
            update_pane_scrollbar(pane);
            unsafe {
                InvalidateRect(pane.output_hwnd, ptr::null(), 1);
            }
        }
        layout_controls(hwnd, state);
    }

    fn toggle_workspace_fullscreen(hwnd: HWND, state: &mut WorkspaceWindowState) {
        if state.is_fullscreen {
            leave_workspace_fullscreen(hwnd, state);
        } else {
            enter_workspace_fullscreen(hwnd, state);
        }
    }

    fn enter_workspace_fullscreen(hwnd: HWND, state: &mut WorkspaceWindowState) {
        let mut current_rect = unsafe { mem::zeroed::<RECT>() };
        unsafe {
            GetWindowRect(hwnd, &mut current_rect);
        }
        state.saved_window_rect = current_rect;
        state.saved_window_style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };

        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        if monitor.is_null() {
            return;
        }

        let mut monitor_info = MONITORINFO {
            cbSize: mem::size_of::<MONITORINFO>() as u32,
            ..unsafe { mem::zeroed() }
        };
        if unsafe { windows_sys::Win32::Graphics::Gdi::GetMonitorInfoW(monitor, &mut monitor_info) }
            == 0
        {
            return;
        }

        unsafe {
            SetWindowLongPtrW(
                hwnd,
                GWL_STYLE,
                state.saved_window_style & !(WS_OVERLAPPEDWINDOW as isize),
            );
            SetWindowPos(
                hwnd,
                ptr::null_mut(),
                monitor_info.rcMonitor.left,
                monitor_info.rcMonitor.top,
                monitor_info.rcMonitor.right - monitor_info.rcMonitor.left,
                monitor_info.rcMonitor.bottom - monitor_info.rcMonitor.top,
                SWP_NOZORDER | SWP_FRAMECHANGED,
            );
            SetWindowTextW(state.fullscreen_hwnd, wide("Windowed").as_ptr());
        }
        state.is_fullscreen = true;
    }

    fn leave_workspace_fullscreen(hwnd: HWND, state: &mut WorkspaceWindowState) {
        unsafe {
            SetWindowLongPtrW(hwnd, GWL_STYLE, state.saved_window_style);
            SetWindowPos(
                hwnd,
                ptr::null_mut(),
                state.saved_window_rect.left,
                state.saved_window_rect.top,
                state.saved_window_rect.right - state.saved_window_rect.left,
                state.saved_window_rect.bottom - state.saved_window_rect.top,
                SWP_NOZORDER | SWP_FRAMECHANGED,
            );
            SetWindowTextW(state.fullscreen_hwnd, wide("Fullscreen").as_ptr());
        }
        state.is_fullscreen = false;
    }

    fn append_pane_output(state: &mut WorkspaceWindowState, pane_id: usize, chunk: &str) {
        let mut alert_to_push = None;
        let Some(pane) = pane_mut_by_id(state, pane_id) else {
            return;
        };

        pane.transcript.push_output(chunk);
        pane.terminal.process(chunk);
        let pending_input = pane.terminal.take_pending_input();
        if !pending_input.is_empty()
            && let Some(session) = pane.session.as_ref()
            && let Err(error) = session.write_input(&pending_input)
        {
            logging::error(format!("pane control response write failed: {error}"));
        }
        if !pending_input.is_empty() {
            pane.transcript
                .push_input(&String::from_utf8_lossy(&pending_input));
        }
        if let Some(clipboard_text) = pane.terminal.take_pending_clipboard_write()
            && let Err(error) = write_clipboard_text(&clipboard_text)
        {
            logging::error(format!("pane OSC 52 clipboard write failed: {error}"));
        }
        let recent = pane.transcript.recent_output(48);
        let matches = scan_output(&pane.tile.output_helpers, &recent);
        let (summary, severity) = helper_summary_text(&matches);
        let signature = format!("{}::{:?}", summary, severity);
        if !matches.is_empty() && pane.last_helper_signature != signature {
            pane.last_helper_signature = signature;
            let mut alert = AlertEventInput::new(
                AlertSourceKind::OutputHelper,
                match severity.unwrap_or(crate::model::assets::OutputSeverity::Info) {
                    crate::model::assets::OutputSeverity::Info => AlertSeverity::Info,
                    crate::model::assets::OutputSeverity::Warning => AlertSeverity::Warning,
                    crate::model::assets::OutputSeverity::Error => AlertSeverity::Error,
                },
                format!("{}: {}", pane.tile.title, summary),
            );
            alert.detail = recent;
            alert.pane_id = Some(pane.id.to_string());
            alert.allows_reconnect = true;
            alert_to_push = Some(alert);
        }
        update_pane_scrollbar(pane);
        unsafe {
            InvalidateRect(pane.title_hwnd, ptr::null(), 1);
            InvalidateRect(pane.output_hwnd, ptr::null(), 1);
        }
        if let Some(alert) = alert_to_push {
            push_alert(state, alert);
        }
    }

    fn mark_pane_exited(state: &mut WorkspaceWindowState, pane_id: usize) {
        let mut reconnect_schedule = None;
        let mut reconnect_alert_to_push = None;
        let Some(pane) = pane_mut_by_id(state, pane_id) else {
            return;
        };

        pane.session = None;
        pane.transcript.push_output("[terminal session exited]");
        pane.terminal
            .process("\r\n\r\n[terminal session exited]\r\n");
        let status = pane.terminal.last_command_status().unwrap_or(1);
        let mut alert = AlertEventInput::new(
            AlertSourceKind::PaneExit,
            if status == 0 {
                AlertSeverity::Info
            } else {
                AlertSeverity::Warning
            },
            format!("{} exited with status {}", pane.tile.title, status),
        );
        alert.detail = pane.transcript.recent_transcript(40);
        alert.pane_id = Some(pane.id.to_string());
        alert.allows_reconnect = true;
        let alert_to_push = Some(alert);

        if !pane.termination_requested && pane.reconnect_attempts < 3 {
            let should_reconnect = match pane.tile.reconnect_policy {
                crate::model::layout::ReconnectPolicy::Manual => false,
                crate::model::layout::ReconnectPolicy::OnAbnormalExit => status != 0,
                crate::model::layout::ReconnectPolicy::Always => true,
            };
            if should_reconnect {
                pane.reconnect_attempts = pane.reconnect_attempts.saturating_add(1);
                let attempt = pane.reconnect_attempts;
                let delay = AUTO_RECONNECT_DELAYS_SECONDS
                    .get((attempt.saturating_sub(1)) as usize)
                    .copied()
                    .unwrap_or(10);
                let window_hwnd = pane.parent_hwnd as isize;
                let pane_id = pane.id;
                let mut reconnect_alert = AlertEventInput::new(
                    AlertSourceKind::Reconnect,
                    AlertSeverity::Info,
                    format!("{} reconnect scheduled", pane.tile.title),
                );
                reconnect_alert.detail =
                    format!("Attempt {} will run in {} second(s).", attempt, delay);
                reconnect_alert.pane_id = Some(pane.id.to_string());
                reconnect_alert.allows_reconnect = true;
                reconnect_alert_to_push = Some(reconnect_alert);
                reconnect_schedule = Some((window_hwnd, pane_id, attempt, delay));
            }
        }
        update_pane_scrollbar(pane);
        unsafe {
            InvalidateRect(pane.title_hwnd, ptr::null(), 1);
            InvalidateRect(pane.output_hwnd, ptr::null(), 1);
        }
        if let Some(alert) = alert_to_push {
            push_alert(state, alert);
        }
        if let Some(alert) = reconnect_alert_to_push {
            push_alert(state, alert);
        }
        if let Some((window_hwnd, pane_id, attempt, delay)) = reconnect_schedule {
            thread::spawn(move || {
                thread::sleep(Duration::from_secs(delay));
                let _ = unsafe {
                    PostMessageW(
                        window_hwnd as HWND,
                        WM_RECONNECT_PANE,
                        pane_id,
                        attempt as LPARAM,
                    )
                };
            });
        }
    }

    fn pane_header_text(pane: &PaneState) -> String {
        if pane.tile.tile_kind == TileKind::WebView {
            if let Some(title) = pane.webview_title.as_deref() {
                return format!("{}  •  {}", pane.tile.title, title);
            }
            if let Some(uri) = pane.webview_uri.as_deref() {
                return format!("{}  •  {}", pane.tile.title, uri);
            }
            return format!("{}  •  web", pane.tile.title);
        }
        if let Some(title) = pane.terminal.window_title() {
            format!("{}  •  {}", pane.tile.title, title)
        } else if let Some(cwd) = pane.terminal.current_working_directory() {
            format!("{}  •  {}", pane.tile.title, cwd)
        } else if pane.terminal.shell_integration_phase().is_some() {
            format!("{}  •  {}", pane.tile.title, shell_status_label(pane))
        } else {
            format!("{}  •  {}", pane.tile.title, pane.tile.agent_label)
        }
    }

    fn shell_status_label(pane: &PaneState) -> String {
        let phase = pane
            .terminal
            .shell_integration_phase()
            .map(shell_phase_label)
            .unwrap_or("shell");
        let command = pane.terminal.shell_integration_command();
        let status = pane.terminal.last_command_status();

        match (phase, command, status) {
            ("running", Some(command), _) => format!("{command}  •  running"),
            ("ready", _, Some(0)) => "ready  •  exit 0".to_string(),
            ("ready", _, Some(status)) => format!("ready  •  exit {status}"),
            (_, Some(command), _) => format!("{command}  •  {phase}"),
            _ => phase.to_string(),
        }
    }

    fn shell_phase_label(phase: ShellIntegrationPhase) -> &'static str {
        match phase {
            ShellIntegrationPhase::PromptStart => "prompt",
            ShellIntegrationPhase::PromptEnd => "ready",
            ShellIntegrationPhase::CommandStart => "running",
            ShellIntegrationPhase::CommandEnd => "done",
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
                let style = resolved_style(
                    cell.style,
                    pane.terminal.hyperlink_at(row, column).is_some(),
                    selection_contains(pane, row, column),
                    false,
                );
                let start_column = column;
                while column < pane.terminal.columns() {
                    let next = pane.terminal.visible_cell(row, column);
                    if resolved_style(
                        next.style,
                        pane.terminal.hyperlink_at(row, column).is_some(),
                        selection_contains(pane, row, column),
                        false,
                    ) != style
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
                if style.underline {
                    let underline_top = (run_rect.bottom - 2).max(run_rect.top);
                    fill_rect_color(
                        hdc,
                        RECT {
                            left: run_rect.left,
                            top: underline_top,
                            right: run_rect.right,
                            bottom: (underline_top + 1).min(run_rect.bottom),
                        },
                        style.fg,
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
                pane.terminal.hyperlink_at(cursor_row, cursor_col).is_some(),
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
            if cursor_style.underline {
                let underline_top = (cursor_rect.bottom - 2).max(cursor_rect.top);
                fill_rect_color(
                    hdc,
                    RECT {
                        left: cursor_rect.left,
                        top: underline_top,
                        right: cursor_rect.right,
                        bottom: (underline_top + 1).min(cursor_rect.bottom),
                    },
                    cursor_style.fg,
                );
            }
        }

        unsafe {
            SelectObject(hdc, old_font);
            EndPaint(hwnd, &paint);
        }
    }

    fn render_pane_header(hwnd: HWND, pane: &PaneState) {
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
        let is_drop_target = unsafe { window_state_mut(pane.parent_hwnd) }
            .and_then(|state| state.pane_drag.as_ref())
            .map(|drag| drag.target_pane_id == pane.id && drag.dragged_pane_id != pane.id)
            .unwrap_or(false);
        let background = if is_drop_target {
            rgb(55, 92, 143)
        } else {
            rgb(36, 39, 48)
        };
        fill_rect_color(
            hdc,
            RECT {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            },
            background,
        );

        let old_font = unsafe { SelectObject(hdc, GetStockObject(DEFAULT_GUI_FONT)) };
        unsafe {
            SetTextColor(hdc, rgb(235, 239, 244));
            SetBkColor(hdc, background);
        }
        let text = wide_no_nul(&pane_header_text(pane));
        unsafe {
            TextOutW(
                hdc,
                8,
                ((rect.height() - 16) / 2).max(0),
                text.as_ptr(),
                text.len() as i32,
            );
            SelectObject(hdc, old_font);
            EndPaint(hwnd, &paint);
        }
    }

    fn render_tab_button(hwnd: HWND, button: &TabButtonState) {
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
        let (title, drop_state) =
            if let Some(state) = unsafe { window_state_mut(button.parent_hwnd) } {
                let title = state
                    .tabs
                    .get(button.index)
                    .map(|tab| {
                        tab.custom_title
                            .clone()
                            .unwrap_or_else(|| tab.preset.name.clone())
                    })
                    .unwrap_or_else(|| "Workspace".to_string());
                let drop_state = state.tab_drag.as_ref().and_then(|drag| {
                    (drag.target_index == button.index && drag.dragged_index != button.index)
                        .then_some(drag.insert_after)
                });
                (title, drop_state)
            } else {
                ("Workspace".to_string(), None)
            };

        let background = if button.active {
            rgb(67, 95, 132)
        } else {
            rgb(48, 51, 60)
        };
        fill_rect_color(
            hdc,
            RECT {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            },
            background,
        );
        if let Some(insert_after) = drop_state {
            let indicator_left = if insert_after {
                rect.right.saturating_sub(4)
            } else {
                rect.left
            };
            fill_rect_color(
                hdc,
                RECT {
                    left: indicator_left,
                    top: rect.top,
                    right: indicator_left + 4,
                    bottom: rect.bottom,
                },
                rgb(241, 196, 15),
            );
        }

        let old_font = unsafe { SelectObject(hdc, GetStockObject(DEFAULT_GUI_FONT)) };
        unsafe {
            SetTextColor(hdc, rgb(245, 247, 250));
            SetBkColor(hdc, background);
        }
        let text = wide_no_nul(&title);
        unsafe {
            TextOutW(
                hdc,
                10,
                ((rect.height() - 16) / 2).max(0),
                text.as_ptr(),
                text.len() as i32,
            );
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
        pane.transcript.push_input(&String::from_utf8_lossy(&bytes));
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
            pane.transcript.push_input(sequence);
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
        underline: bool,
    }

    fn resolved_style(
        style: VtStyle,
        hyperlink: bool,
        selected: bool,
        cursor: bool,
    ) -> ResolvedStyle {
        let mut fg = style.fg;
        let mut bg = style.bg;

        if style.inverse {
            std::mem::swap(&mut fg, &mut bg);
        }
        if hyperlink && matches!(fg, VtColor::DefaultForeground) {
            fg = VtColor::Indexed(12);
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
            underline: style.underline || hyperlink,
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

    fn hyperlink_at_lparam<'a>(pane: &'a PaneState, lparam: LPARAM) -> Option<&'a str> {
        let position = pane_position_from_lparam(pane, lparam);
        pane.terminal.hyperlink_at(position.row, position.column)
    }

    fn hyperlink_under_pointer<'a>(hwnd: HWND, pane: &'a PaneState) -> Option<&'a str> {
        let mut point = POINT { x: 0, y: 0 };
        unsafe {
            if GetCursorPos(&mut point) == 0 {
                return None;
            }
            ScreenToClient(hwnd, &mut point);
        }
        let lparam = ((point.y as u32) << 16 | (point.x as u32 & 0xffff)) as isize;
        hyperlink_at_lparam(pane, lparam)
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

        pane.transcript.push_input(&report);
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

        let click_position = pane_position_from_lparam(pane, lparam);
        let hyperlink = pane
            .terminal
            .hyperlink_at(click_position.row, click_position.column)
            .map(str::to_string);
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
                MF_STRING | if hyperlink.is_some() { 0 } else { MF_GRAYED },
                MENU_OPEN_LINK,
                wide("Open Link").as_ptr(),
            );
            AppendMenuW(
                menu,
                MF_STRING | if hyperlink.is_some() { 0 } else { MF_GRAYED },
                MENU_COPY_LINK,
                wide("Copy Link").as_ptr(),
            );
            AppendMenuW(
                menu,
                MF_STRING,
                MENU_PASTE_CLIPBOARD,
                wide("Paste").as_ptr(),
            );
            AppendMenuW(menu, MF_STRING, MENU_RECONNECT, wide("Reconnect").as_ptr());
            AppendMenuW(
                menu,
                MF_STRING,
                MENU_SHOW_TRANSCRIPT,
                wide("Show Transcript").as_ptr(),
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
            MENU_OPEN_LINK => {
                if let Some(link) = hyperlink.as_deref() {
                    let _ = open_url(link);
                }
            }
            MENU_COPY_LINK => {
                if let Some(link) = hyperlink.as_deref() {
                    let _ = write_clipboard_text(link);
                }
            }
            MENU_PASTE_CLIPBOARD => {
                let _ = paste_clipboard_into_pane(pane);
            }
            MENU_RECONNECT => {
                if let Some(state) = unsafe { window_state_mut(pane.parent_hwnd) } {
                    let _ = reconnect_pane(state, pane.id, None);
                }
            }
            MENU_SHOW_TRANSCRIPT => {
                show_transcript_dialog(pane.parent_hwnd, &pane.transcript.recent_transcript(240));
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
            pane.transcript.push_input(&wrapped);
            session.write_input(wrapped.as_bytes())
        } else {
            pane.transcript.push_input(&normalized);
            session.write_input(normalized.as_bytes())
        }
    }

    fn show_transcript_dialog(parent_hwnd: HWND, transcript: &str) {
        let _ = transcript_viewer::present(parent_hwnd, "Recent Transcript", transcript);
    }

    fn open_url(url: &str) -> Result<(), String> {
        let operation = wide("open");
        let target = wide(url);
        let result = unsafe {
            ShellExecuteW(
                ptr::null_mut(),
                operation.as_ptr(),
                target.as_ptr(),
                ptr::null(),
                ptr::null(),
                SW_SHOW,
            )
        };

        if result as usize <= 32 {
            Err(format!(
                "ShellExecuteW failed for hyperlink launch: {}",
                result as usize
            ))
        } else {
            Ok(())
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

    fn build_windows_command_line(command: &WindowsLaunchCommand) -> String {
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

    fn spawn_output_reader(window_hwnd: HWND, pane_id: usize, output_read: HANDLE) {
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
                let event = Box::new(PaneOutputEvent { pane_id, text });
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
            let _ = unsafe { PostMessageW(window_hwnd, WM_PANE_EXIT, pane_id, 0) };
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

    unsafe fn tab_button_state_mut(hwnd: HWND) -> Option<&'static mut TabButtonState> {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut TabButtonState;
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *ptr })
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
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

    fn wide_mut(value: &str) -> Vec<u16> {
        wide(value)
    }

    fn wide_no_nul(value: &str) -> Vec<u16> {
        value.encode_utf16().collect()
    }

    fn density_button_label(density: ApplicationDensity) -> &'static str {
        match density {
            ApplicationDensity::Comfortable => "Density: Cozy",
            ApplicationDensity::Standard => "Density: Std",
            ApplicationDensity::Compact => "Density: Tight",
        }
    }
}

#[cfg(target_os = "windows")]
pub use imp::open_saved_workspaces;
