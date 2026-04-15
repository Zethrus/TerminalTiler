#[cfg(target_os = "windows")]
mod imp {
    use std::mem;
    use std::ptr;
    use std::rc::Rc;

    use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BN_CLICKED, CB_ADDSTRING, CB_GETCURSEL, CB_RESETCONTENT, CB_SETCURSEL, CBN_SELCHANGE,
        CBS_DROPDOWNLIST, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW,
        DestroyWindow, EN_CHANGE, ES_AUTOHSCROLL, ES_LEFT, GWLP_USERDATA, GetClientRect,
        GetDlgItem, GetParent, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HMENU,
        IDC_ARROW, LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL, LBN_SELCHANGE,
        LoadCursorW, RegisterClassW, SW_SHOW, SWP_NOZORDER, SendMessageW, SetForegroundWindow,
        SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, WINDOW_EX_STYLE, WM_CLOSE,
        WM_COMMAND, WM_CREATE, WM_NCCREATE, WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW,
        WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
    };

    use crate::model::assets::{TileConnectionTarget, WorkspaceAssets};
    use crate::model::layout::{
        DEFAULT_WEB_URL, LayoutNode, ReconnectPolicy, SplitAxis, TileKind, TileSpec,
        normalize_web_url,
    };
    use crate::services::layout_editor::{close_tile, split_tile, split_tile_with_kind};
    use crate::services::tile_draft::apply_role_to_tile;

    const WINDOW_CLASS: &str = "TerminalTilerWindowsLauncherEditor";
    const ID_TILE_LIST: isize = 1001;
    const ID_TITLE: isize = 1002;
    const ID_AGENT: isize = 1003;
    const ID_STARTUP: isize = 1004;
    const ID_ROLE: isize = 1005;
    const ID_CONNECTION: isize = 1006;
    const ID_GROUPS: isize = 1007;
    const ID_RECONNECT: isize = 1008;
    const ID_HINT: isize = 1009;
    const ID_SPLIT_HORIZONTAL: isize = 1010;
    const ID_SPLIT_VERTICAL: isize = 1011;
    const ID_CLONE_TILE: isize = 1012;
    const ID_CLOSE_TILE: isize = 1013;
    const ID_CLOSE_WINDOW: isize = 1014;
    const ID_LABEL_TITLE: isize = 1015;
    const ID_LABEL_AGENT: isize = 1016;
    const ID_LABEL_STARTUP: isize = 1017;
    const ID_LABEL_ROLE: isize = 1018;
    const ID_LABEL_CONNECTION: isize = 1019;
    const ID_LABEL_GROUPS: isize = 1020;
    const ID_LABEL_RECONNECT: isize = 1021;
    const ID_KIND: isize = 1022;
    const ID_URL: isize = 1023;
    const ID_AUTO_REFRESH: isize = 1024;
    const ID_LABEL_KIND: isize = 1025;
    const ID_LABEL_URL: isize = 1026;
    const ID_LABEL_AUTO_REFRESH: isize = 1027;
    const ID_SPLIT_WEB: isize = 1028;
    const MARGIN: i32 = 16;
    const BUTTON_HEIGHT: i32 = 32;
    const FIELD_HEIGHT: i32 = 28;

    struct EditorWindowState {
        layout: LayoutNode,
        assets: WorkspaceAssets,
        on_layout_changed: Rc<dyn Fn(LayoutNode)>,
        on_closed: Rc<dyn Fn()>,
        selected_tile_index: usize,
        syncing_controls: bool,
        tile_list_hwnd: HWND,
        title_hwnd: HWND,
        kind_hwnd: HWND,
        agent_hwnd: HWND,
        startup_hwnd: HWND,
        url_hwnd: HWND,
        auto_refresh_hwnd: HWND,
        role_hwnd: HWND,
        connection_hwnd: HWND,
        groups_hwnd: HWND,
        reconnect_hwnd: HWND,
        hint_hwnd: HWND,
        split_horizontal_hwnd: HWND,
        split_vertical_hwnd: HWND,
        split_web_hwnd: HWND,
        clone_hwnd: HWND,
        close_tile_hwnd: HWND,
        close_window_hwnd: HWND,
    }

    pub fn present(
        parent_hwnd: HWND,
        layout: LayoutNode,
        assets: WorkspaceAssets,
        on_layout_changed: Rc<dyn Fn(LayoutNode)>,
        on_closed: Rc<dyn Fn()>,
    ) -> Result<HWND, String> {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err("could not resolve module handle for launcher editor".into());
        }

        register_window_class(instance)?;
        let state = Box::new(EditorWindowState {
            layout,
            assets,
            on_layout_changed,
            on_closed,
            selected_tile_index: 0,
            syncing_controls: false,
            tile_list_hwnd: ptr::null_mut(),
            title_hwnd: ptr::null_mut(),
            kind_hwnd: ptr::null_mut(),
            agent_hwnd: ptr::null_mut(),
            startup_hwnd: ptr::null_mut(),
            url_hwnd: ptr::null_mut(),
            auto_refresh_hwnd: ptr::null_mut(),
            role_hwnd: ptr::null_mut(),
            connection_hwnd: ptr::null_mut(),
            groups_hwnd: ptr::null_mut(),
            reconnect_hwnd: ptr::null_mut(),
            hint_hwnd: ptr::null_mut(),
            split_horizontal_hwnd: ptr::null_mut(),
            split_vertical_hwnd: ptr::null_mut(),
            split_web_hwnd: ptr::null_mut(),
            clone_hwnd: ptr::null_mut(),
            close_tile_hwnd: ptr::null_mut(),
            close_window_hwnd: ptr::null_mut(),
        });
        let state_ptr = Box::into_raw(state);

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                wide(WINDOW_CLASS).as_ptr(),
                wide("Edit Tiles").as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                180,
                180,
                980,
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
            return Err("CreateWindowExW returned null for launcher editor".into());
        }

        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }
        Ok(hwnd)
    }

    pub fn sync_draft_state(hwnd: HWND, layout: LayoutNode, assets: WorkspaceAssets) {
        if let Some(state) = unsafe { state_mut(hwnd) } {
            state.layout = layout;
            state.assets = assets;
            state.selected_tile_index = state
                .selected_tile_index
                .min(state.layout.tile_count().saturating_sub(1));
            refresh_editor(state);
        }
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
                let state_ptr = unsafe { (*create).lpCreateParams as *mut EditorWindowState };
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
                }
                1
            }
            WM_CREATE => {
                if let Some(state) = unsafe { state_mut(hwnd) } {
                    create_controls(hwnd, state);
                    refresh_editor(state);
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
                    if state.syncing_controls {
                        return 0;
                    }
                    match command_id {
                        ID_TILE_LIST if notification == LBN_SELCHANGE => {
                            state.selected_tile_index =
                                selected_listbox_index(state.tile_list_hwnd)
                                    .min(state.layout.tile_count().saturating_sub(1));
                            refresh_editor(state);
                        }
                        ID_TITLE if notification == EN_CHANGE => {
                            let value = read_window_text(state.title_hwnd);
                            mutate_selected_tile(state, |tile| {
                                tile.title = value;
                            });
                            refresh_tile_list(state);
                        }
                        ID_KIND if notification == CBN_SELCHANGE => {
                            let selected = selected_combo_index(state.kind_hwnd);
                            mutate_selected_tile(state, |tile| {
                                tile.tile_kind = if selected == 1 {
                                    TileKind::WebView
                                } else {
                                    TileKind::Terminal
                                };
                                match tile.tile_kind {
                                    TileKind::WebView => {
                                        if tile.url.is_none() {
                                            tile.url = Some(DEFAULT_WEB_URL.into());
                                        }
                                        tile.auto_refresh_seconds = None;
                                        tile.startup_command = None;
                                        tile.applied_role_id = None;
                                        tile.connection_target = TileConnectionTarget::Local;
                                        tile.output_helpers.clear();
                                        if tile.agent_label.trim().is_empty() {
                                            tile.agent_label = "Web".into();
                                        }
                                    }
                                    TileKind::Terminal => {
                                        tile.url = None;
                                        tile.auto_refresh_seconds = None;
                                    }
                                }
                            });
                            refresh_editor(state);
                        }
                        ID_AGENT if notification == EN_CHANGE => {
                            let value = read_window_text(state.agent_hwnd);
                            mutate_selected_tile(state, |tile| {
                                tile.agent_label = value;
                                if tile.applied_role_id.is_none() {
                                    tile.accent_class = accent_class_for_agent(&tile.agent_label);
                                }
                            });
                            refresh_tile_list(state);
                            refresh_hint(state);
                        }
                        ID_STARTUP if notification == EN_CHANGE => {
                            let value = read_window_text(state.startup_hwnd);
                            mutate_selected_tile(state, |tile| {
                                let value = value.trim().to_string();
                                tile.startup_command =
                                    if value.is_empty() { None } else { Some(value) };
                            });
                        }
                        ID_URL if notification == EN_CHANGE => {
                            let value = read_window_text(state.url_hwnd).trim().to_string();
                            mutate_selected_tile(state, |tile| {
                                tile.url = if value.is_empty() {
                                    None
                                } else {
                                    Some(normalize_web_url(&value))
                                };
                            });
                            refresh_hint(state);
                        }
                        ID_AUTO_REFRESH if notification == EN_CHANGE => {
                            let value = read_window_text(state.auto_refresh_hwnd);
                            mutate_selected_tile(state, |tile| {
                                tile.auto_refresh_seconds = value.trim().parse::<u32>().ok();
                            });
                            refresh_hint(state);
                        }
                        ID_GROUPS if notification == EN_CHANGE => {
                            let value = read_window_text(state.groups_hwnd);
                            mutate_selected_tile(state, |tile| {
                                tile.pane_groups = parse_groups(&value);
                            });
                            refresh_hint(state);
                        }
                        ID_ROLE if notification == CBN_SELCHANGE => {
                            let selected = selected_combo_index(state.role_hwnd);
                            let selected_role = state
                                .assets
                                .role_templates
                                .get(selected.saturating_sub(1))
                                .cloned();
                            mutate_selected_tile(state, |tile| {
                                if selected == 0 {
                                    tile.applied_role_id = None;
                                    tile.accent_class = accent_class_for_agent(&tile.agent_label);
                                } else if let Some(role) = selected_role.as_ref() {
                                    apply_role_to_tile(tile, role);
                                }
                            });
                            refresh_editor(state);
                        }
                        ID_CONNECTION if notification == CBN_SELCHANGE => {
                            let selected = selected_combo_index(state.connection_hwnd);
                            let selected_profile_id = state
                                .assets
                                .connection_profiles
                                .get(selected.saturating_sub(1))
                                .map(|profile| profile.id.clone());
                            mutate_selected_tile(state, |tile| {
                                tile.connection_target = if selected == 0 {
                                    TileConnectionTarget::Local
                                } else {
                                    selected_profile_id
                                        .as_ref()
                                        .map(|profile_id| {
                                            TileConnectionTarget::Profile(profile_id.clone())
                                        })
                                        .unwrap_or(TileConnectionTarget::Local)
                                };
                            });
                            refresh_hint(state);
                        }
                        ID_RECONNECT if notification == CBN_SELCHANGE => {
                            let selected = selected_combo_index(state.reconnect_hwnd);
                            mutate_selected_tile(state, |tile| {
                                tile.reconnect_policy = reconnect_policy_from_index(selected);
                            });
                            refresh_hint(state);
                        }
                        ID_SPLIT_HORIZONTAL if notification == BN_CLICKED => {
                            mutate_layout_structure(state, SplitAxis::Horizontal, false);
                        }
                        ID_SPLIT_VERTICAL if notification == BN_CLICKED => {
                            mutate_layout_structure(state, SplitAxis::Vertical, false);
                        }
                        ID_SPLIT_WEB if notification == BN_CLICKED => {
                            mutate_layout_structure_with_kind(
                                state,
                                SplitAxis::Horizontal,
                                false,
                                TileKind::WebView,
                            );
                        }
                        ID_CLONE_TILE if notification == BN_CLICKED => {
                            mutate_layout_structure(state, SplitAxis::Horizontal, true);
                        }
                        ID_CLOSE_TILE if notification == BN_CLICKED => {
                            close_selected_tile(state);
                        }
                        ID_CLOSE_WINDOW if notification == BN_CLICKED => unsafe {
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
                    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) } as *mut EditorWindowState;
                if !state_ptr.is_null() {
                    let state = unsafe { Box::from_raw(state_ptr) };
                    (state.on_closed)();
                }
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    fn create_controls(hwnd: HWND, state: &mut EditorWindowState) {
        state.tile_list_hwnd = create_child_window(
            hwnd,
            "LISTBOX",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | WS_VSCROLL,
            ID_TILE_LIST,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Title",
            WS_CHILD | WS_VISIBLE,
            ID_LABEL_TITLE,
        );
        state.title_hwnd = create_child_window(
            hwnd,
            "EDIT",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_LEFT as u32 | ES_AUTOHSCROLL as u32,
            ID_TITLE,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Tile kind",
            WS_CHILD | WS_VISIBLE,
            ID_LABEL_KIND,
        );
        state.kind_hwnd = create_combo_box(hwnd, ID_KIND);
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Agent label",
            WS_CHILD | WS_VISIBLE,
            ID_LABEL_AGENT,
        );
        state.agent_hwnd = create_child_window(
            hwnd,
            "EDIT",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_LEFT as u32 | ES_AUTOHSCROLL as u32,
            ID_AGENT,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Startup command",
            WS_CHILD | WS_VISIBLE,
            ID_LABEL_STARTUP,
        );
        state.startup_hwnd = create_child_window(
            hwnd,
            "EDIT",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_LEFT as u32 | ES_AUTOHSCROLL as u32,
            ID_STARTUP,
        );
        let _ = create_child_window(hwnd, "STATIC", "URL", WS_CHILD | WS_VISIBLE, ID_LABEL_URL);
        state.url_hwnd = create_child_window(
            hwnd,
            "EDIT",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_LEFT as u32 | ES_AUTOHSCROLL as u32,
            ID_URL,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Auto refresh (s)",
            WS_CHILD | WS_VISIBLE,
            ID_LABEL_AUTO_REFRESH,
        );
        state.auto_refresh_hwnd = create_child_window(
            hwnd,
            "EDIT",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_LEFT as u32 | ES_AUTOHSCROLL as u32,
            ID_AUTO_REFRESH,
        );
        let _ = create_child_window(hwnd, "STATIC", "Role", WS_CHILD | WS_VISIBLE, ID_LABEL_ROLE);
        state.role_hwnd = create_combo_box(hwnd, ID_ROLE);
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Connection",
            WS_CHILD | WS_VISIBLE,
            ID_LABEL_CONNECTION,
        );
        state.connection_hwnd = create_combo_box(hwnd, ID_CONNECTION);
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Pane groups",
            WS_CHILD | WS_VISIBLE,
            ID_LABEL_GROUPS,
        );
        state.groups_hwnd = create_child_window(
            hwnd,
            "EDIT",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_LEFT as u32 | ES_AUTOHSCROLL as u32,
            ID_GROUPS,
        );
        let _ = create_child_window(
            hwnd,
            "STATIC",
            "Reconnect",
            WS_CHILD | WS_VISIBLE,
            ID_LABEL_RECONNECT,
        );
        state.reconnect_hwnd = create_combo_box(hwnd, ID_RECONNECT);
        state.hint_hwnd = create_child_window(hwnd, "STATIC", "", WS_CHILD | WS_VISIBLE, ID_HINT);
        state.split_horizontal_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Split Horizontal",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_SPLIT_HORIZONTAL,
        );
        state.split_vertical_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Split Vertical",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_SPLIT_VERTICAL,
        );
        state.split_web_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Split Web Tile",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_SPLIT_WEB,
        );
        state.clone_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Clone Tile",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_CLONE_TILE,
        );
        state.close_tile_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Close Tile",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_CLOSE_TILE,
        );
        state.close_window_hwnd = create_child_window(
            hwnd,
            "BUTTON",
            "Close",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            ID_CLOSE_WINDOW,
        );

        let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
        for control in [
            state.tile_list_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_TITLE as i32) },
            state.title_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_KIND as i32) },
            state.kind_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_AGENT as i32) },
            state.agent_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_STARTUP as i32) },
            state.startup_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_URL as i32) },
            state.url_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_AUTO_REFRESH as i32) },
            state.auto_refresh_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_ROLE as i32) },
            state.role_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_CONNECTION as i32) },
            state.connection_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_GROUPS as i32) },
            state.groups_hwnd,
            unsafe { GetDlgItem(hwnd, ID_LABEL_RECONNECT as i32) },
            state.reconnect_hwnd,
            state.hint_hwnd,
            state.split_horizontal_hwnd,
            state.split_vertical_hwnd,
            state.split_web_hwnd,
            state.clone_hwnd,
            state.close_tile_hwnd,
            state.close_window_hwnd,
        ] {
            unsafe {
                SendMessageW(control, WM_SETFONT, font as usize, 1);
            }
        }
        populate_tile_kind_choices(state.kind_hwnd);
        populate_reconnect_choices(state.reconnect_hwnd);
        layout_controls(hwnd, state);
    }

    fn layout_controls(hwnd: HWND, state: &EditorWindowState) {
        let mut rect = unsafe { mem::zeroed() };
        unsafe {
            GetClientRect(hwnd, &mut rect);
        }
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let list_width = (width / 3).max(240);
        let detail_x = MARGIN + list_width + 16;
        let detail_width = width - detail_x - MARGIN;
        let button_y = height - MARGIN - BUTTON_HEIGHT;
        let label_width = 120;
        let field_x = detail_x + label_width + 8;
        let field_width = (detail_width - label_width - 8).max(180);

        let title_y = MARGIN;
        let kind_y = title_y + FIELD_HEIGHT + 18;
        let agent_y = kind_y + FIELD_HEIGHT + 18;
        let startup_y = agent_y + FIELD_HEIGHT + 18;
        let url_y = startup_y + FIELD_HEIGHT + 18;
        let auto_refresh_y = url_y + FIELD_HEIGHT + 18;
        let role_y = auto_refresh_y + FIELD_HEIGHT + 18;
        let connection_y = role_y + FIELD_HEIGHT + 18;
        let groups_y = connection_y + FIELD_HEIGHT + 18;
        let reconnect_y = groups_y + FIELD_HEIGHT + 18;
        let hint_y = reconnect_y + FIELD_HEIGHT + 20;
        let hint_height = 56;

        unsafe {
            SetWindowPos(
                state.tile_list_hwnd,
                ptr::null_mut(),
                MARGIN,
                MARGIN,
                list_width,
                button_y - MARGIN - 12,
                SWP_NOZORDER,
            );
            position_label_and_field(
                hwnd,
                ID_LABEL_TITLE,
                state.title_hwnd,
                detail_x,
                field_x,
                field_width,
                title_y,
            );
            position_label_and_field(
                hwnd,
                ID_LABEL_KIND,
                state.kind_hwnd,
                detail_x,
                field_x,
                field_width,
                kind_y,
            );
            position_label_and_field(
                hwnd,
                ID_LABEL_AGENT,
                state.agent_hwnd,
                detail_x,
                field_x,
                field_width,
                agent_y,
            );
            position_label_and_field(
                hwnd,
                ID_LABEL_STARTUP,
                state.startup_hwnd,
                detail_x,
                field_x,
                field_width,
                startup_y,
            );
            position_label_and_field(
                hwnd,
                ID_LABEL_URL,
                state.url_hwnd,
                detail_x,
                field_x,
                field_width,
                url_y,
            );
            position_label_and_field(
                hwnd,
                ID_LABEL_AUTO_REFRESH,
                state.auto_refresh_hwnd,
                detail_x,
                field_x,
                field_width,
                auto_refresh_y,
            );
            position_label_and_field(
                hwnd,
                ID_LABEL_ROLE,
                state.role_hwnd,
                detail_x,
                field_x,
                field_width,
                role_y,
            );
            position_label_and_field(
                hwnd,
                ID_LABEL_CONNECTION,
                state.connection_hwnd,
                detail_x,
                field_x,
                field_width,
                connection_y,
            );
            position_label_and_field(
                hwnd,
                ID_LABEL_GROUPS,
                state.groups_hwnd,
                detail_x,
                field_x,
                field_width,
                groups_y,
            );
            position_label_and_field(
                hwnd,
                ID_LABEL_RECONNECT,
                state.reconnect_hwnd,
                detail_x,
                field_x,
                field_width,
                reconnect_y,
            );
            SetWindowPos(
                state.hint_hwnd,
                ptr::null_mut(),
                detail_x,
                hint_y,
                detail_width,
                hint_height,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.split_horizontal_hwnd,
                ptr::null_mut(),
                detail_x,
                button_y,
                132,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.split_vertical_hwnd,
                ptr::null_mut(),
                detail_x + 140,
                button_y,
                124,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.clone_hwnd,
                ptr::null_mut(),
                detail_x + 272,
                button_y,
                108,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.split_web_hwnd,
                ptr::null_mut(),
                detail_x + 388,
                button_y,
                120,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.close_tile_hwnd,
                ptr::null_mut(),
                detail_x + 516,
                button_y,
                108,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.close_window_hwnd,
                ptr::null_mut(),
                width - MARGIN - 96,
                button_y,
                96,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
        }
    }

    fn position_label_and_field(
        hwnd: HWND,
        label_id: isize,
        field_hwnd: HWND,
        label_x: i32,
        field_x: i32,
        field_width: i32,
        y: i32,
    ) {
        unsafe {
            SetWindowPos(
                GetDlgItem(hwnd, label_id as i32),
                ptr::null_mut(),
                label_x,
                y + 4,
                112,
                18,
                SWP_NOZORDER,
            );
            SetWindowPos(
                field_hwnd,
                ptr::null_mut(),
                field_x,
                y,
                field_width,
                FIELD_HEIGHT,
                SWP_NOZORDER,
            );
        }
    }

    fn refresh_editor(state: &mut EditorWindowState) {
        state.syncing_controls = true;
        refresh_tile_list(state);
        populate_role_choices(state.role_hwnd, &state.assets);
        populate_connection_choices(state.connection_hwnd, &state.assets);
        sync_selected_tile_controls(state);
        state.syncing_controls = false;
    }

    fn refresh_tile_list(state: &EditorWindowState) {
        unsafe {
            SendMessageW(state.tile_list_hwnd, LB_RESETCONTENT, 0, 0);
        }
        for (index, tile) in state.layout.tile_specs().iter().enumerate() {
            let label = format!(
                "Tile {}  •  {}  •  {}",
                index + 1,
                tile_kind_label(tile.tile_kind),
                tile.title,
            );
            unsafe {
                SendMessageW(
                    state.tile_list_hwnd,
                    LB_ADDSTRING,
                    0,
                    wide(&label).as_ptr() as LPARAM,
                );
            }
        }
        unsafe {
            SendMessageW(
                state.tile_list_hwnd,
                LB_SETCURSEL,
                state
                    .selected_tile_index
                    .min(state.layout.tile_count().saturating_sub(1)),
                0,
            );
        }
    }

    fn sync_selected_tile_controls(state: &mut EditorWindowState) {
        let tiles = state.layout.tile_specs();
        let Some(tile) = tiles.get(state.selected_tile_index) else {
            return;
        };

        unsafe {
            SetWindowTextW(state.title_hwnd, wide(&tile.title).as_ptr());
            SetWindowTextW(
                state.url_hwnd,
                wide(tile.url.as_deref().unwrap_or(DEFAULT_WEB_URL)).as_ptr(),
            );
            SetWindowTextW(
                state.auto_refresh_hwnd,
                wide(
                    &tile
                        .auto_refresh_seconds
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                )
                .as_ptr(),
            );
            SetWindowTextW(state.agent_hwnd, wide(&tile.agent_label).as_ptr());
            SetWindowTextW(
                state.startup_hwnd,
                wide(tile.startup_command.as_deref().unwrap_or("")).as_ptr(),
            );
            SetWindowTextW(
                state.groups_hwnd,
                wide(&tile.pane_groups.join(", ")).as_ptr(),
            );
            EnableWindow(
                state.close_tile_hwnd,
                (state.layout.tile_count() > 1) as i32,
            );
        }

        select_combo_index(
            state.kind_hwnd,
            usize::from(tile.tile_kind == TileKind::WebView),
        );

        select_combo_index(
            state.role_hwnd,
            tile.applied_role_id
                .as_deref()
                .and_then(|role_id| {
                    state
                        .assets
                        .role_templates
                        .iter()
                        .position(|role| role.id == role_id)
                })
                .map(|index| index + 1)
                .unwrap_or(0),
        );
        select_combo_index(
            state.connection_hwnd,
            match &tile.connection_target {
                TileConnectionTarget::Local => 0,
                TileConnectionTarget::Profile(profile_id) => state
                    .assets
                    .connection_profiles
                    .iter()
                    .position(|profile| profile.id == *profile_id)
                    .map(|index| index + 1)
                    .unwrap_or(0),
            },
        );
        select_combo_index(
            state.reconnect_hwnd,
            reconnect_policy_index(tile.reconnect_policy),
        );
        sync_control_visibility(state, tile.tile_kind);
        refresh_hint(state);
    }

    fn refresh_hint(state: &EditorWindowState) {
        let text = state
            .layout
            .tile_specs()
            .get(state.selected_tile_index)
            .map(|tile| tile_editor_hint(tile, &state.assets))
            .unwrap_or_else(|| "No tile selected.".into());
        unsafe {
            SetWindowTextW(state.hint_hwnd, wide(&text).as_ptr());
        }
    }

    fn mutate_selected_tile<F>(state: &mut EditorWindowState, update: F)
    where
        F: FnOnce(&mut TileSpec),
    {
        let mut tiles = state.layout.tile_specs();
        if let Some(tile) = tiles.get_mut(state.selected_tile_index) {
            update(tile);
            state.layout = state.layout.with_tile_specs(&tiles);
            let next_layout = state.layout.clone();
            (state.on_layout_changed)(next_layout);
        }
    }

    fn mutate_layout_structure(
        state: &mut EditorWindowState,
        axis: SplitAxis,
        clone_existing: bool,
    ) {
        let Some(tile_id) = state
            .layout
            .tile_specs()
            .get(state.selected_tile_index)
            .map(|tile| tile.id.clone())
        else {
            return;
        };
        if let Some(next_layout) = split_tile(&state.layout, &tile_id, axis, clone_existing) {
            state.layout = next_layout;
            refresh_editor(state);
            let next_layout = state.layout.clone();
            (state.on_layout_changed)(next_layout);
        }
    }

    fn mutate_layout_structure_with_kind(
        state: &mut EditorWindowState,
        axis: SplitAxis,
        clone_existing: bool,
        tile_kind: TileKind,
    ) {
        let Some(tile_id) = state
            .layout
            .tile_specs()
            .get(state.selected_tile_index)
            .map(|tile| tile.id.clone())
        else {
            return;
        };
        if let Some((next_layout, new_tile_id)) =
            split_tile_with_kind(&state.layout, &tile_id, axis, clone_existing, tile_kind)
        {
            state.layout = next_layout;
            state.selected_tile_index = state
                .layout
                .tile_specs()
                .iter()
                .position(|tile| tile.id == new_tile_id)
                .unwrap_or(state.selected_tile_index);
            refresh_editor(state);
            let next_layout = state.layout.clone();
            (state.on_layout_changed)(next_layout);
        }
    }

    fn close_selected_tile(state: &mut EditorWindowState) {
        let Some(tile_id) = state
            .layout
            .tile_specs()
            .get(state.selected_tile_index)
            .map(|tile| tile.id.clone())
        else {
            return;
        };
        if let Some(next_layout) = close_tile(&state.layout, &tile_id) {
            state.layout = next_layout;
            state.selected_tile_index = state
                .selected_tile_index
                .min(state.layout.tile_count().saturating_sub(1));
            refresh_editor(state);
            let next_layout = state.layout.clone();
            (state.on_layout_changed)(next_layout);
        }
    }

    fn populate_role_choices(hwnd: HWND, assets: &WorkspaceAssets) {
        unsafe {
            SendMessageW(hwnd, CB_RESETCONTENT, 0, 0);
            SendMessageW(hwnd, CB_ADDSTRING, 0, wide("No role").as_ptr() as LPARAM);
        }
        for role in &assets.role_templates {
            unsafe {
                SendMessageW(hwnd, CB_ADDSTRING, 0, wide(&role.name).as_ptr() as LPARAM);
            }
        }
    }

    fn populate_connection_choices(hwnd: HWND, assets: &WorkspaceAssets) {
        unsafe {
            SendMessageW(hwnd, CB_RESETCONTENT, 0, 0);
            SendMessageW(
                hwnd,
                CB_ADDSTRING,
                0,
                wide("Default local target").as_ptr() as LPARAM,
            );
        }
        for profile in &assets.connection_profiles {
            unsafe {
                SendMessageW(
                    hwnd,
                    CB_ADDSTRING,
                    0,
                    wide(&profile.name).as_ptr() as LPARAM,
                );
            }
        }
    }

    fn populate_reconnect_choices(hwnd: HWND) {
        unsafe {
            SendMessageW(hwnd, CB_RESETCONTENT, 0, 0);
        }
        for label in [
            ReconnectPolicy::Manual.label(),
            ReconnectPolicy::OnAbnormalExit.label(),
            ReconnectPolicy::Always.label(),
        ] {
            unsafe {
                SendMessageW(hwnd, CB_ADDSTRING, 0, wide(label).as_ptr() as LPARAM);
            }
        }
        select_combo_index(hwnd, 0);
    }

    fn populate_tile_kind_choices(hwnd: HWND) {
        unsafe {
            SendMessageW(hwnd, CB_RESETCONTENT, 0, 0);
            SendMessageW(hwnd, CB_ADDSTRING, 0, wide("Terminal").as_ptr() as LPARAM);
            SendMessageW(hwnd, CB_ADDSTRING, 0, wide("Web View").as_ptr() as LPARAM);
        }
        select_combo_index(hwnd, 0);
    }

    fn reconnect_policy_index(policy: ReconnectPolicy) -> usize {
        match policy {
            ReconnectPolicy::Manual => 0,
            ReconnectPolicy::OnAbnormalExit => 1,
            ReconnectPolicy::Always => 2,
        }
    }

    fn reconnect_policy_from_index(index: usize) -> ReconnectPolicy {
        match index {
            1 => ReconnectPolicy::OnAbnormalExit,
            2 => ReconnectPolicy::Always,
            _ => ReconnectPolicy::Manual,
        }
    }

    fn parse_groups(input: &str) -> Vec<String> {
        input
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect()
    }

    fn tile_editor_hint(tile: &TileSpec, assets: &WorkspaceAssets) -> String {
        if tile.tile_kind == TileKind::WebView {
            return format!(
                "Kind: Web View  •  URL: {}  •  Auto refresh: {}",
                tile.url.as_deref().unwrap_or(DEFAULT_WEB_URL),
                tile.auto_refresh_seconds
                    .map(|value| format!("{value}s"))
                    .unwrap_or_else(|| "off".into())
            );
        }

        let role_label = tile
            .applied_role_id
            .as_deref()
            .and_then(|role_id| assets.role_templates.iter().find(|role| role.id == role_id))
            .map(|role| role.name.clone())
            .unwrap_or_else(|| "No role".into());
        let connection_label = match &tile.connection_target {
            TileConnectionTarget::Local => "Default local target".into(),
            TileConnectionTarget::Profile(profile_id) => assets
                .connection_profiles
                .iter()
                .find(|profile| profile.id == *profile_id)
                .map(|profile| profile.name.clone())
                .unwrap_or_else(|| format!("Missing profile: {profile_id}")),
        };
        format!(
            "Working directory: {}  •  Role: {}  •  Connection: {}  •  Reconnect: {}",
            tile.working_directory.short_label(),
            role_label,
            connection_label,
            tile.reconnect_policy.label()
        )
    }

    fn tile_kind_label(tile_kind: TileKind) -> &'static str {
        match tile_kind {
            TileKind::Terminal => "Terminal",
            TileKind::WebView => "Web View",
        }
    }

    fn sync_control_visibility(state: &EditorWindowState, tile_kind: TileKind) {
        let parent_hwnd = unsafe { GetParent(state.tile_list_hwnd) };
        let show_web = tile_kind == TileKind::WebView;
        for control in [
            unsafe { GetDlgItem(parent_hwnd, ID_LABEL_AGENT as i32) },
            state.agent_hwnd,
            unsafe { GetDlgItem(parent_hwnd, ID_LABEL_STARTUP as i32) },
            state.startup_hwnd,
            unsafe { GetDlgItem(parent_hwnd, ID_LABEL_ROLE as i32) },
            state.role_hwnd,
            unsafe { GetDlgItem(parent_hwnd, ID_LABEL_CONNECTION as i32) },
            state.connection_hwnd,
            unsafe { GetDlgItem(parent_hwnd, ID_LABEL_GROUPS as i32) },
            state.groups_hwnd,
            unsafe { GetDlgItem(parent_hwnd, ID_LABEL_RECONNECT as i32) },
            state.reconnect_hwnd,
        ] {
            unsafe {
                ShowWindow(control, if show_web { 0 } else { SW_SHOW });
            }
        }
        for control in [
            unsafe { GetDlgItem(parent_hwnd, ID_LABEL_URL as i32) },
            state.url_hwnd,
            unsafe { GetDlgItem(parent_hwnd, ID_LABEL_AUTO_REFRESH as i32) },
            state.auto_refresh_hwnd,
        ] {
            unsafe {
                ShowWindow(control, if show_web { SW_SHOW } else { 0 });
            }
        }
    }

    fn accent_class_for_agent(agent_label: &str) -> String {
        let label = agent_label.trim().to_ascii_lowercase();
        if label.contains("claude") {
            "accent-amber".into()
        } else if label.contains("gemini") {
            "accent-violet".into()
        } else if label.contains("open") {
            "accent-rose".into()
        } else {
            "accent-cyan".into()
        }
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

    fn create_combo_box(parent: HWND, control_id: isize) -> HWND {
        unsafe {
            CreateWindowExW(
                0 as WINDOW_EX_STYLE,
                wide("COMBOBOX").as_ptr(),
                wide("").as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32,
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

    fn selected_listbox_index(hwnd: HWND) -> usize {
        let index = unsafe { SendMessageW(hwnd, LB_GETCURSEL, 0, 0) };
        if index < 0 { 0 } else { index as usize }
    }

    fn selected_combo_index(hwnd: HWND) -> usize {
        let index = unsafe { SendMessageW(hwnd, CB_GETCURSEL, 0, 0) };
        if index < 0 { 0 } else { index as usize }
    }

    fn select_combo_index(hwnd: HWND, index: usize) {
        unsafe {
            SendMessageW(hwnd, CB_SETCURSEL, index, 0);
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
                    "RegisterClassW failed for launcher editor: {error}"
                ));
            }
        }
        Ok(())
    }

    unsafe fn state_mut(hwnd: HWND) -> Option<&'static mut EditorWindowState> {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut EditorWindowState;
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
pub use imp::{present, sync_draft_state};
