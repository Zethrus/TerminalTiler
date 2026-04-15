#[cfg(target_os = "windows")]
mod imp {
    use std::mem;
    use std::path::PathBuf;
    use std::ptr;
    use std::rc::Rc;

    use serde::{Deserialize, Serialize};
    use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
        ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_LEFT, ES_MULTILINE, ES_READONLY, GWLP_USERDATA,
        GetClientRect, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW,
        LoadCursorW, RegisterClassW, SW_SHOW, SWP_NOZORDER, SendMessageW, SetWindowLongPtrW,
        SetWindowPos, SetWindowTextW, ShowWindow, WINDOW_EX_STYLE, WM_CLOSE, WM_COMMAND, WM_CREATE,
        WM_NCCREATE, WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW, WS_BORDER, WS_CHILD,
        WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
    };

    use crate::model::assets::{
        AgentRoleTemplate, CliSnippet, ConnectionProfile, InventoryGroup, InventoryHost, Runbook,
        WorkspaceAssets,
    };
    use crate::model::workspace_config::ConfigScope;
    use crate::services::assets_editor::{
        AssetSection, AssetValidationIssue, effective_assets_for_scope, prune_blank_drafts,
        validate_assets,
    };
    use crate::storage::asset_store::AssetStore;

    const WINDOW_CLASS: &str = "TerminalTilerWindowsAssetsManager";
    const ID_GLOBAL_SCOPE: isize = 1001;
    const ID_WORKSPACE_SCOPE: isize = 1002;
    const ID_INFO: isize = 1003;
    const ID_TEXT: isize = 1004;
    const ID_RELOAD: isize = 1005;
    const ID_SAVE: isize = 1006;
    const ID_CLOSE: isize = 1007;
    const ID_ISSUES: isize = 1008;
    const ID_SECTION_OVERVIEW: isize = 1101;
    const ID_SECTION_CONNECTIONS: isize = 1102;
    const ID_SECTION_HOSTS: isize = 1103;
    const ID_SECTION_GROUPS: isize = 1104;
    const ID_SECTION_ROLES: isize = 1105;
    const ID_SECTION_RUNBOOKS: isize = 1106;
    const ID_SECTION_SNIPPETS: isize = 1107;
    const ID_SECTION_RAW: isize = 1108;
    const MARGIN: i32 = 16;
    const BUTTON_HEIGHT: i32 = 32;
    const FIELD_HEIGHT: i32 = 54;
    const SECTION_ROWS: usize = 2;

    const SECTION_BUTTONS: &[(AssetSection, isize)] = &[
        (AssetSection::Overview, ID_SECTION_OVERVIEW),
        (AssetSection::Connections, ID_SECTION_CONNECTIONS),
        (AssetSection::Hosts, ID_SECTION_HOSTS),
        (AssetSection::Groups, ID_SECTION_GROUPS),
        (AssetSection::Roles, ID_SECTION_ROLES),
        (AssetSection::Runbooks, ID_SECTION_RUNBOOKS),
        (AssetSection::Snippets, ID_SECTION_SNIPPETS),
        (AssetSection::RawToml, ID_SECTION_RAW),
    ];

    #[derive(Serialize, Deserialize, Default)]
    struct ConnectionsDocument {
        #[serde(default)]
        connection_profiles: Vec<ConnectionProfile>,
    }

    #[derive(Serialize, Deserialize, Default)]
    struct HostsDocument {
        #[serde(default)]
        inventory_hosts: Vec<InventoryHost>,
    }

    #[derive(Serialize, Deserialize, Default)]
    struct GroupsDocument {
        #[serde(default)]
        inventory_groups: Vec<InventoryGroup>,
    }

    #[derive(Serialize, Deserialize, Default)]
    struct RolesDocument {
        #[serde(default)]
        role_templates: Vec<AgentRoleTemplate>,
    }

    #[derive(Serialize, Deserialize, Default)]
    struct RunbooksDocument {
        #[serde(default)]
        runbooks: Vec<Runbook>,
    }

    #[derive(Serialize, Deserialize, Default)]
    struct SnippetsDocument {
        #[serde(default)]
        snippets: Vec<CliSnippet>,
    }

    struct AssetsWindowState {
        asset_store: AssetStore,
        workspace_root: Option<PathBuf>,
        on_saved: Rc<dyn Fn()>,
        scope: ConfigScope,
        active_section: AssetSection,
        current_assets: WorkspaceAssets,
        global_assets: WorkspaceAssets,
        warning_text: Option<String>,
        status_message: Option<String>,
        global_button_hwnd: HWND,
        workspace_button_hwnd: HWND,
        section_buttons: Vec<(AssetSection, HWND)>,
        info_hwnd: HWND,
        issues_hwnd: HWND,
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
            active_section: AssetSection::Overview,
            current_assets: WorkspaceAssets::default(),
            global_assets: WorkspaceAssets::default(),
            warning_text: None,
            status_message: None,
            global_button_hwnd: ptr::null_mut(),
            workspace_button_hwnd: ptr::null_mut(),
            section_buttons: Vec::new(),
            info_hwnd: ptr::null_mut(),
            issues_hwnd: ptr::null_mut(),
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
                            state.status_message = None;
                            load_scope(state);
                        }
                        ID_WORKSPACE_SCOPE if state.workspace_root.is_some() => {
                            state.scope = ConfigScope::Workspace;
                            state.status_message = None;
                            load_scope(state);
                        }
                        ID_RELOAD => {
                            state.status_message = Some("Reloaded current scope from disk.".into());
                            load_scope(state);
                        }
                        ID_SAVE => save_scope(state),
                        ID_CLOSE => unsafe {
                            DestroyWindow(hwnd);
                        },
                        _ => {
                            if let Some(section) = section_for_command_id(command_id) {
                                state.active_section = section;
                                state.status_message = None;
                                refresh_view(state);
                            }
                        }
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
        state.section_buttons = SECTION_BUTTONS
            .iter()
            .map(|(section, control_id)| {
                (
                    *section,
                    create_child_window(
                        hwnd,
                        "BUTTON",
                        section.title(),
                        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                        *control_id,
                    ),
                )
            })
            .collect();
        state.info_hwnd = create_child_window(
            hwnd,
            "EDIT",
            "",
            WS_CHILD
                | WS_VISIBLE
                | WS_BORDER
                | ES_LEFT as u32
                | ES_MULTILINE as u32
                | ES_AUTOVSCROLL as u32
                | ES_AUTOHSCROLL as u32
                | ES_READONLY as u32,
            ID_INFO,
        );
        state.issues_hwnd = create_child_window(
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
            ID_ISSUES,
        );
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
            state.issues_hwnd,
            state.text_hwnd,
            state.reload_hwnd,
            state.save_hwnd,
            state.close_hwnd,
        ]
        .into_iter()
        .chain(state.section_buttons.iter().map(|(_, hwnd)| *hwnd))
        {
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
        let scope_y = MARGIN;
        let section_y = scope_y + BUTTON_HEIGHT + 10;
        let section_gap = 8;
        let section_columns = (SECTION_BUTTONS.len() + SECTION_ROWS - 1) / SECTION_ROWS;
        let section_button_width = ((content_width - (section_gap * (section_columns as i32 - 1)))
            / section_columns as i32)
            .max(110);
        let section_block_height = (SECTION_ROWS as i32 * (BUTTON_HEIGHT + 8)) - 8;
        let info_y = section_y + section_block_height + 10;
        let issues_y = info_y + FIELD_HEIGHT + 10;
        let button_y = height - MARGIN - BUTTON_HEIGHT;
        let text_y = issues_y + 122;
        let text_height = (button_y - text_y - 12).max(220);
        unsafe {
            SetWindowPos(
                state.global_button_hwnd,
                ptr::null_mut(),
                MARGIN,
                scope_y,
                132,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.workspace_button_hwnd,
                ptr::null_mut(),
                MARGIN + 140,
                scope_y,
                168,
                BUTTON_HEIGHT,
                SWP_NOZORDER,
            );
        }

        for (index, (_, button_hwnd)) in state.section_buttons.iter().enumerate() {
            let column = index / SECTION_ROWS;
            let row = index % SECTION_ROWS;
            let x = MARGIN + (column as i32 * (section_button_width + section_gap));
            let y = section_y + (row as i32 * (BUTTON_HEIGHT + 8));
            unsafe {
                SetWindowPos(
                    *button_hwnd,
                    ptr::null_mut(),
                    x,
                    y,
                    section_button_width,
                    BUTTON_HEIGHT,
                    SWP_NOZORDER,
                );
            }
        }

        unsafe {
            SetWindowPos(
                state.info_hwnd,
                ptr::null_mut(),
                MARGIN,
                info_y,
                content_width,
                FIELD_HEIGHT,
                SWP_NOZORDER,
            );
            SetWindowPos(
                state.issues_hwnd,
                ptr::null_mut(),
                MARGIN,
                issues_y,
                content_width,
                112,
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

    fn load_scope(state: &mut AssetsWindowState) {
        let global_outcome = state.asset_store.load_assets_with_status();
        state.global_assets = global_outcome.assets.clone();

        let (current_assets, warning_text) = match state.scope {
            ConfigScope::Global => (state.global_assets.clone(), global_outcome.warning),
            ConfigScope::Workspace => {
                if let Some(workspace_root) = state.workspace_root.as_ref() {
                    (
                        state
                            .asset_store
                            .load_workspace_config(workspace_root)
                            .assets,
                        combine_warnings(
                            global_outcome.warning,
                            state
                                .asset_store
                                .load_assets_for_workspace_root(workspace_root)
                                .warning,
                        ),
                    )
                } else {
                    (
                        WorkspaceAssets::default(),
                        Some(
                            "Workspace overrides are unavailable until a workspace root is selected."
                                .into(),
                        ),
                    )
                }
            }
        };

        state.current_assets = current_assets;
        state.warning_text = warning_text;
        refresh_view(state);
    }

    fn refresh_view(state: &AssetsWindowState) {
        let issues = validate_assets(state.scope, &state.current_assets, &state.global_assets);
        let info_text = build_info_text(state);
        let issues_text = build_issues_text(state, &issues);
        let main_text = render_section_text(state, &issues);

        unsafe {
            SetWindowTextW(state.info_hwnd, wide(&info_text).as_ptr());
            SetWindowTextW(state.issues_hwnd, wide(&issues_text).as_ptr());
            SetWindowTextW(state.text_hwnd, wide(&main_text).as_ptr());
            EnableWindow(
                state.save_hwnd,
                (state.active_section != AssetSection::Overview) as i32,
            );
            EnableWindow(
                state.workspace_button_hwnd,
                state.workspace_root.is_some() as i32,
            );
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

        for (section, hwnd) in &state.section_buttons {
            let label = if *section == state.active_section {
                format!("{} *", section.title())
            } else {
                section.title().to_string()
            };
            unsafe {
                SetWindowTextW(*hwnd, wide(&label).as_ptr());
            }
        }
    }

    fn build_info_text(state: &AssetsWindowState) -> String {
        let scope_text = match state.scope {
            ConfigScope::Global => {
                "Editing global defaults from ~/.config/TerminalTiler/workspace-assets.toml. These assets are shared by every workspace.".to_string()
            }
            ConfigScope::Workspace => state
                .workspace_root
                .as_ref()
                .map(|workspace_root| {
                    format!(
                        "Editing workspace overrides from {}/.terminaltiler/workspace.toml. Matching IDs shadow global definitions only in this workspace.",
                        workspace_root.display()
                    )
                })
                .unwrap_or_else(|| {
                    "Workspace overrides are unavailable until a workspace root is selected."
                        .to_string()
                }),
        };

        let save_hint = match state.active_section {
            AssetSection::Overview => "Overview is read-only.",
            AssetSection::RawToml => {
                "Raw TOML edits replace the full scope document. Save after reviewing validation output."
            }
            _ => "Save writes only the selected section back into the current scope document.",
        };

        format!(
            "{}\r\n\r\n{}\r\n{}",
            scope_text,
            state.active_section.description(),
            save_hint
        )
    }

    fn build_issues_text(state: &AssetsWindowState, issues: &[AssetValidationIssue]) -> String {
        let mut lines = Vec::new();
        if let Some(message) = state.status_message.as_deref() {
            lines.push(message.to_string());
        }
        if let Some(warning) = state.warning_text.as_deref() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push("Scope warning:".into());
            lines.push(warning.replace('\n', "\r\n"));
        }

        let filtered = if state.active_section == AssetSection::Overview {
            issues.to_vec()
        } else {
            issues
                .iter()
                .filter(|issue| issue.section == state.active_section)
                .cloned()
                .collect::<Vec<_>>()
        };

        if !lines.is_empty() {
            lines.push(String::new());
        }
        if filtered.is_empty() {
            lines.push(if state.active_section == AssetSection::Overview {
                "Validation: no issues detected in the current scope.".into()
            } else {
                format!(
                    "Validation: no issues detected in {}.",
                    state.active_section.title()
                )
            });
        } else {
            lines.push(format!("Validation issues: {}", filtered.len()));
            for issue in filtered {
                let prefix = issue
                    .item_id
                    .as_deref()
                    .map(|id| format!("- [{}] ", id))
                    .unwrap_or_else(|| "- ".into());
                lines.push(format!("{}{}", prefix, issue.message));
            }
        }

        lines.join("\r\n")
    }

    fn render_section_text(state: &AssetsWindowState, issues: &[AssetValidationIssue]) -> String {
        match state.active_section {
            AssetSection::Overview => build_overview_text(state, issues),
            AssetSection::Connections => toml::to_string_pretty(&ConnectionsDocument {
                connection_profiles: state.current_assets.connection_profiles.clone(),
            })
            .unwrap_or_else(|error| format!("# serialization failed\n# {}\n", error)),
            AssetSection::Hosts => toml::to_string_pretty(&HostsDocument {
                inventory_hosts: state.current_assets.inventory_hosts.clone(),
            })
            .unwrap_or_else(|error| format!("# serialization failed\n# {}\n", error)),
            AssetSection::Groups => toml::to_string_pretty(&GroupsDocument {
                inventory_groups: state.current_assets.inventory_groups.clone(),
            })
            .unwrap_or_else(|error| format!("# serialization failed\n# {}\n", error)),
            AssetSection::Roles => toml::to_string_pretty(&RolesDocument {
                role_templates: state.current_assets.role_templates.clone(),
            })
            .unwrap_or_else(|error| format!("# serialization failed\n# {}\n", error)),
            AssetSection::Runbooks => toml::to_string_pretty(&RunbooksDocument {
                runbooks: state.current_assets.runbooks.clone(),
            })
            .unwrap_or_else(|error| format!("# serialization failed\n# {}\n", error)),
            AssetSection::Snippets => toml::to_string_pretty(&SnippetsDocument {
                snippets: state.current_assets.snippets.clone(),
            })
            .unwrap_or_else(|error| format!("# serialization failed\n# {}\n", error)),
            AssetSection::RawToml => toml::to_string_pretty(&state.current_assets)
                .unwrap_or_else(|error| format!("# serialization failed\n# {}\n", error)),
        }
    }

    fn build_overview_text(state: &AssetsWindowState, issues: &[AssetValidationIssue]) -> String {
        let effective_assets =
            effective_assets_for_scope(state.scope, &state.current_assets, &state.global_assets);
        let counts = [
            (
                AssetSection::Connections,
                state.current_assets.connection_profiles.len(),
                effective_assets.connection_profiles.len(),
            ),
            (
                AssetSection::Hosts,
                state.current_assets.inventory_hosts.len(),
                effective_assets.inventory_hosts.len(),
            ),
            (
                AssetSection::Groups,
                state.current_assets.inventory_groups.len(),
                effective_assets.inventory_groups.len(),
            ),
            (
                AssetSection::Roles,
                state.current_assets.role_templates.len(),
                effective_assets.role_templates.len(),
            ),
            (
                AssetSection::Runbooks,
                state.current_assets.runbooks.len(),
                effective_assets.runbooks.len(),
            ),
            (
                AssetSection::Snippets,
                state.current_assets.snippets.len(),
                effective_assets.snippets.len(),
            ),
        ];

        let mut lines = vec![
            format!("Scope: {}", scope_label(state.scope)),
            format!("Validation issues: {}", issues.len()),
            String::new(),
            "Current scope document:".into(),
        ];
        for (section, current_count, _) in counts {
            lines.push(format!("- {}: {} item(s)", section.title(), current_count));
        }
        lines.push(String::new());
        lines.push("Effective asset view seen by workspaces:".into());
        for (section, _, effective_count) in counts {
            lines.push(format!(
                "- {}: {} item(s)",
                section.title(),
                effective_count
            ));
        }
        lines.push(String::new());
        lines.push("Use the section buttons above to edit one asset area at a time, or switch to Raw TOML for full-document edits.".into());
        lines.join("\r\n")
    }

    fn save_scope(state: &mut AssetsWindowState) {
        let raw = read_window_text(state.text_hwnd);
        let next_assets =
            match parse_section_text(state.active_section, &raw, &state.current_assets) {
                Ok(assets) => prune_blank_drafts(assets),
                Err(error) => {
                    state.status_message = Some(format!(
                        "Failed to parse {}: {}",
                        state.active_section.title(),
                        error
                    ));
                    refresh_view(state);
                    return;
                }
            };

        match state.asset_store.save_assets_for_scope(
            &next_assets,
            state.scope,
            state.workspace_root.as_deref(),
        ) {
            Ok(()) => {
                state.current_assets = next_assets;
                if state.scope == ConfigScope::Global {
                    state.global_assets = state.current_assets.clone();
                }
                state.status_message = Some(format!(
                    "Saved {} for the {} scope.",
                    state.active_section.title(),
                    scope_label(state.scope).to_lowercase()
                ));
                (state.on_saved)();
                refresh_view(state);
            }
            Err(error) => {
                state.status_message = Some(format!("Failed to save assets: {}", error));
                refresh_view(state);
            }
        }
    }

    fn parse_section_text(
        section: AssetSection,
        raw: &str,
        current_assets: &WorkspaceAssets,
    ) -> Result<WorkspaceAssets, toml::de::Error> {
        let mut next_assets = current_assets.clone();
        match section {
            AssetSection::Overview => {}
            AssetSection::Connections => {
                next_assets.connection_profiles =
                    toml::from_str::<ConnectionsDocument>(raw)?.connection_profiles;
            }
            AssetSection::Hosts => {
                next_assets.inventory_hosts = toml::from_str::<HostsDocument>(raw)?.inventory_hosts;
            }
            AssetSection::Groups => {
                next_assets.inventory_groups =
                    toml::from_str::<GroupsDocument>(raw)?.inventory_groups;
            }
            AssetSection::Roles => {
                next_assets.role_templates = toml::from_str::<RolesDocument>(raw)?.role_templates;
            }
            AssetSection::Runbooks => {
                next_assets.runbooks = toml::from_str::<RunbooksDocument>(raw)?.runbooks;
            }
            AssetSection::Snippets => {
                next_assets.snippets = toml::from_str::<SnippetsDocument>(raw)?.snippets;
            }
            AssetSection::RawToml => {
                next_assets = toml::from_str::<WorkspaceAssets>(raw)?;
            }
        }
        Ok(next_assets)
    }

    fn combine_warnings(first: Option<String>, second: Option<String>) -> Option<String> {
        match (first, second) {
            (Some(first), Some(second)) if !second.trim().is_empty() => {
                Some(format!("{}\n{}", first, second))
            }
            (Some(first), _) => Some(first),
            (_, Some(second)) => Some(second),
            (None, None) => None,
        }
    }

    fn scope_label(scope: ConfigScope) -> &'static str {
        match scope {
            ConfigScope::Global => "Global",
            ConfigScope::Workspace => "Workspace",
        }
    }

    fn section_for_command_id(command_id: isize) -> Option<AssetSection> {
        SECTION_BUTTONS
            .iter()
            .find_map(|(section, id)| (*id == command_id).then_some(*section))
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
