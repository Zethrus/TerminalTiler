#[cfg(target_os = "windows")]
mod imp {
    use std::mem;
    use std::path::PathBuf;
    use std::ptr;
    use std::rc::Rc;

    use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
        ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_LEFT, ES_MULTILINE, GWLP_USERDATA, GetClientRect,
        GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, LoadCursorW,
        RegisterClassW, SW_SHOW, SWP_NOZORDER, SendMessageW, SetWindowLongPtrW, SetWindowPos,
        SetWindowTextW, ShowWindow, WINDOW_EX_STYLE, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_NCCREATE,
        WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW, WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW,
        WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
    };

    use crate::model::workspace_config::ConfigScope;
    use crate::storage::asset_store::AssetStore;

    const WINDOW_CLASS: &str = "TerminalTilerWindowsAssetsManager";
    const ID_GLOBAL_SCOPE: isize = 1001;
    const ID_WORKSPACE_SCOPE: isize = 1002;
    const ID_INFO: isize = 1003;
    const ID_TEXT: isize = 1004;
    const ID_RELOAD: isize = 1005;
    const ID_SAVE: isize = 1006;
    const ID_CLOSE: isize = 1007;
    const MARGIN: i32 = 16;
    const BUTTON_HEIGHT: i32 = 32;
    const FIELD_HEIGHT: i32 = 24;

    struct AssetsWindowState {
        asset_store: AssetStore,
        workspace_root: Option<PathBuf>,
        on_saved: Rc<dyn Fn()>,
        scope: ConfigScope,
        global_button_hwnd: HWND,
        workspace_button_hwnd: HWND,
        info_hwnd: HWND,
        text_hwnd: HWND,
        reload_hwnd: HWND,
        save_hwnd: HWND,
        close_hwnd: HWND,
    }

    pub fn present(
        parent_hwnd: HWND,
        asset_store: AssetStore,
        workspace_root: Option<PathBuf>,
        on_saved: Rc<dyn Fn()>,
    ) -> Result<(), String> {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err("could not resolve module handle for assets manager".into());
        }

        register_window_class(instance)?;
        let state = Box::new(AssetsWindowState {
            asset_store,
            workspace_root,
            on_saved,
            scope: ConfigScope::Global,
            global_button_hwnd: ptr::null_mut(),
            workspace_button_hwnd: ptr::null_mut(),
            info_hwnd: ptr::null_mut(),
            text_hwnd: ptr::null_mut(),
            reload_hwnd: ptr::null_mut(),
            save_hwnd: ptr::null_mut(),
            close_hwnd: ptr::null_mut(),
        });
        let state_ptr = Box::into_raw(state);

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                wide(WINDOW_CLASS).as_ptr(),
                wide("Assets Manager").as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                160,
                160,
                900,
                720,
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
            return Err("CreateWindowExW returned null for assets manager".into());
        }

        unsafe { ShowWindow(hwnd, SW_SHOW) };
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
                let state_ptr = unsafe { (*create).lpCreateParams as *mut AssetsWindowState };
                unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize) };
                1
            }
            WM_CREATE => {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    create_controls(hwnd, state);
                    load_scope(state);
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
                        ID_GLOBAL_SCOPE => {
                            state.scope = ConfigScope::Global;
                            load_scope(state);
                        }
                        ID_WORKSPACE_SCOPE if state.workspace_root.is_some() => {
                            state.scope = ConfigScope::Workspace;
                            load_scope(state);
                        }
                        ID_RELOAD => load_scope(state),
                        ID_SAVE => save_scope(state),
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
                    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) } as *mut AssetsWindowState;
                if !state_ptr.is_null() {
                    drop(unsafe { Box::from_raw(state_ptr) });
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    fn create_controls(hwnd: HWND, state: &mut AssetsWindowState) {
        state.global_button_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Global defaults",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_GLOBAL_SCOPE,
        );
        state.workspace_button_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Workspace overrides",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_WORKSPACE_SCOPE,
        );
        state.info_hwnd = create_child_window(hwnd, "STATIC", "", WS_CHILD | WS_VISIBLE, ID_INFO);
        state.text_hwnd = create_child_window(
            hwnd,
            "EDIT",
            "",
            WS_CHILD
                | WS_VISIBLE
                | WS_BORDER
                | WS_TABSTOP
                | WS_VSCROLL
                | ES_LEFT as u32
                | ES_MULTILINE as u32
                | ES_AUTOVSCROLL as u32
                | ES_AUTOHSCROLL as u32,
            ID_TEXT,
        );
        state.reload_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Reload",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_RELOAD,
        );
        state.save_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Save",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_SAVE,
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
            state.global_button_hwnd,
            state.workspace_button_hwnd,
            state.info_hwnd,
            state.text_hwnd,
            state.reload_hwnd,
            state.save_hwnd,
            state.close_hwnd,
        ] {
            unsafe { SendMessageW(control, WM_SETFONT, font as usize, 1) };
        }
        layout_controls(hwnd, state);
    }

    fn layout_controls(hwnd: HWND, state: &AssetsWindowState) {
        let mut rect = unsafe { mem::zeroed() };
        unsafe { GetClientRect(hwnd, &mut rect) };
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let content_width = width - (MARGIN * 2);
        let button_y = height - MARGIN - BUTTON_HEIGHT;
        let text_y = MARGIN + BUTTON_HEIGHT + 44;
        let text_height = (button_y - text_y - 12).max(220);
        unsafe {
            SetWindowPos(
                state.global_button_hwnd,
                ptr::null_mut(),
                MARGIN,
                MARGIN,
                120,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.workspace_button_hwnd,
                ptr::null_mut(),
                MARGIN + 128,
                MARGIN,
                140,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.info_hwnd,
                ptr::null_mut(),
                MARGIN,
                MARGIN + BUTTON_HEIGHT + 10,
                content_width,
                FIELD_HEIGHT + 20,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.text_hwnd,
                ptr::null_mut(),
                MARGIN,
                text_y,
                content_width,
                text_height,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.reload_hwnd,
                ptr::null_mut(),
                MARGIN,
                button_y,
                96,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.save_hwnd,
                ptr::null_mut(),
                MARGIN + 104,
                button_y,
                96,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.close_hwnd,
                ptr::null_mut(),
                width - MARGIN - 96,
                button_y,
                96,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
        }
    }

    fn load_scope(state: &AssetsWindowState) {
        let (assets, info_text) = match state.scope {
            ConfigScope::Global => (
                state.asset_store.load_assets(),
                "Editing global defaults from ~/.config/TerminalTiler/workspace-assets.toml. These assets are shared by every workspace."
                    .to_string(),
            ),
            ConfigScope::Workspace => {
                if let Some(workspace_root) = state.workspace_root.as_ref() {
                    (
                        state
                            .asset_store
                            .load_workspace_config(workspace_root)
                            .assets,
                        format!(
                            "Editing workspace overrides from {}/.terminaltiler/workspace.toml. Matching IDs shadow the global definitions in this workspace only.",
                            workspace_root.display()
                        ),
                    )
                } else {
                    (
                        crate::model::assets::WorkspaceAssets::default(),
                        "Workspace overrides are unavailable until a workspace root is selected."
                            .to_string(),
                    )
                }
            }
        };
        let serialized =
            toml::to_string_pretty(&assets).unwrap_or_else(|_| "# serialization failed\n".into());
        unsafe {
            SetWindowTextW(state.info_hwnd, wide(&info_text).as_ptr());
            SetWindowTextW(state.text_hwnd, wide(&serialized).as_ptr());
            SetWindowTextW(
                state.global_button_hwnd,
                wide(if state.scope == ConfigScope::Global {
                    "Global defaults *"
                } else {
                    "Global defaults"
                })
                .as_ptr(),
            );
            SetWindowTextW(
                state.workspace_button_hwnd,
                wide(if state.scope == ConfigScope::Workspace {
                    "Workspace overrides *"
                } else {
                    "Workspace overrides"
                })
                .as_ptr(),
            );
        }
    }

    fn save_scope(state: &AssetsWindowState) {
        let raw = read_window_text(state.text_hwnd);
        match toml::from_str::<crate::model::assets::WorkspaceAssets>(&raw) {
            Ok(assets) => match state.asset_store.save_assets_for_scope(
                &assets,
                state.scope,
                state.workspace_root.as_deref(),
            ) {
                Ok(()) => {
                    unsafe {
                        SetWindowTextW(state.info_hwnd, wide("Assets saved successfully.").as_ptr())
                    };
                    (state.on_saved)();
                }
                Err(error) => unsafe {
                    SetWindowTextW(
                        state.info_hwnd,
                        wide(&format!("Failed to save assets: {error}")).as_ptr(),
                    );
                },
            },
            Err(error) => unsafe {
                SetWindowTextW(
                    state.info_hwnd,
                    wide(&format!("Failed to parse assets TOML: {error}")).as_ptr(),
                );
            },
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
                return Err(format!("RegisterClassW failed for assets manager: {error}"));
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

    unsafe fn state_mut(hwnd: HWND) -> Option<&'static mut AssetsWindowState> {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut AssetsWindowState;
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
