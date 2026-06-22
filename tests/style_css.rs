const STYLE_CSS: &str = include_str!("../resources/style.css");
const ABOUT_DIALOG_RS: &str = include_str!("../src/ui/about_dialog.rs");
const APP_CHROME_RS: &str = include_str!("../src/ui/app_chrome.rs");
const APP_PATHS_RS: &str = include_str!("../src/app_paths.rs");
const APPEARANCE_RS: &str = include_str!("../src/ui/appearance.rs");
const ASSETS_MANAGER_RS: &str = include_str!("../src/ui/assets_manager.rs");
const COMMAND_PALETTE_RS: &str = include_str!("../src/ui/command_palette.rs");
const COMPANION_DIALOG_RS: &str = include_str!("../src/ui/companion_dialog.rs");
const CONTEXT_MENU_RS: &str = include_str!("../src/ui/context_menu.rs");
const DIALOG_CHROME_RS: &str = include_str!("../src/ui/dialog_chrome.rs");
const DIALOG_SMOKE_RS: &str = include_str!("../src/ui/dialog_smoke.rs");
const CARGO_TOML: &str = include_str!("../Cargo.toml");
const BROADCAST_RS: &str = include_str!("../src/services/broadcast.rs");
const BUILD_RS: &str = include_str!("../build.rs");
const CI_YML: &str = include_str!("../.github/workflows/ci.yml");
const DESIGN_MD: &str = include_str!("../DESIGN.md");
const DOC_WINDOWS_GTK_VISUAL_QA: &str = include_str!("../docs/windows-gtk-visual-qa.md");
const GTK_SHELL_RS: &str = include_str!("../src/gtk_shell/mod.rs");
const ICONS_RS: &str = include_str!("../src/ui/icons.rs");
const LAYOUT_TREE_RS: &str = include_str!("../src/ui/layout_tree.rs");
const LAUNCH_SCREEN_RS: &str = include_str!("../src/ui/launch_screen.rs");
const PACKAGE_APPIMAGE_SH: &str = include_str!("../packaging/build-appimage.sh");
const PACKAGE_ARTIFACTS_YML: &str = include_str!("../.github/workflows/package-artifacts.yml");
const PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH: &str =
    include_str!("../packaging/capture-linux-gtk-visuals.sh");
const PACKAGE_COMPARE_GTK_VISUALS_SH: &str = include_str!("../packaging/compare-gtk-visuals.sh");
const PACKAGE_DEB_SH: &str = include_str!("../packaging/build-deb.sh");
const PANE_STATUS_RS: &str = include_str!("../src/ui/pane_status.rs");
const RELEASE_YML: &str = include_str!("../.github/workflows/release.yml");
const RELEASE_SMOKE_TEST_SH: &str = include_str!("../packaging/release-smoke-test.sh");
const RUNBOOK_CONTROLS_RS: &str = include_str!("../src/ui/runbook_controls.rs");
const RUNBOOK_DIALOG_RS: &str = include_str!("../src/ui/runbook_dialog.rs");
const SETTINGS_DIALOG_RS: &str = include_str!("../src/ui/settings_dialog.rs");
const SNIPPET_POPOVER_RS: &str = include_str!("../src/ui/snippet_popover.rs");
const STATS_DIALOG_RS: &str = include_str!("../src/ui/stats_dialog.rs");
const TAB_RENAME_DIALOG_RS: &str = include_str!("../src/ui/tab_rename_dialog.rs");
const TERMINAL_CONTEXT_MENU_RS: &str = include_str!("../src/ui/terminal_context_menu.rs");
const TERMINAL_RECOVERY_POPOVER_RS: &str = include_str!("../src/ui/terminal_recovery_popover.rs");
const TERMINAL_SESSION_RS: &str = include_str!("../src/terminal/session.rs");
const TERMINAL_HISTORY_RS: &str = include_str!("../src/services/terminal_history.rs");
const TILE_CHROME_RS: &str = include_str!("../src/ui/tile_chrome.rs");
const TITLE_CHROME_RS: &str = include_str!("../src/ui/title_chrome.rs");
const TRANSCRIPT_DIALOG_RS: &str = include_str!("../src/ui/transcript_dialog.rs");
const TILE_VIEW_RS: &str = include_str!("../src/ui/tile_view.rs");
const UI_MOD_RS: &str = include_str!("../src/ui/mod.rs");
const WEB_CONTEXT_MENU_RS: &str = include_str!("../src/ui/web_context_menu.rs");
const WEB_TILE_RS: &str = include_str!("../src/ui/web_tile.rs");
const WINDOW_RS: &str = include_str!("../src/ui/window.rs");
const WINDOWS_APP_RS: &str = include_str!("../src/windows/app.rs");
const WINDOWS_BUILD_PS1: &str = include_str!("../packaging/build-windows.ps1");
const WINDOWS_CAPTURE_VISUALS_PS1: &str =
    include_str!("../packaging/capture-windows-gtk-visuals.ps1");
const WINDOWS_CAPTURE_RELEASE_VISUALS_PS1: &str =
    include_str!("../packaging/capture-windows-release-gtk-visuals.ps1");
const WINDOWS_GTK_APP_RS: &str = include_str!("../src/windows/gtk_app.rs");
const WINDOWS_GTK_RUNTIME_RS: &str = include_str!("../src/windows/gtk_runtime.rs");
const WINDOWS_GTK_TRAY_RS: &str = include_str!("../src/windows/gtk_tray.rs");
const WINDOWS_GTK_VOICE_HOTKEY_RS: &str = include_str!("../src/windows/gtk_voice_hotkey.rs");
const WINDOWS_GTK_SMOKE_PS1: &str = include_str!("../packaging/build-windows-gtk-smoke.ps1");
const WINDOWS_WORKSPACE_RS: &str = include_str!("../src/windows/workspace.rs");
const WINDOWS_INSTALLER_NSI: &str = include_str!("../packaging/windows/installer.nsi");
const WINDOWS_INSTALLER_TOOLS_PS1: &str = include_str!("../packaging/windows-installer-tools.ps1");
const WINDOWS_INSTALLER_WXS: &str = include_str!("../packaging/windows/installer.wxs");
const WINDOWS_WIN32_HELPERS_RS: &str = include_str!("../src/windows/win32_helpers.rs");
const WINDOWS_MOD_RS: &str = include_str!("../src/windows/mod.rs");
const WINDOWS_PORTABLE_NSI: &str = include_str!("../packaging/windows/portable.nsi");
const WINDOWS_RC: &str = include_str!("../resources/windows/terminaltiler.rc");
const WINDOWS_SETUP_GTK_PS1: &str = include_str!("../packaging/setup-windows-gtk.ps1");
const WINDOWS_SMOKE_PS1: &str = include_str!("../packaging/windows-smoke-test.ps1");
const WORKSPACE_CHROME_RS: &str = include_str!("../src/ui/workspace_chrome.rs");
const BOARD_VIEW_RS: &str = include_str!("../src/ui/board_view.rs");
const BOARD_CHROME_RS: &str = include_str!("../src/ui/board_chrome.rs");
const NEW_TASK_DIALOG_RS: &str = include_str!("../src/ui/new_task_dialog.rs");
const AGENT_SETUP_DIALOG_RS: &str = include_str!("../src/ui/agent_setup_dialog.rs");
const WORKSPACE_NAVIGATION_RS: &str = include_str!("../src/ui/workspace_navigation.rs");
const WORKSPACE_TILE_STATE_RS: &str = include_str!("../src/ui/workspace_tile_state.rs");
const WORKSPACE_ALERTS_RS: &str = include_str!("../src/ui/workspace_alerts.rs");
const WORKSPACE_PREVIEW_RS: &str = include_str!("../src/ui/workspace_preview.rs");
const WORKSPACE_VIEW_RS: &str = include_str!("../src/ui/workspace_view.rs");
const VOICE_ENGINE_RS: &str = include_str!("../src/voice/engine.rs");
const VOICE_HUD_RS: &str = include_str!("../src/ui/voice_hud.rs");
const VOICE_PACK_RS: &str = include_str!("../src/voice/pack.rs");
const VOICE_PROCESS_RS: &str = include_str!("../src/voice/process.rs");

const TERMINAL_CARD_STATES: &[&str] = &[
    ".terminal-card.is-active-tile",
    ".terminal-card.is-disconnected",
    ".terminal-card.is-drop-target",
];

#[test]
fn main_header_icon_buttons_have_clear_tooltips() {
    assert!(
        ICONS_RS.contains("button.set_tooltip_text(Some(tooltip))"),
        "shared icon-only button helper should attach native GTK tooltips"
    );
    assert!(
        source_contains(
            APP_CHROME_RS,
            "icon_name::SETTINGS,\n        \"Open application settings\"",
        ) && source_contains(
            APP_CHROME_RS,
            "icon_name::ASSETS,\n        \"Open assets manager\"",
        ),
        "main app header icon-only actions should explain their purpose on hover"
    );
}

#[test]
fn saved_terminal_history_setting_does_not_resize_live_scrollback() {
    assert!(
        TERMINAL_HISTORY_RS.contains("normalize_saved_terminal_history_line_limit")
            && TERMINAL_SESSION_RS.contains("LIVE_TERMINAL_SCROLLBACK_LINES")
            && TERMINAL_SESSION_RS
                .contains("terminal.set_scrollback_lines(LIVE_TERMINAL_SCROLLBACK_LINES)")
            && !WORKSPACE_VIEW_RS.contains("set_scrollback_lines(lines)")
            && !WINDOWS_WORKSPACE_RS.contains("normalize_terminal_history_line_limit")
            && !WINDOWS_GTK_RUNTIME_RS.contains("normalize_terminal_history_line_limit"),
        "saved/restored history limits must stay separate from live terminal scrollback"
    );
}

#[test]
fn win32_tab_switch_captures_outgoing_terminal_history_before_rebuild() {
    assert!(
        WINDOWS_WORKSPACE_RS.contains("fn captured_active_terminal_history")
            && WINDOWS_WORKSPACE_RS.contains("fn capture_active_tab_terminal_history")
            && source_contains(
                WINDOWS_WORKSPACE_RS,
                "capture_active_tab_terminal_history(state);\n        state.active_tab_index = index;\n        rebuild_active_tab_content(hwnd, state);"
            )
            && source_contains(
                WINDOWS_WORKSPACE_RS,
                "capture_active_tab_terminal_history(target_state);\n            target_state.tabs.push(saved_tab);\n            target_state.active_tab_index = target_state.tabs.len().saturating_sub(1);"
            ),
        "Win32 must snapshot the outgoing tab before switching/rebuilding panes"
    );
}

#[test]
fn linux_and_windows_gtk_shells_share_appearance_chrome() {
    assert!(
        UI_MOD_RS.contains("pub(crate) mod appearance;")
            && APPEARANCE_RS.contains("pub(crate) fn apply_theme_mode")
            && APPEARANCE_RS.contains("pub(crate) fn apply_window_density")
            && APPEARANCE_RS.contains("pub(crate) fn apply_optional_window_density")
            && APPEARANCE_RS.contains("pub(crate) fn resolved_theme_uses_dark_palette")
            && APPEARANCE_RS.contains("pub(crate) fn window_uses_dark_theme"),
        "GTK appearance helpers should live in one shared module so Windows and Linux cannot drift"
    );
    assert!(
        APPEARANCE_RS.contains("window.remove_css_class(\"theme-light\")")
            && APPEARANCE_RS.contains("window.remove_css_class(\"theme-dark\")")
            && APPEARANCE_RS.contains("window.add_css_class(if manager.is_dark()")
            && APPEARANCE_RS.contains("window.remove_css_class(\"profile-comfortable\")")
            && APPEARANCE_RS.contains("window.remove_css_class(\"profile-standard\")")
            && APPEARANCE_RS.contains("window.remove_css_class(\"profile-compact\")"),
        "shared GTK appearance should own the exact theme/density CSS class contract"
    );
    assert!(
        WINDOW_RS.contains("use crate::ui::appearance::{")
            && WINDOWS_GTK_APP_RS.contains("use crate::ui::appearance::{")
            && !WINDOW_RS.contains("fn apply_theme_mode(window:")
            && !WINDOWS_GTK_APP_RS.contains("fn apply_theme_mode(window:")
            && !WINDOW_RS.contains("fn apply_window_density(window:")
            && !WINDOWS_GTK_APP_RS.contains("fn apply_window_density(window:"),
        "Linux and Windows GTK shells should call the same appearance helpers instead of carrying duplicate theme/density implementations"
    );
    assert!(
        WINDOWS_GTK_APP_RS.contains("fn apply_launch_deck_profile")
            && WINDOWS_GTK_APP_RS.contains("fn apply_active_preview_profile")
            && WINDOWS_GTK_APP_RS.contains("apply_theme_mode(window, tab.preset.theme)")
            && WINDOWS_GTK_APP_RS.contains("apply_window_density(window, tab.preset.density)")
            && WINDOWS_GTK_APP_RS.contains("apply_active_preview_profile(window, &preview)"),
        "Windows GTK should apply the active Linux-style workspace theme/density profile when workspace tabs are opened or selected"
    );
}

#[test]
fn linux_and_windows_gtk_shells_share_main_window_chrome() {
    assert!(
        UI_MOD_RS.contains("pub(crate) mod app_chrome;")
            && APP_CHROME_RS.contains("pub(crate) struct AppHeaderChrome")
            && APP_CHROME_RS.contains("pub(crate) struct MainTitlebarActions")
            && APP_CHROME_RS.contains("pub(crate) fn build_app_header_chrome")
            && APP_CHROME_RS.contains("pub(crate) fn build_main_titlebar_actions")
            && APP_CHROME_RS.contains("pub(crate) fn sync_workspace_fullscreen_chrome")
            && APP_CHROME_RS.contains("pub(crate) fn build_window_shell")
            && APP_CHROME_RS.contains("show_start_title_buttons(true)")
            && APP_CHROME_RS.contains("show_end_title_buttons(true)")
            && APP_CHROME_RS.contains("header.set_centering_policy(adw::CenteringPolicy::Loose)")
            && APP_CHROME_RS.contains("header.add_css_class(\"app-headerbar\")")
            && APP_CHROME_RS.contains("TitleChrome::new()")
            && APP_CHROME_RS.contains("title.root.add_css_class(\"app-title-handle\")")
            && APP_CHROME_RS.contains("header.set_title_widget(Some(&title.root))")
            && APP_CHROME_RS.contains("\"Templates\"")
            && APP_CHROME_RS.contains("\"Fullscreen\"")
            && APP_CHROME_RS.contains("\"Open application settings\"")
            && APP_CHROME_RS.contains("\"Account / Sync\"")
            && APP_CHROME_RS.contains("\"Open assets manager\""),
        "main GTK shell header/window chrome and titlebar actions should have one shared builder for Linux and Windows"
    );
    assert!(
        WINDOW_RS.contains("build_app_header_chrome()")
            && WINDOWS_GTK_APP_RS.contains("build_app_header_chrome()")
            && WINDOW_RS.contains("build_main_titlebar_actions(&header")
            && WINDOWS_GTK_APP_RS
                .contains("build_main_titlebar_actions(&header, options.companion.is_some())")
            && WINDOW_RS.contains("build_window_shell()")
            && WINDOWS_GTK_APP_RS.contains("build_window_shell()")
            && !WINDOWS_GTK_APP_RS.contains("HeaderBar::builder()")
            && !WINDOWS_GTK_APP_RS.contains("icons::icon_button(")
            && !WINDOWS_GTK_APP_RS.contains("header.add_css_class(\"app-headerbar\")")
            && !WINDOWS_GTK_APP_RS.contains("title.root.add_css_class(\"app-title-handle\")"),
        "Windows GTK should reuse the same main shell chrome/actions as Linux rather than carrying parallel titlebar construction"
    );
    assert!(
        UI_MOD_RS.contains("pub mod companion_dialog;")
            && UI_MOD_RS.contains("all(target_os = \"windows\", feature = \"windows-gtk-shell\")")
            && WINDOWS_GTK_APP_RS.contains("companion_button = titlebar_actions.companion_button")
            && WINDOWS_GTK_APP_RS.contains("options.companion.as_ref()")
            && WINDOWS_GTK_APP_RS.contains("companion_dialog::present(&window, companion.clone())"),
        "Windows GTK should expose the same shared Account / Sync companion titlebar action as Linux when an integration is provided"
    );
    assert!(
        WINDOW_RS.contains("gio::SimpleAction::new(\"open-companion\", None)")
            && WINDOWS_GTK_APP_RS.contains("gio::SimpleAction::new(\"open-companion\", None)")
            && WINDOW_RS.contains("window.add_action(&action);")
            && WINDOWS_GTK_APP_RS.contains("window.add_action(&action);"),
        "Linux and Windows GTK shells should expose win.open-companion when RuntimeOptions carries a companion integration"
    );
    assert!(
        WINDOW_RS.contains("sync_workspace_fullscreen_chrome(")
            && WINDOWS_GTK_APP_RS.contains("sync_workspace_fullscreen_chrome(")
            && WINDOWS_GTK_APP_RS
                .contains("let fullscreen_button = titlebar_actions.fullscreen_button")
            && WINDOWS_GTK_APP_RS.contains("fullscreen_button.connect_clicked")
            && WINDOWS_GTK_APP_RS.contains("workspace_fullscreen_shortcut_controller")
            && WINDOWS_GTK_APP_RS.contains("workspace_density_shortcut_controller")
            && WINDOWS_GTK_APP_RS.contains("workspace_zoom_in_shortcut_controller")
            && WINDOWS_GTK_APP_RS.contains("workspace_zoom_out_shortcut_controller")
            && WINDOWS_GTK_APP_RS.contains("install_workspace_fullscreen_shortcut")
            && WINDOWS_GTK_APP_RS.contains("install_workspace_density_shortcut")
            && WINDOWS_GTK_APP_RS.contains("install_workspace_zoom_shortcut")
            && WINDOWS_GTK_APP_RS.contains("workspace_fullscreen")
            && WINDOWS_GTK_APP_RS.contains("workspace_density")
            && WINDOWS_GTK_APP_RS.contains("workspace_zoom_in")
            && WINDOWS_GTK_APP_RS.contains("workspace_zoom_out")
            && WINDOWS_GTK_APP_RS.contains("save_workspace_fullscreen_shortcut(&shortcut)")
            && WINDOWS_GTK_APP_RS.contains("save_workspace_density_shortcut(&shortcut)")
            && WINDOWS_GTK_APP_RS.contains("save_workspace_zoom_in_shortcut(&shortcut)")
            && WINDOWS_GTK_APP_RS.contains("save_workspace_zoom_out_shortcut(&shortcut)")
            && WINDOWS_GTK_APP_RS.contains("Fullscreen shortcut set to {shortcut}")
            && WINDOWS_GTK_APP_RS.contains("Density shortcut set to {shortcut}")
            && WINDOWS_GTK_APP_RS.contains("Zoom in shortcut set to {shortcut}")
            && WINDOWS_GTK_APP_RS.contains("Zoom out shortcut set to {shortcut}")
            && WINDOWS_GTK_APP_RS.contains("preview.cycle_active_density()")
            && WINDOWS_GTK_APP_RS.contains("preview.adjust_active_zoom(delta)")
            && WORKSPACE_PREVIEW_RS.contains("pub fn cycle_active_density(&self)")
            && WORKSPACE_PREVIEW_RS.contains("pub fn adjust_active_zoom(&self, delta: i32)")
            && WORKSPACE_PREVIEW_RS.contains("appearance_applier")
            && WORKSPACE_PREVIEW_RS.contains("resolved_theme_uses_dark_palette(tab.preset.theme)")
            && WORKSPACE_PREVIEW_RS.contains("tab.preset.density")
            && WORKSPACE_PREVIEW_RS.contains("tab.terminal_zoom_steps")
            && WINDOWS_GTK_RUNTIME_RS.contains("apply_terminal_runtime_appearance")
            && WINDOWS_GTK_RUNTIME_RS.contains("effective_terminal_font_points")
            && WINDOWS_GTK_RUNTIME_RS.contains("font-size: {}pt")
            && WINDOWS_GTK_RUNTIME_RS.contains("appearance_applier: Some(appearance_applier)")
            && WORKSPACE_PREVIEW_RS.contains("bind_preview_web_tile_settings")
            && WORKSPACE_PREVIEW_RS.contains("bind_web_tile_settings_popover")
            && WORKSPACE_PREVIEW_RS.contains("update_active_web_tile_settings")
            && WORKSPACE_PREVIEW_RS.contains("reapply_active_web_runtime_url")
            && WORKSPACE_PREVIEW_RS.contains("url_applier")
            && WINDOWS_GTK_RUNTIME_RS.contains("url_applier: Some(url_applier)")
            && WINDOWS_GTK_RUNTIME_RS.contains("web_runtime_detail")
            && TILE_CHROME_RS.contains("pub(crate) fn bind_web_tile_settings_popover")
            && WORKSPACE_PREVIEW_RS.contains("clamp_terminal_zoom_steps")
            && WINDOWS_GTK_APP_RS.contains("window.connect_fullscreened_notify")
            && WINDOWS_GTK_APP_RS.contains("sync_windows_fullscreen_chrome(window")
            && WINDOWS_GTK_APP_RS.contains("fullscreen_button, true")
            && WINDOWS_GTK_APP_RS.contains("&fullscreen_button, false")
            && source_contains(
                WINDOWS_GTK_APP_RS,
                "&fullscreen_for_click,\n                        &shell_state_for_launch"
            ),
        "Windows GTK should use the same shared workspace fullscreen chrome behavior as Linux for workspace previews and hide it on the launch deck"
    );
}

#[test]
fn terminal_card_state_selectors_do_not_draw_full_card_rings() {
    // is-active-tile intentionally draws a full-card amber border (focused-pane emphasis on
    // squared tiles); disconnected/drop-target stay header-local to avoid noisy split-pane rings.
    const HEADER_LOCAL_ONLY_STATES: &[&str] = &[
        ".terminal-card.is-disconnected",
        ".terminal-card.is-drop-target",
    ];

    let mut checked_selectors = Vec::new();

    for (selectors, body) in css_blocks(STYLE_CSS) {
        for selector in selectors.split(',').map(str::trim) {
            for state in HEADER_LOCAL_ONLY_STATES {
                if is_full_card_state_selector(selector, state) {
                    checked_selectors.push(selector.to_string());
                    assert_forbidden_full_card_ring_properties(selector, body);
                }
            }
        }
    }

    assert!(
        checked_selectors.is_empty(),
        "disconnected/drop-target tile state styling should be header-local; found full-card state selector(s): {}",
        checked_selectors.join(", ")
    );
}

#[test]
fn active_terminal_card_draws_full_amber_border() {
    // Focused pane gets a full-card accent border in both themes (squared-tile emphasis).
    let mut dark = false;
    let mut light = false;

    for (selectors, body) in css_blocks(STYLE_CSS) {
        for selector in selectors.split(',').map(str::trim) {
            if is_full_card_state_selector(selector, ".terminal-card.is-active-tile")
                && declaration_value(body, "border-color").is_some()
            {
                if selector.contains("theme-light") {
                    light = true;
                } else {
                    dark = true;
                }
            }
        }
    }

    assert!(
        dark,
        "dark theme active tile must recolor the full card border"
    );
    assert!(
        light,
        "light theme active tile must recolor the full card border"
    );
}

#[test]
fn workspace_summary_square_radius_survives_base_cascade() {
    let mut final_base_radius = None;

    for (selectors, body) in css_blocks(STYLE_CSS) {
        let is_base_workspace_summary = selectors
            .split(',')
            .map(str::trim)
            .any(|candidate| candidate == ".workspace-summary");

        if is_base_workspace_summary && let Some(value) = declaration_value(body, "border-radius") {
            final_base_radius = Some(value);
        }
    }

    assert_eq!(
        final_base_radius,
        Some("0"),
        "the default Linux workspace summary radius should be set by the final base .workspace-summary declaration, not an earlier dead override"
    );
    assert_css_declaration(
        "window.profile-compact .workspace-summary",
        "border-radius",
        "0",
        "compact summaries should keep the squared visual contract",
    );
    assert_css_declaration(
        "window.windows-gtk-shell .workspace-summary",
        "border-radius",
        "0",
        "Windows GTK summaries should keep the squared visual contract",
    );
}

#[test]
fn terminal_card_states_have_header_local_indicators() {
    for state in TERMINAL_CARD_STATES {
        assert!(
            STYLE_CSS.contains(&format!("{state} .terminal-header")),
            "missing dark-theme header-local indicator for {state}"
        );
        assert!(
            STYLE_CSS.contains(&format!(
                "window.window-shell.theme-light {state} .terminal-header"
            )),
            "missing light-theme header-local indicator for {state}"
        );
    }
}

#[test]
fn tile_surfaces_have_zero_minimum_css_for_split_resizing() {
    for selector in [
        ".terminal-card",
        ".terminal-frame",
        ".web-tile-frame",
        ".terminal-surface",
        ".split-pane",
    ] {
        assert_css_declaration(
            selector,
            "min-width",
            "0",
            "tile surfaces must not impose a horizontal minimum during pane resize",
        );
        assert_css_declaration(
            selector,
            "min-height",
            "0",
            "tile surfaces must not impose a vertical minimum during pane resize",
        );
        assert_css_declaration(
            selector,
            "overflow",
            "hidden",
            "tile surfaces should clip child overflow instead of letting content paint over headers",
        );
    }
}

#[test]
fn tile_widgets_and_panes_are_configured_as_shrinkable() {
    for source in [TILE_CHROME_RS, LAYOUT_TREE_RS] {
        assert!(
            source.contains("set_size_request(0, 0)"),
            "tile, layout slot, and pane widgets should advertise a zero minimum size"
        );
        assert!(
            source.contains("set_overflow(gtk::Overflow::Hidden)"),
            "tile, layout slot, and pane widgets should hide child overflow while resizing"
        );
    }

    assert!(
        TILE_VIEW_RS.contains("build_tile_shell(tile)")
            && TILE_VIEW_RS.contains("build_tile_frame(\"terminal-frame\")")
            && TILE_VIEW_RS.contains("make_shrinkable(&terminal)")
            && WEB_TILE_RS.contains("build_tile_shell(tile)")
            && WEB_TILE_RS.contains("build_tile_frame(\"web-tile-frame\")")
            && WEB_TILE_RS.contains("make_shrinkable(&web_view)"),
        "terminal and web tiles should opt into the shared shrinkable tile shell/frame helpers while keeping runtime surfaces shrinkable"
    );

    assert!(
        TERMINAL_SESSION_RS.contains("terminal.set_size_request(0, 0)")
            && TERMINAL_SESSION_RS.contains("terminal.set_overflow(gtk::Overflow::Hidden)"),
        "VTE terminals should be explicitly shrinkable"
    );

    for paned_property in [
        ".resize_start_child(true)",
        ".resize_end_child(true)",
        ".shrink_start_child(true)",
        ".shrink_end_child(true)",
    ] {
        assert!(
            LAYOUT_TREE_RS.contains(paned_property),
            "GTK Paned must set {paned_property} so both children remain responsive"
        );
    }
}

#[test]
fn web_tile_initial_navigation_waits_until_mapped() {
    assert!(
        WEB_TILE_RS.contains("fn defer_initial_navigation_until_mapped"),
        "web tiles should centralize deferred initial navigation"
    );
    assert!(
        WEB_TILE_RS.contains("web_view.connect_map")
            && WEB_TILE_RS.contains("glib::idle_add_local_once")
            && WEB_TILE_RS.contains("web_view.load_uri(&url)"),
        "WebKit views should start initial navigation only after they are mapped into the rebuilt layout"
    );
}

#[test]
fn linux_workspace_reattach_reflows_existing_layout_tree() {
    assert!(
        WORKSPACE_VIEW_RS.contains("pub fn reflow_layout(&self)")
            && WORKSPACE_VIEW_RS.contains("detach_tile_widgets(tiles.iter())")
            && WORKSPACE_VIEW_RS.contains("self.replace_layout_shell(&layout)")
            && WORKSPACE_VIEW_RS.contains("remount_tiles(&self.inner.slots.borrow(), &tiles)")
            && WORKSPACE_VIEW_RS.contains("self.inner.layout_host.queue_resize()"),
        "workspace reflow should rebuild only the layout shell, then remount existing live tile widgets"
    );
    assert!(
        WINDOW_RS.matches("runtime.reflow_layout();").count() >= 2,
        "Linux detach and reattach paths should reflow GTK paned layouts after reparenting workspaces"
    );
    assert!(
        LAYOUT_TREE_RS.contains("suppress_position_notify")
            && LAYOUT_TREE_RS.contains("apply_saved_ratio_when_allocated")
            && LAYOUT_TREE_RS.contains("add_tick_callback")
            && LAYOUT_TREE_RS.contains("glib::ControlFlow::Continue"),
        "programmatic split-ratio application should wait for allocation and must not persist as user drag changes"
    );
}

#[test]
fn windows_gtk_workspace_tab_close_confirms_active_runtimes_like_linux() {
    assert!(
        WORKSPACE_PREVIEW_RS.contains("pub fn tab_has_active_processes(&self, index: usize)")
            && WORKSPACE_PREVIEW_RS.contains("fn tab_runtime_surfaces(")
            && WORKSPACE_PREVIEW_RS.contains("surface.active_process_checker")
            && WORKSPACE_PREVIEW_RS.contains("fn runtime_surface_key_moves_after_tab_close(")
            && WORKSPACE_PREVIEW_RS.contains("fn apply_runtime_surface_key_update(")
            && WORKSPACE_PREVIEW_RS.contains("stale_keys: runtime_surface_keys_for_tab")
            && WINDOWS_GTK_APP_RS.contains("fn close_windows_preview_tab(")
            && WINDOWS_GTK_APP_RS.contains("preview.tab_has_active_processes(index)")
            && WINDOWS_GTK_APP_RS.contains("dialog_chrome::confirm_destructive_action")
            && WINDOWS_GTK_APP_RS.contains("\"Close Workspace?\"")
            && WINDOWS_GTK_APP_RS
                .contains("\"Running terminal sessions in this workspace will be terminated.\"")
            && WINDOWS_GTK_APP_RS.contains("fn close_windows_preview_tab_now("),
        "Windows GTK title-tab close should use Linux's destructive workspace close confirmation before terminating active terminal runtimes"
    );
}

#[test]
fn windows_gtk_workspace_title_tabs_reorder_like_linux_tabs() {
    assert!(
        WORKSPACE_PREVIEW_RS.contains("pub fn move_tab(&self, index: usize, position: usize)")
            && WORKSPACE_PREVIEW_RS.contains("fn move_tab_in_preview_state(")
            && WORKSPACE_PREVIEW_RS.contains("fn rekey_runtime_surfaces_after_tab_move(")
            && WORKSPACE_PREVIEW_RS.contains("runtime_surface_key_moves_for_tab_reorder")
            && WINDOWS_GTK_APP_RS.contains("fn reorder_windows_preview_tab(")
            && WINDOWS_GTK_APP_RS.contains("fn install_windows_title_tab_reorder(")
            && WINDOWS_GTK_APP_RS.contains("gtk::DragSource::builder()")
            && WINDOWS_GTK_APP_RS.contains(".actions(gdk::DragAction::MOVE)")
            && WINDOWS_GTK_APP_RS.contains(".button(1)")
            && WINDOWS_GTK_APP_RS
                .contains("gtk::DropTarget::new(u32::static_type(), gdk::DragAction::MOVE)")
            && WINDOWS_GTK_APP_RS
                .contains("drop_target.set_propagation_phase(gtk::PropagationPhase::Capture)")
            && WINDOWS_GTK_APP_RS.contains("windows_title_tab_drop_position")
            && WINDOWS_GTK_APP_RS.contains("gdk::Paintable::new_empty(1, 1)")
            && WINDOWS_GTK_APP_RS.contains("preview.move_tab(from_index, position)"),
        "Windows GTK title tabs should support Linux-style left-button drag/drop reordering while preserving live runtime surfaces"
    );
}

#[test]
fn windows_gtk_workspace_tabs_detach_and_reattach_like_linux_tabs() {
    assert!(
        WORKSPACE_PREVIEW_RS.contains("pub struct DetachedPreviewTab")
            && WORKSPACE_PREVIEW_RS.contains("pub fn detach_tab_as_preview(")
            && WORKSPACE_PREVIEW_RS.contains("fn detach_tab_in_preview_state(")
            && WORKSPACE_PREVIEW_RS.contains("fn detach_runtime_surfaces_for_tab(")
            && WORKSPACE_PREVIEW_RS.contains("pub fn take_single_tab_for_transfer(")
            && WORKSPACE_PREVIEW_RS.contains("pub fn push_detached_tab(")
            && WINDOWS_GTK_APP_RS.contains("fn detach_windows_preview_tab(")
            && WINDOWS_GTK_APP_RS.contains("fn present_detached_windows_preview_window(")
            && WINDOWS_GTK_APP_RS.contains("detached_previews")
            && WINDOWS_GTK_APP_RS.contains("fn register_detached_preview(")
            && WINDOWS_GTK_APP_RS.contains("fn unregister_detached_preview(")
            && WINDOWS_GTK_APP_RS.contains("Windows GTK detached workspace registered")
            && WINDOWS_GTK_APP_RS.contains("Windows GTK detached workspace unregistered")
            && WINDOWS_GTK_APP_RS.contains("fn combined_session_snapshot(&self)")
            && WINDOWS_GTK_APP_RS.contains("context_menu::action_button(\"Detach\", None)")
            && WINDOWS_GTK_APP_RS.contains("Workspace detached to a new window")
            && WINDOWS_GTK_APP_RS.contains("context_menu::action_button(\"Reattach\", None)")
            && source_contains(
                WINDOWS_GTK_APP_RS,
                "&[\"flat\", \"titlebar-action-button\"],\n        );\n        header.pack_end(&detached_fullscreen_button);"
            )
            && source_contains(
                WINDOWS_GTK_APP_RS,
                "&[\"flat\", \"titlebar-action-button\"],\n        );\n        reattach_button.set_tooltip_text"
            )
            && WINDOWS_GTK_APP_RS.contains("Close Detached Workspace?")
            && WINDOWS_GTK_APP_RS.contains("detached_preview.take_single_tab_for_transfer()")
            && WINDOWS_GTK_APP_RS.contains("main_preview.push_detached_tab(detached_tab)")
            && WINDOWS_GTK_APP_RS
                .contains("detached_workspace_overlay.set_child(None::<&gtk::Widget>)")
            && WINDOWS_GTK_APP_RS
                .contains("*shell_state.preview.borrow_mut() = Some(detached_preview.clone())")
            && WINDOWS_GTK_APP_RS.contains("let title_right_click = gtk::GestureClick::builder()")
            && WINDOWS_GTK_APP_RS.contains("title_shell.add_controller(title_right_click)")
            && WINDOWS_GTK_APP_RS
                .contains("shell_state.unregister_detached_preview(&detached_preview)")
            && WINDOWS_GTK_APP_RS.contains(
                "detached_preview.terminate_all(\"closing detached Windows GTK workspace\")"
            ),
        "Windows GTK workspace tabs should expose Linux-style Detach/Reattach windows while preserving live runtime surfaces and including detached previews in shell lifecycle state"
    );
}

#[test]
fn voice_pack_install_uses_inline_progress_replacement() {
    assert!(
        SETTINGS_DIALOG_RS.contains("fn build_voice_pack_install_row")
            && SETTINGS_DIALOG_RS.contains("gtk::ProgressBar::builder()")
            && SETTINGS_DIALOG_RS.contains("set_visible_child_name(\"progress\")")
            && SETTINGS_DIALOG_RS.contains("voice_pack_status_provider"),
        "voice pack installation should replace the install button with a live progress bar while downloading"
    );
}

#[test]
fn settings_exposes_application_logs_folder_action() {
    assert!(
        SETTINGS_DIALOG_RS.contains("on_open_logs_folder")
            && SETTINGS_DIALOG_RS.contains("\"Open Logs Folder\"")
            && SETTINGS_DIALOG_RS.contains("icon_name::FOLDER"),
        "GTK settings should expose a folder button for opening application logs"
    );
    assert!(
        WINDOW_RS.contains("logging::ensure_log_directory()")
            && WINDOW_RS.contains("gio::AppInfo::launch_default_for_uri"),
        "GTK settings log-folder action should create the logs directory and launch it through the desktop"
    );
    assert!(
        APP_PATHS_RS.contains("fn platform_state_dir() -> Option<PathBuf>")
            && APP_PATHS_RS.contains("dirs.data_local_dir()")
            && APP_PATHS_RS.contains(".parent()")
            && APP_PATHS_RS.contains("parent.join(\"state\")")
            && APP_PATHS_RS.contains("pub fn webview2_user_data_dir() -> Option<PathBuf>")
            && APP_PATHS_RS.contains("data_local_dir().map(|dir| dir.join(\"webview2\"))"),
        "Windows logs should resolve under a local application state directory and WebView2 should use the writable local data profile"
    );
    assert!(
        WINDOWS_APP_RS.contains("ID_SETTINGS_OPEN_LOGS_FOLDER")
            && WINDOWS_APP_RS.contains("open_logs_folder_from_settings")
            && WINDOWS_APP_RS.contains("open_path_with_shell(state.window_hwnd, &path)"),
        "Windows Win32 settings should open the logs folder through the native shell helper"
    );
    assert!(
        WINDOWS_GTK_APP_RS.contains("on_open_logs_folder")
            && WINDOWS_GTK_APP_RS.contains("open_path_with_shell(std::ptr::null_mut(), &path)")
            && !WINDOWS_GTK_APP_RS.contains("gio::AppInfo::launch_default_for_uri"),
        "Windows GTK settings should use the native shell helper instead of Gio URI launching for folders"
    );
    assert!(
        WINDOWS_WIN32_HELPERS_RS.contains("ShellExecuteW"),
        "source audit should fail if Windows shell execution disappears entirely"
    );
}

#[test]
fn windows_launcher_startup_defers_heavy_initialization() {
    assert!(
        source_contains(
            WINDOWS_APP_RS,
            "logging::info(\"Windows launcher window created\");\n                    if unsafe { PostMessageW(hwnd, WM_STARTUP_INIT, 0, 0) } == 0"
        ),
        "Windows WM_CREATE should create/show the launcher and post deferred startup init"
    );
    assert!(
        source_contains(WINDOWS_APP_RS, "run_deferred_startup_init(hwnd, state);"),
        "Windows startup init should run from a posted message/fallback helper"
    );
    assert!(
        source_contains(
            WINDOWS_APP_RS,
            "if state.controls_initializing\n                        || !state.controls_ready\n                        || state.syncing_launcher_controls\n                    {\n                        return 0;\n                    }"
        ),
        "Windows startup should ignore reentrant control notifications until controls are ready and while programmatically syncing controls"
    );
    assert!(
        source_contains(
            WINDOWS_APP_RS,
            "let was_syncing = state.syncing_launcher_controls;\n        state.syncing_launcher_controls = true;"
        ) && WINDOWS_APP_RS.contains("fn apply_launcher_selection_controls"),
        "programmatic launcher selection sync should guard against recursive WM_COMMAND notifications"
    );
}

#[test]
fn windows_status_webview_check_stays_side_effect_free() {
    assert!(
        source_contains(
            WINDOWS_APP_RS,
            "fn selected_launcher_requires_webview2(state: &AppWindowState) -> bool {\n        layout_requires_webview2(&state.active_layout)\n    }"
        ),
        "status rendering should inspect active layout directly instead of building saveable preset snapshots"
    );
}

#[test]
fn microphone_selector_stays_compact_and_premium() {
    assert!(
        SETTINGS_DIALOG_RS.contains("\"settings-microphone-row\"")
            && SETTINGS_DIALOG_RS.contains("\"microphone-select-shell\"")
            && SETTINGS_DIALOG_RS.contains("\"microphone-select-control\"")
            && SETTINGS_DIALOG_RS.contains("microphone_combo.set_valign(gtk::Align::Center)")
            && SETTINGS_DIALOG_RS.contains("microphone_combo.set_size_request(0, -1)"),
        "microphone selection should use a dedicated compact control instead of stretching to the row height"
    );
    assert!(
        STYLE_CSS.contains(".microphone-select-shell")
            && STYLE_CSS.contains("combobox.surface-select-control button.combo")
            && STYLE_CSS.contains("window.theme-light .microphone-select-shell"),
        "microphone selector should have polished dark and light theme styling"
    );
}

#[test]
fn launch_deck_uses_terminaltiler_logo_asset() {
    assert!(
        LAUNCH_SCREEN_RS.contains("resources/terminaltiler.svg")
            && LAUNCH_SCREEN_RS.contains("gtk::Image::from_icon_name(\"terminaltiler\")"),
        "Workspace Launch Deck should use the TerminalTiler logo asset instead of a symbolic terminal icon"
    );
    assert!(
        LAUNCH_SCREEN_RS.contains("build_terminaltiler_logo_image")
            && LAUNCH_SCREEN_RS.contains("launch-overview-logo-image")
            && STYLE_CSS.contains(".launch-overview-icon.is-brand-logo"),
        "launch deck logo should have explicit brand-logo code and styling hooks"
    );
}

#[test]
fn launch_deck_header_stays_slim_and_readable() {
    assert!(
        LAUNCH_SCREEN_RS.contains(".width_request(36)")
            && LAUNCH_SCREEN_RS.contains(".set_pixel_size(28)")
            && LAUNCH_SCREEN_RS
                .contains("Open saved workspaces or create guided terminal layouts."),
        "launch deck header should use a slimmer logo and shorter readable subtitle"
    );
    assert!(
        LAUNCH_SCREEN_RS.contains("build_launch_meta_chip(\"Core\")")
            && LAUNCH_SCREEN_RS.contains("build_launch_meta_chip(\"Wizard\")")
            && !LAUNCH_SCREEN_RS.contains("build_launch_meta_chip(\"Live preview\")"),
        "launch deck header chips should stay concise and not dominate the slim command bar"
    );
    assert!(
        STYLE_CSS.contains(".launch-overview {")
            && STYLE_CSS.contains("padding: 10px 14px")
            && STYLE_CSS.contains(".launch-overview-copy")
            && STYLE_CSS.contains("line-height: 1.35"),
        "launch deck header should have dedicated compact readable CSS"
    );
}

#[test]
fn launch_deck_keeps_dashboard_polished_and_bounded() {
    assert!(
        LAUNCH_SCREEN_RS.contains("adw::Clamp::builder()")
            && LAUNCH_SCREEN_RS.contains(".maximum_size(1600)")
            && LAUNCH_SCREEN_RS.contains(".max_children_per_line(4)")
            && LAUNCH_SCREEN_RS.contains("launch-stage-clamp"),
        "launch deck content should use a wider bounded four-column dashboard on large windows"
    );
    assert!(
        LAUNCH_SCREEN_RS.contains("compact-action-button")
            && LAUNCH_SCREEN_RS.contains("compact-icon-button")
            && LAUNCH_SCREEN_RS.contains("saved-workspace-footer")
            && LAUNCH_SCREEN_RS.contains("saved-workspace-actions")
            && !LAUNCH_SCREEN_RS.contains("Delete\",\n        icon_name::DELETE"),
        "saved workspace cards should balance the path and compact actions in a footer row with an icon-only Delete action"
    );
    assert!(
        STYLE_CSS.contains(".saved-workspace-card")
            && STYLE_CSS.contains(".saved-workspace-tile-chip")
            && STYLE_CSS.contains(".saved-workspace-card .card-meta")
            && STYLE_CSS.contains("button.pill-button.compact-action-button")
            && STYLE_CSS.contains("button.pill-button.compact-icon-button"),
        "saved workspace card polish should have dedicated readable and maintainable CSS hooks"
    );
}

#[test]
fn primary_actions_use_shared_symbolic_icon_helper() {
    assert!(
        ICONS_RS.contains("itsHover's animated catalog")
            && ICONS_RS.contains("pub(crate) fn labeled_button")
            && ICONS_RS.contains("pub(crate) fn icon_button")
            && ICONS_RS.contains("resources/hover-icons"),
        "action icons should stay centralized and mapped from the requested itsHover action vocabulary"
    );
    assert!(
        std::path::Path::new("resources/hover-icons/terminal.svg").exists()
            && std::path::Path::new("resources/hover-icons/layout-dashboard.svg").exists()
            && std::path::Path::new("resources/hover-icons/save.svg").exists(),
        "core itsHover SVG assets should be present for terminal, layout, and save actions"
    );
    assert!(
        PACKAGE_DEB_SH.contains("resources/hover-icons/*.svg")
            && PACKAGE_APPIMAGE_SH.contains("resources/hover-icons/*.svg"),
        "Linux packages should include vendored itsHover action icons"
    );
    assert!(
        STYLE_CSS.contains(".button-icon-label-content")
            && STYLE_CSS.contains(".button-leading-icon"),
        "shared labeled button icons should have CSS hooks for consistent polish"
    );

    for (surface, source) in [
        ("about dialog", ABOUT_DIALOG_RS),
        ("assets manager", ASSETS_MANAGER_RS),
        ("command palette", COMMAND_PALETTE_RS),
        ("launch screen", LAUNCH_SCREEN_RS),
        ("settings dialog", SETTINGS_DIALOG_RS),
        ("terminal tile", TILE_VIEW_RS),
        ("web tile", WEB_TILE_RS),
        ("window chrome", WINDOW_RS),
        ("workspace toolbar", WORKSPACE_VIEW_RS),
    ] {
        assert!(
            source.contains("icons::labeled_button")
                || source.contains("icons::icon_button")
                || source.contains("build_header_icon_button(icon_name::")
                || source.contains("build_terminal_tile_action_chrome")
                || source.contains("bind_web_tile_settings_popover")
                || source.contains("workspace_alerts::bind_alert_list"),
            "{surface} should use shared symbolic icon helpers for visible actions"
        );
    }
}

#[test]
fn session_resume_prompt_uses_stable_custom_dialog_layout() {
    let resume_prompt = WINDOW_RS
        .split_once("fn prompt_session_resume")
        .and_then(|(_, tail)| {
            tail.split_once("fn show_startup_notice")
                .map(|(body, _)| body)
        })
        .expect("prompt_session_resume should stay before show_startup_notice");

    assert!(
        resume_prompt.contains("let dialog = adw::Dialog::new();")
            && !resume_prompt.contains("MessageDialog")
            && resume_prompt.contains("dialog.set_content_width(380)")
            && !resume_prompt.contains("set_follows_content_size(true)")
            && resume_prompt.contains(".css_classes([\"session-resume-content\"])")
            && resume_prompt.contains(".css_classes([\"session-resume-actions\"])")
            && resume_prompt.contains("gtk::Button::with_label(\"Resume And Rerun\")")
            && resume_prompt.contains("gtk::Button::with_label(\"Resume As Shells\")")
            && resume_prompt.contains("gtk::Button::with_label(\"Start Fresh\")")
            && resume_prompt.contains("dialog.connect_closed")
            && resume_prompt.contains("if !action_taken.replace(true)")
            && resume_prompt.contains("dialog.set_default_widget(Some(&shells_button))"),
        "startup session resume should use a fixed-width custom AdwDialog layout with separate wrapped copy, action stack, and once-guarded close fallback"
    );

    for selector in [
        ".session-resume-dialog .session-resume-content",
        ".session-resume-dialog .session-resume-heading",
        ".session-resume-dialog .session-resume-body",
        ".session-resume-dialog .session-resume-actions",
        ".session-resume-dialog button.session-resume-action",
        ".session-resume-dialog.windows-gtk-shell button.session-resume-action",
    ] {
        assert!(
            STYLE_CSS.contains(selector),
            "resume prompt should have scoped CSS hook: {selector}"
        );
    }
}

#[test]
fn launch_buttons_use_premium_role_contract() {
    for role in [
        "primary = warm cream CTA",
        "secondary = dark glass support action",
        "ghost = low-emphasis transparent action",
        "surface = compact utility control",
        "destructive = red-accent risk",
        "focus = amber keyboard ring",
        "disabled = intentional muted state",
        "Windows parity",
    ] {
        assert!(
            STYLE_CSS.contains(role),
            "GTK CSS should document the button role contract token: {role}"
        );
    }

    for role in [
        "primary-cta-button",
        "suggested-action",
        "secondary-button",
        "ghost-link-button",
        "destructive-button",
        "destructive-action",
        "surface-button",
        "Focus and disabled states",
        "Windows parity",
    ] {
        assert!(
            DESIGN_MD.contains(role),
            "DESIGN.md should document button parity role: {role}"
        );
    }

    assert!(
        source_contains(
            LAUNCH_SCREEN_RS,
            "\"primary-cta-button\",\n            \"new-workspace-layout-button\"",
        ) && source_contains(
            LAUNCH_SCREEN_RS,
            "\"primary-cta-button\",\n            \"compact-action-button\"",
        ) && LAUNCH_SCREEN_RS.contains("\"ghost-link-button\"")
            && source_contains(
                LAUNCH_SCREEN_RS,
                "\"secondary-button\",\n            \"compact-action-button\"",
            )
            && source_contains(
                LAUNCH_SCREEN_RS,
                "\"destructive-button\",\n            \"compact-icon-button\"",
            ),
        "launch dashboard and wizard actions should use explicit primary, secondary, ghost, and destructive roles"
    );

    for selector in [
        "button.pill-button",
        "button.pill-button.flat",
        "button.primary-cta-button",
        "button.suggested-action",
        "button.secondary-button",
        "button.ghost-link-button",
        "button.destructive-button",
        "button.destructive-action",
        "button.surface-button",
        ".wizard-stepper",
        ".wizard-step-chip.is-active",
    ] {
        assert_css_block_contains(
            selector,
            "background",
            "button and stepper roles should override platform-default gray styling",
        );
    }

    assert_css_block_contains(
        "button.pill-button:disabled",
        "color: alpha(@tt_white, 0.36)",
        "dark disabled buttons should remain deliberately muted and legible",
    );
    assert_css_block_contains(
        "button.primary-cta-button:focus",
        "outline-color: alpha(@tt_amber, 0.58)",
        "keyboard focus should use a visible premium amber ring",
    );
    assert_css_block_contains(
        "button.suggested-action:disabled",
        "background",
        "native GTK suggested actions should share primary disabled styling",
    );
    assert_css_block_contains(
        "button.ghost-link-button:active",
        "background",
        "ghost actions should have an explicit active state",
    );
    assert_css_block_contains(
        "button.destructive-action:active",
        "background",
        "native GTK destructive actions should share explicit risk-state styling",
    );
    assert_css_block_contains(
        "window.window-shell button.primary-cta-button.compact-action-button",
        "0 10px 20px alpha(@tt_shadow, 0.26)",
        "compact primary CTAs should keep dimensional premium shadow after compact sizing overrides",
    );
    assert_css_block_contains(
        "window.theme-light button.pill-button",
        "background",
        "light-mode pill buttons should remain usable and not inherit dark glass",
    );

    let global_flat_button_override = css_blocks(STYLE_CSS).into_iter().any(|(selectors, _)| {
        selectors
            .split(',')
            .map(str::trim)
            .any(|candidate| candidate == "button.flat")
    });
    assert!(
        !global_flat_button_override,
        "premium polish should not restyle every flat row/list/context-menu button globally"
    );
}

#[test]
fn windows_gtk_shell_uses_linux_visual_contract_without_replacing_win32_fallback() {
    for dependency in [
        "windows-gtk-shell = [\"dep:adw\", \"dep:gdk\", \"dep:gtk\"]",
        "windows-win32-shell = []",
        "adw = { package = \"libadwaita\"",
        "gtk = { package = \"gtk4\"",
        "gdk = { package = \"gdk4\"",
    ] {
        assert!(
            CARGO_TOML.contains(dependency),
            "Windows GTK parity should declare explicit optional GTK/libadwaita dependency or feature: {dependency}"
        );
    }

    assert!(
        GTK_SHELL_RS.contains("STYLE_CSS: &str = include_str!")
            && GTK_SHELL_RS.contains("DEFAULT_WINDOW_WIDTH: i32 = 1280")
            && GTK_SHELL_RS.contains("DEFAULT_WINDOW_HEIGHT: i32 = 680")
            && GTK_SHELL_RS.contains("SHARED_VISUAL_CONTRACT_CLASSES")
            && GTK_SHELL_RS.contains("WINDOWS_GTK_RESOURCE_PAYLOAD")
            && GTK_SHELL_RS.contains("PLATFORM_RUNTIME_ADAPTERS")
            && GTK_SHELL_RS.contains("terminal-pane")
            && GTK_SHELL_RS.contains("web-pane"),
        "shared GTK shell contract should centralize CSS, resources, visual classes, and runtime adapter boundaries"
    );

    for class_name in [
        "window-shell",
        "launch-shell",
        "launch-stage",
        "launch-dashboard",
        "launch-wizard-shell",
        "saved-workspace-card",
        "wizard-step-chip",
        "app-tab",
        "workspace-summary",
        "terminal-card",
        "web-tile-frame",
        "primary-cta-button",
    ] {
        assert!(
            GTK_SHELL_RS.contains(class_name) && STYLE_CSS.contains(&format!(".{class_name}")),
            "Windows GTK parity contract should name and CSS should style class {class_name}"
        );
    }

    assert!(
        WINDOWS_MOD_RS.contains("feature = \"windows-gtk-shell\"")
            && WINDOWS_MOD_RS.contains("feature = \"windows-win32-shell\"")
            && WINDOWS_MOD_RS.contains("gtk_app::run")
            && WINDOWS_MOD_RS.contains("app::run")
            && WINDOWS_MOD_RS.contains("show_primary_shell_window")
            && WINDOWS_MOD_RS.contains("mod gtk_runtime;"),
        "Windows module routing should make GTK shell selectable while retaining the Win32 fallback"
    );

    assert!(
        WINDOWS_GTK_APP_RS.contains("windows GTK shell startup")
            && WINDOWS_GTK_APP_RS.contains("load_css_for_default_display")
            && WINDOWS_GTK_APP_RS.contains("crate::gtk_shell::DEFAULT_WINDOW_WIDTH")
            && WINDOWS_GTK_APP_RS.contains("crate::gtk_shell::DEFAULT_WINDOW_HEIGHT")
            && WINDOWS_GTK_APP_RS.contains("build_app_header_chrome()")
            && WINDOWS_GTK_APP_RS.contains("window_shell.append(&header)")
            && WINDOWS_GTK_APP_RS.contains("let back_button = titlebar_actions.back_button")
            && WINDOWS_GTK_APP_RS.contains("back_button.set_visible(true)")
            && WINDOWS_GTK_APP_RS.contains("let title_add_button = title.add_button.clone()")
            && WINDOWS_GTK_APP_RS.contains("title_add_button.connect_clicked")
            && !WINDOWS_GTK_APP_RS.contains("title.add_button.set_sensitive(false)")
            && WINDOWS_GTK_APP_RS.contains("Windows GTK shell selected launch deck tab")
            && TITLE_CHROME_RS.contains("pub(crate) struct TitleChrome")
            && TITLE_CHROME_RS.contains("build_title_tab_chrome")
            && WINDOWS_GTK_APP_RS.contains("LaunchScreenInput")
            && WINDOWS_GTK_APP_RS.contains("crate::ui::launch_screen::build")
            && WINDOWS_GTK_APP_RS.contains("code.get()")
            && WINDOWS_GTK_APP_RS.contains("session_for_restore_mode")
            && WINDOWS_GTK_APP_RS
                .contains("crate::ui::workspace_preview::SessionPreview::with_runtime_assets")
            && WINDOWS_GTK_APP_RS.contains("with_runtime_assets_and_change_handler")
            && WINDOWS_GTK_APP_RS.contains("WindowsGtkShellState")
            && WINDOWS_GTK_APP_RS.contains("WindowsGtkShellState::new(session_store.clone())")
            && WINDOWS_GTK_APP_RS.contains("window.connect_close_request")
            && WINDOWS_GTK_APP_RS.contains("let quit_requested = Rc::new(Cell::new(false))")
            && WINDOWS_GTK_APP_RS
                .contains("let current_close_to_background = Rc::new(Cell::new(preferences.close_to_background))")
            && WINDOWS_GTK_APP_RS.contains("!quit_requested.replace(false) && current_close_to_background.get()")
            && WINDOWS_MOD_RS.contains("mod gtk_tray;")
            && WINDOWS_GTK_APP_RS.contains("WindowsGtkTrayController::new(")
            && WINDOWS_GTK_APP_RS.contains("tray_controller.hide_window_to_tray()")
            && WINDOWS_GTK_TRAY_RS.contains("pub(super) struct WindowsGtkTrayController")
            && WINDOWS_GTK_TRAY_RS.contains("Shell_NotifyIconW(NIM_ADD")
            && WINDOWS_GTK_TRAY_RS.contains("WM_WINDOWS_GTK_TRAYICON")
            && WINDOWS_GTK_TRAY_RS.contains("Show / Restore")
            && WINDOWS_GTK_TRAY_RS.contains("Open Settings")
            && WINDOWS_GTK_TRAY_RS.contains("hiding Windows GTK shell window to tray")
            && WINDOWS_GTK_APP_RS.contains("Windows GTK tray unavailable; minimizing shell to background")
            && WINDOWS_GTK_APP_RS.contains("window.minimize()")
            && WINDOWS_GTK_APP_RS.contains("quit_requested.set(true)")
            && WINDOWS_GTK_APP_RS.contains("current_close_to_background.set(close_to_background)")
            && WINDOWS_GTK_APP_RS.contains("current_close_to_background.set(defaults.close_to_background)")
            && WINDOWS_GTK_APP_RS.contains("let force_quit_requested = Rc::new(Cell::new(false))")
            && WINDOWS_GTK_APP_RS.contains("dialog_chrome::confirm_destructive_action")
            && DIALOG_CHROME_RS.contains("pub(crate) fn confirm_destructive_action")
            && WINDOWS_GTK_APP_RS.contains("glib::Propagation::Stop")
            && WINDOWS_GTK_APP_RS.contains("save_preview_session")
            && WINDOWS_GTK_APP_RS.contains("terminate_preview_runtimes")
            && WINDOWS_GTK_APP_RS.contains("gio::SimpleAction::new(\"quit-app\", None)")
            && WINDOWS_GTK_APP_RS.contains("window_for_quit_action.close()")
            && WINDOWS_GTK_APP_RS.contains("persist_windows_gtk_session")
            && WINDOWS_GTK_APP_RS.contains("session_store.save(session)")
            && WINDOWS_GTK_APP_RS.contains("session_store.clear()")
            && WINDOWS_GTK_APP_RS.contains("show_launch_deck_tab")
            && WINDOWS_GTK_APP_RS.contains("show_workspace_preview_tab")
            && WINDOWS_GTK_APP_RS.contains("refresh_windows_launch_deck")
            && WINDOWS_GTK_APP_RS.contains("request_windows_launch_deck_refresh")
            && WINDOWS_GTK_APP_RS
                .contains("Windows GTK shell refreshed launch deck after preset/default change")
            && !WINDOWS_GTK_APP_RS.contains("relaunch to refresh deck")
            && WINDOWS_GTK_APP_RS.contains("sync_windows_shell_title_tabs")
            && WINDOWS_GTK_APP_RS.contains("preview.push_tab(saved_tab)")
            && WORKSPACE_PREVIEW_RS.contains("pub fn push_tab(&self, tab: SavedTab)")
            && WINDOWS_GTK_APP_RS.contains("on_close: Some(Rc::new")
            && WINDOWS_GTK_APP_RS.contains("preview.close_tab(index)")
            && WINDOWS_GTK_APP_RS.contains("Workspace opened as an interactive GTK tab")
            && WINDOWS_GTK_APP_RS.contains("Windows GTK shell {action} interactive GTK workspace")
            && !WINDOWS_GTK_APP_RS.contains("workspace::open_saved_workspaces")
            && !WINDOWS_GTK_APP_RS.contains("wsl::probe_runtime")
            && WINDOWS_GTK_RUNTIME_RS.contains("build_tile_runtime_surface")
            && WINDOWS_GTK_RUNTIME_RS.contains("wsl::probe_runtime")
            && WINDOWS_GTK_RUNTIME_RS.contains("resolve_tile_launch")
            && WINDOWS_GTK_RUNTIME_RS.contains("Command::new(&command.program)")
            && WINDOWS_GTK_RUNTIME_RS.contains("probe_webview2_runtime"),
        "Windows GTK shell should load canonical CSS, reuse the GTK launch deck, share the Linux header/tab chrome, and restore/open workspaces inside the shared interactive GTK shell instead of the legacy Win32 host"
    );

    assert!(
        UI_MOD_RS.contains("pub mod settings_dialog;")
            && UI_MOD_RS.contains("pub mod assets_manager;")
            && UI_MOD_RS.contains("pub(crate) mod dialog_smoke;")
            && WINDOWS_GTK_APP_RS.contains("settings_dialog::present")
            && WINDOWS_GTK_APP_RS.contains("assets_manager::present")
            && WINDOWS_GTK_APP_RS.contains("SettingsDialogInput")
            && WINDOWS_GTK_APP_RS.contains("SettingsDialogActions")
            && WINDOWS_GTK_APP_RS.contains("save_default_theme")
            && WINDOWS_GTK_APP_RS.contains("save_default_density")
            && WINDOWS_GTK_APP_RS.contains("reset_builtin_presets")
            && WINDOWS_GTK_APP_RS.contains("install_windows_voice_pack")
            && WINDOWS_GTK_APP_RS.contains("delete_windows_voice_pack")
            && WINDOWS_GTK_APP_RS.contains("check_windows_voice_pack_health")
            && WINDOWS_GTK_APP_RS.contains("run_voice_engine_health_check")
            && !WINDOWS_GTK_APP_RS
                .contains("GTK settings will open here once Windows settings are migrated")
            && !WINDOWS_GTK_APP_RS
                .contains("GTK assets manager will open here once Windows assets are migrated")
            && !WINDOWS_GTK_APP_RS.contains("will be enabled after runtime parity work"),
        "Windows GTK titlebar actions should open the shared GTK settings/assets dialogs with real shared callbacks instead of placeholder toasts"
    );

    assert!(
        VOICE_PROCESS_RS.contains("CREATE_NO_WINDOW")
            && VOICE_PROCESS_RS.contains("CommandExt")
            && VOICE_PROCESS_RS.contains("apply_background_spawn")
            && VOICE_PACK_RS.contains("apply_background_spawn(command)")
            && VOICE_PACK_RS.contains(".stdout(Stdio::piped())")
            && VOICE_PACK_RS.contains(".stderr(Stdio::piped())")
            && VOICE_PACK_RS.contains("voice-pack-install.log")
            && VOICE_ENGINE_RS.contains("apply_background_spawn(&mut command)")
            && VOICE_ENGINE_RS.contains(".stderr(crate::voice::process::voice_engine_stderr())"),
        "Windows voice pack install and voice helper launches must run hidden and capture output instead of opening console windows"
    );

    assert!(
        UI_MOD_RS.contains("pub mod voice_hud;")
            && VOICE_HUD_RS.contains("pub struct VoiceHud")
            && WINDOW_RS.contains("voice_hud::VoiceHud")
            && WINDOWS_GTK_APP_RS.contains("voice_hud::VoiceHud")
            && WINDOWS_GTK_APP_RS.contains("install_windows_voice_hotkey_controller")
            && WINDOWS_GTK_APP_RS.contains("WindowsVoiceTranscriberHandle::start()")
            && WINDOWS_GTK_APP_RS.contains("ParakeetTranscriber::launch")
            && WINDOWS_GTK_APP_RS.contains("preview.focused_terminal_available()")
            && WINDOWS_GTK_APP_RS.contains("preview.send_text_to_focused_terminal(&text)")
            && WINDOWS_GTK_APP_RS.contains("sync_windows_voice_global_hotkey")
            && WINDOWS_GTK_APP_RS.contains("handle_windows_voice_global_hotkey_activation")
            && WINDOWS_GTK_APP_RS.contains("active_voice_target")
            && WINDOWS_GTK_APP_RS.contains("fn voice_target(&self)")
            && WINDOWS_GTK_APP_RS.contains("if self.launch_deck_active.get()")
            && WINDOWS_GTK_APP_RS.contains("shell_state.set_main_voice_target();")
            && WINDOWS_GTK_APP_RS.contains("shell_state.set_voice_target(&detached_preview)")
            && WINDOWS_GTK_APP_RS.contains("fn install_detached_windows_voice_controls(")
            && WINDOWS_GTK_APP_RS
                .contains("detached_workspace_overlay.add_overlay(&detached_voice_hud.widget())")
            && WINDOWS_GTK_APP_RS.contains("detached_preview.send_text_to_focused_terminal(&text)")
            && WINDOWS_GTK_APP_RS
                .contains("Windows GTK detached workspace selected as voice dictation target")
            && WINDOWS_GTK_APP_RS.contains("WindowsGlobalHotkeyHandle::start")
            && WINDOWS_GTK_VOICE_HOTKEY_RS.contains("RegisterHotKey")
            && WINDOWS_GTK_VOICE_HOTKEY_RS.contains("WM_HOTKEY")
            && WINDOWS_GTK_VOICE_HOTKEY_RS.contains("PostThreadMessageW")
            && WORKSPACE_PREVIEW_RS.contains("pub fn focused_terminal_available(&self)")
            && WORKSPACE_PREVIEW_RS
                .contains("pub fn send_text_to_focused_terminal(&self, text: &str) -> bool"),
        "Windows GTK should share the Linux voice HUD and provide app-scoped/global voice dictation into active main or detached GTK terminal runtimes instead of exposing only voice settings"
    );

    assert!(
        WORKSPACE_PREVIEW_RS.contains("workspace-summary")
            && WORKSPACE_PREVIEW_RS.contains("app-tab-strip")
            && TITLE_CHROME_RS.contains("app-tab-shell")
            && WORKSPACE_PREVIEW_RS.contains("fn render_session_preview")
            && WORKSPACE_PREVIEW_RS.contains("Rc<Cell<usize>>")
            && WORKSPACE_PREVIEW_RS.contains("build_tile_shell")
            && TILE_CHROME_RS.contains("terminal-card")
            && TILE_CHROME_RS.contains("terminal-header")
            && WORKSPACE_PREVIEW_RS.contains("terminal-frame")
            && WORKSPACE_PREVIEW_RS.contains("terminal-surface")
            && WORKSPACE_PREVIEW_RS.contains("web-tile-frame")
            && WORKSPACE_PREVIEW_RS.contains("build_session_preview")
            && WORKSPACE_PREVIEW_RS.contains("session_shape"),
        "Windows GTK workspace preview should reuse the same visible workspace classes as the Linux GTK workspace shell"
    );
    assert!(
        WORKSPACE_PREVIEW_RS.contains("build_interactive_title_tab(TitleTabInput")
            && WORKSPACE_PREVIEW_RS.contains("on_select: Some(Rc::new")
            && WORKSPACE_PREVIEW_RS.contains("on_close: Some(Rc::new")
            && WORKSPACE_PREVIEW_RS.contains("pub struct SessionPreview")
            && WORKSPACE_PREVIEW_RS.contains("pub fn select_tab(&self, next_index: usize)")
            && WORKSPACE_PREVIEW_RS.contains("pub fn close_tab(&self, index: usize) -> bool")
            && WORKSPACE_PREVIEW_RS.contains("pub fn snapshot(&self) -> SavedSession")
            && WORKSPACE_PREVIEW_RS.contains("Rc<RefCell<SavedSession>>")
            && WORKSPACE_PREVIEW_RS.contains("show_inline_tab_strip")
            && WORKSPACE_PREVIEW_RS.contains("while let Some(child) = shell.first_child()")
            && WORKSPACE_PREVIEW_RS.contains("shell.remove(&child)")
            && !TITLE_CHROME_RS.contains("badge_label")
            && !STYLE_CSS.contains("app-tab-badge")
            && !WORKSPACE_PREVIEW_RS.contains("select.set_sensitive(false)"),
        "Windows GTK workspace preview tabs should switch active restored tabs through the shared title tab chrome without adding Windows-only badge chips"
    );
    assert!(
        TITLE_CHROME_RS.contains("app-tab-strip")
            && TITLE_CHROME_RS.contains("app-tab-add")
            && TITLE_CHROME_RS.contains("pub(crate) fn build_title_tab_chrome")
            && TITLE_CHROME_RS.contains("pub(crate) fn build_interactive_title_tab")
            && TITLE_CHROME_RS.contains("pub(crate) struct TitleTabInput")
            && TITLE_CHROME_RS.contains("pub(crate) struct TitleTabChrome")
            && TITLE_CHROME_RS.contains("pub(crate) fn apply_title_tab_state")
            && TITLE_CHROME_RS.contains("let rename_click = gtk::GestureClick::builder()")
            && TITLE_CHROME_RS.contains("let middle_close = gtk::GestureClick::builder()")
            && TITLE_CHROME_RS.contains("chrome.shell.remove_css_class(\"is-inactive\")")
            && TITLE_CHROME_RS.contains("chrome.shell.remove_css_class(\"is-active\")")
            && TITLE_CHROME_RS.contains("chrome.close_button.set_sensitive(close_enabled)")
            && APP_CHROME_RS.contains("let title = TitleChrome::new();")
            && WINDOW_RS.contains("build_interactive_title_tab(TitleTabInput")
            && WORKSPACE_PREVIEW_RS.contains("build_interactive_title_tab(TitleTabInput")
            && WINDOW_RS.contains("apply_title_tab_state(")
            && WINDOWS_GTK_APP_RS.contains("build_app_header_chrome()")
            && WINDOWS_GTK_APP_RS.contains("build_interactive_title_tab(TitleTabInput")
            && WINDOWS_GTK_APP_RS.contains("with_runtime_assets_and_change_handler")
            && WINDOWS_GTK_APP_RS.contains("&session,")
            && WINDOWS_GTK_APP_RS.contains("sync_windows_title_tabs")
            && WINDOWS_GTK_APP_RS.contains("build_windows_title_tab"),
        "Linux and Windows GTK shells should share the same titlebar tab chrome builder/state contract while Windows drives workspace-preview tab switching from the titlebar"
    );
    assert!(
        UI_MOD_RS.contains("pub(crate) mod pane_status;")
            && PANE_STATUS_RS.contains("pub(crate) fn initial_status_snapshot")
            && PANE_STATUS_RS.contains("resolve_tile_launch(tile, workspace_root, assets)")
            && TILE_VIEW_RS.contains("use crate::ui::pane_status::initial_status_snapshot")
            && WORKSPACE_PREVIEW_RS.contains("use crate::ui::pane_status::initial_status_snapshot")
            && WORKSPACE_PREVIEW_RS
                .contains("initial_status_snapshot(tile, &tab.workspace_root, assets).to_line()")
            && WINDOWS_GTK_APP_RS.contains("let workspace_assets = asset_outcome.assets.clone()")
            && WINDOWS_GTK_APP_RS.contains("assets.clone()"),
        "Windows GTK preview headers should share Linux launch-resolution status text instead of reducing terminal status to only the working-directory label"
    );
    assert!(
        WORKSPACE_PREVIEW_RS.contains("crate::ui::layout_tree::build(")
            && WORKSPACE_PREVIEW_RS.contains("update_active_split_ratio")
            && WORKSPACE_PREVIEW_RS.contains("workspace preview split ratio changed")
            && WORKSPACE_PREVIEW_RS
                .contains("for (index, tile) in layout.tile_specs().iter().enumerate()")
            && WORKSPACE_PREVIEW_RS.contains("slot.append(&build_tile(")
            && WORKSPACE_PREVIEW_RS.contains("runtime_factory")
            && WORKSPACE_PREVIEW_RS.contains("runtime_surfaces")
            && !source_contains(
                WORKSPACE_PREVIEW_RS,
                "LayoutNode::Split {\n            axis,"
            ),
        "Windows GTK workspace preview should reuse the shared Linux GTK split renderer so split orientation, ratios, resize handles, and shrink behavior stay identical"
    );
    assert!(
        UI_MOD_RS.contains("pub(crate) mod workspace_tile_state;")
            && WORKSPACE_TILE_STATE_RS
                .contains("const ACTIVE_TILE_CLASS: &str = \"is-active-tile\";")
            && WORKSPACE_TILE_STATE_RS
                .contains("pub(crate) fn set_tile_active_class<W: IsA<gtk::Widget>>")
            && WORKSPACE_TILE_STATE_RS.contains("widget.add_css_class(ACTIVE_TILE_CLASS)")
            && WORKSPACE_TILE_STATE_RS.contains("widget.remove_css_class(ACTIVE_TILE_CLASS)")
            && WORKSPACE_VIEW_RS.contains("workspace_tile_state::set_tile_active_class(")
            && WORKSPACE_PREVIEW_RS
                .contains("workspace_tile_state::set_tile_active_class(&shell, active)")
            && !WORKSPACE_VIEW_RS.contains("const ACTIVE_TILE_CLASS")
            && !WORKSPACE_PREVIEW_RS.contains("shell.add_css_class(\"is-active-tile\")"),
        "Linux and Windows GTK workspace tiles should share active tile class application"
    );
    assert!(
        WORKSPACE_PREVIEW_RS.contains("build_tile_header_chrome")
            && TILE_VIEW_RS.contains("build_tile_header_chrome")
            && WEB_TILE_RS.contains("build_tile_header_chrome")
            && TILE_CHROME_RS.contains("build_pane_group_chip(&input.tile.pane_groups)")
            && TILE_CHROME_RS.contains("pane_groups.join(\", \")")
            && TILE_CHROME_RS
                .contains("set_tooltip_text(Some(&format!(\"Pane groups: {pane_groups}\")))"),
        "Windows GTK workspace preview headers should carry pane-group chips through the same helper as Linux GTK workspace headers"
    );
    assert!(
        UI_MOD_RS.contains("all(target_os = \"windows\", feature = \"windows-gtk-shell\")")
            && UI_MOD_RS.contains("pub(crate) mod tile_chrome;")
            && TILE_CHROME_RS.contains("pub(crate) struct TerminalTileActionChrome")
            && TILE_CHROME_RS.contains("pub(crate) struct WebTileActionChrome")
            && TILE_CHROME_RS.contains("pub(crate) fn build_terminal_tile_action_chrome")
            && TILE_CHROME_RS.contains("pub(crate) fn build_web_tile_action_chrome")
            && TILE_CHROME_RS.contains("pub(crate) fn append_terminal_tile_action_chrome")
            && TILE_CHROME_RS.contains("pub(crate) fn append_web_tile_action_chrome")
            && TILE_CHROME_RS.contains("tile-recovery-action")
            && TILE_CHROME_RS.contains("tile-snippet-action")
            && TILE_CHROME_RS.contains("\"Edit URL and refresh settings\"")
            && source_contains(TILE_CHROME_RS, "actions.append(&status_label);",)
            && WORKSPACE_PREVIEW_RS.contains("let can_close = tab.preset.layout.tile_count() > 1")
            && WORKSPACE_PREVIEW_RS.contains("build_terminal_tile_action_chrome(can_close)")
            && WORKSPACE_PREVIEW_RS.contains("build_web_tile_action_chrome(can_close)")
            && WORKSPACE_PREVIEW_RS.contains("connect_preview_tile_close")
            && TILE_VIEW_RS.contains("build_terminal_tile_action_chrome(can_close)")
            && WEB_TILE_RS.contains("build_web_tile_action_chrome(can_close)")
            && source_contains(
                TILE_CHROME_RS,
                "actions.append(&chrome.recovery_button);\n    actions.append(&chrome.snippet_button);\n    actions.append(&chrome.close_button);"
            )
            && source_contains(
                TILE_CHROME_RS,
                "actions.append(&chrome.settings_button);\n    actions.append(&chrome.close_button);"
            ),
        "Windows GTK preview tile headers should share the Linux header action order and controls for terminal and web tiles"
    );
    assert!(
        WORKSPACE_PREVIEW_RS.contains("close_tab_in_preview_state")
            && WORKSPACE_PREVIEW_RS.contains("build_interactive_title_tab(TitleTabInput")
            && WORKSPACE_PREVIEW_RS.contains("on_close: Some(Rc::new")
            && !WORKSPACE_PREVIEW_RS.contains("chrome.close_button.set_sensitive(false)")
            && !WORKSPACE_PREVIEW_RS.contains("add_button.set_sensitive(false)")
            && !WORKSPACE_PREVIEW_RS.contains("tile_actions.snippet_button.set_sensitive(false)")
            && !WORKSPACE_PREVIEW_RS.contains("tile_actions.settings_button.set_sensitive(false)"),
        "Windows GTK preview should keep shared controls visually live and wire tab close through preview state instead of rendering disabled chrome"
    );
    assert!(
        WORKSPACE_PREVIEW_RS.contains("build_tile_header_chrome")
            && TILE_VIEW_RS.contains("build_tile_header_chrome")
            && WEB_TILE_RS.contains("build_tile_header_chrome")
            && TILE_CHROME_RS.contains("configure_dynamic_header_label")
            && TILE_CHROME_RS.contains("TileHeaderInput")
            && TILE_CHROME_RS.contains("HEADER_STATUS_MAX_CHARS")
            && TILE_CHROME_RS.contains("HEADER_TITLE_MAX_CHARS")
            && TILE_CHROME_RS.contains("HEADER_GROUP_MAX_CHARS")
            && TILE_CHROME_RS.contains("fn domain_from_url")
            && WORKSPACE_PREVIEW_RS.contains("domain_from_url(&url)")
            && WORKSPACE_PREVIEW_RS.contains("pango::EllipsizeMode::Start")
            && WORKSPACE_PREVIEW_RS.contains("pango::EllipsizeMode::End"),
        "Windows GTK preview header labels should use the same bounded ellipsized status/title/group behavior as Linux tile headers"
    );
    assert!(
        WORKSPACE_PREVIEW_RS.contains("build_workspace_summary_chrome")
            && WORKSPACE_VIEW_RS.contains("build_workspace_summary_chrome")
            && WORKSPACE_CHROME_RS.contains("\"Alerts (0)\"")
            && WORKSPACE_CHROME_RS.contains("\"Broadcast Off\"")
            && WORKSPACE_CHROME_RS.contains("\"Quick send command\"")
            && WORKSPACE_CHROME_RS.contains("workspace-broadcast-entry")
            && WORKSPACE_CHROME_RS.contains("\"Add Terminal Tile\"")
            && WORKSPACE_CHROME_RS.contains("\"Add Web Tile\"")
            && WORKSPACE_CHROME_RS.contains("workspace-url-entry")
            && WORKSPACE_CHROME_RS.contains("\"Reload\"")
            && WORKSPACE_CHROME_RS.contains("surface-select-control")
            && WORKSPACE_CHROME_RS.contains("\"Runbook\"")
            && WORKSPACE_CHROME_RS.contains("\"Run\"")
            && WORKSPACE_PREVIEW_RS.contains("fn saved_groups(tab: &SavedTab) -> Vec<String>"),
        "Windows GTK workspace preview summary should mirror the Linux GTK workspace toolbar controls and classes through the shared workspace chrome helper"
    );
    assert!(
        source_contains(
            WORKSPACE_CHROME_RS,
            "summary.append(&name_label);\n    summary.append(&alert_button);\n    summary.append(&toolbar_divider());\n    summary.append(&broadcast_group);\n    summary.append(&toolbar_divider());\n    summary.append(&tiles_group);\n    summary.append(&toolbar_divider());\n    summary.append(&runbook_group);\n    summary.append(&path_label);"
        ) && WORKSPACE_PREVIEW_RS.contains("controls_sensitive: true")
            && WORKSPACE_VIEW_RS.contains("controls_sensitive: true"),
        "Windows GTK workspace preview summary should keep the same grouped toolbar ordering as Linux GTK workspaces via shared chrome"
    );
    assert!(
        source_contains(
            WORKSPACE_CHROME_RS,
            "tiles_group.append(&add_terminal_tile_button);\n    tiles_group.append(&add_web_tile_button);\n    tiles_group.append(&url_entry);\n    tiles_group.append(&url_reload_button);"
        ) && WORKSPACE_CHROME_RS.contains("css_classes([\"toolbar-group\"])")
            && WORKSPACE_CHROME_RS.contains("fn toolbar_divider()"),
        "shared workspace chrome should group the Add-Terminal-Tile/Add-Web-Tile/url controls into one segmented toolbar cluster"
    );
    assert!(
        WORKSPACE_CHROME_RS.contains("pub(crate) fn build_workspace_content_chrome")
            && WORKSPACE_CHROME_RS.contains("pub(crate) fn build_workspace_shell_chrome")
            && WORKSPACE_CHROME_RS.contains("pub(crate) fn build_workspace_alert_sidebar_chrome")
            && WORKSPACE_CHROME_RS.contains("pub(crate) fn build_workspace_alert_revealer")
            && WORKSPACE_CHROME_RS.contains("gtk::Orientation::Horizontal")
            && WORKSPACE_CHROME_RS.contains("gtk::RevealerTransitionType::SlideLeft")
            && WORKSPACE_CHROME_RS.contains("\"Alert Center\"")
            && WORKSPACE_CHROME_RS.contains("\"Mark All Read\"")
            && WORKSPACE_CHROME_RS.contains(".min_content_width(320)")
            && WORKSPACE_CHROME_RS.contains(".margin_top(4)")
            && WORKSPACE_VIEW_RS.contains("build_workspace_shell_chrome")
            && WORKSPACE_VIEW_RS.contains("build_workspace_alert_sidebar_chrome(true)")
            && WORKSPACE_VIEW_RS.contains("build_workspace_content_chrome")
            && WORKSPACE_VIEW_RS.contains("build_workspace_alert_revealer")
            && WORKSPACE_PREVIEW_RS.contains("build_workspace_shell_chrome")
            && WORKSPACE_PREVIEW_RS.contains("build_workspace_alert_sidebar_chrome(true)")
            && WORKSPACE_PREVIEW_RS.contains("build_workspace_content_chrome")
            && WORKSPACE_PREVIEW_RS.contains("build_workspace_alert_revealer"),
        "Windows GTK workspace preview should share the Linux workspace shell/content/alert sidebar/revealer structure instead of appending layout directly"
    );
    assert!(
        !WORKSPACE_PREVIEW_RS.contains("tab.preset.description")
            && !WORKSPACE_PREVIEW_RS.contains("tab.preset.density.label"),
        "Windows GTK workspace preview summary should not keep preview-only description/density chips that are absent from the Linux GTK workspace toolbar"
    );
    assert!(
        WORKSPACE_PREVIEW_RS.contains("fn build_tile_surface(tile: &TileSpec) -> gtk::Box")
            && WORKSPACE_PREVIEW_RS.contains("fn tile_surface_primary(tile: &TileSpec) -> String")
            && WORKSPACE_PREVIEW_RS.contains("fn tile_surface_detail(tile: &TileSpec) -> String")
            && WORKSPACE_PREVIEW_RS.contains("startup_command")
            && WORKSPACE_PREVIEW_RS.contains("working_directory.short_label()")
            && WORKSPACE_PREVIEW_RS.contains("DEFAULT_WEB_URL")
            && !WORKSPACE_PREVIEW_RS.contains("$ terminal runtime adapter")
            && !WORKSPACE_PREVIEW_RS.contains("web runtime adapter")
            && !WORKSPACE_PREVIEW_RS.contains("Windows GTK shell is using"),
        "Windows GTK workspace surfaces should avoid preview-only explanatory copy and instead render pane-specific content with the same terminal-surface classes"
    );

    assert!(
        WINDOW_RS.contains("gtk_shell::DEFAULT_WINDOW_WIDTH")
            && WINDOW_RS.contains("gtk_shell::DEFAULT_WINDOW_HEIGHT")
            && !WINDOW_RS.contains(".default_width(1280)")
            && !WINDOW_RS.contains(".default_height(680)")
            && !WINDOWS_GTK_APP_RS.contains(".default_width(1180)")
            && !WINDOWS_GTK_APP_RS.contains(".default_height(780)"),
        "Linux and Windows GTK shells should share the same default window geometry"
    );
}

#[test]
fn companion_account_sync_dialog_uses_settings_quality_chrome() {
    let companion_dialog = COMPANION_DIALOG_RS.replace("\r\n", "\n");
    assert!(
        companion_dialog.contains(
            "css_classes([\"settings-dialog-content\", \"companion-dialog-content\"])"
        ) && companion_dialog.contains("fn build_companion_summary(")
            && companion_dialog.contains(
                "\"config-panel\",\n            \"settings-section\",\n            \"settings-summary\",\n            \"companion-summary\""
            )
            && companion_dialog.contains("\"companion-section\"")
            && companion_dialog.contains("\"companion-row-list\"")
            && companion_dialog.contains("\"companion-row-label\"")
            && companion_dialog.contains("\"companion-row-value\"")
            && companion_dialog.contains("\"companion-footer\"")
            && companion_dialog.contains("fn action_icon(action: &CompanionAction)")
            && companion_dialog.contains("icon_name::REFRESH")
            && companion_dialog.contains("icon_name::APPLY")
            && companion_dialog.contains("icon_name::WEB")
            && companion_dialog.contains("fn status_class(status: CompanionStatus)")
            && companion_dialog.contains("dialog_smoke::register_companion_dialog(&dialog)")
            && companion_dialog.contains("\"companion-status-chip\""),
        "Account / Sync should reuse the premium settings dialog shell with a summary hero, structured sections, readable rows, status chips, and action icons"
    );

    for selector in [
        ".companion-summary",
        ".companion-summary-icon image",
        ".companion-status-chip.is-ok",
        ".companion-status-chip.is-warning",
        ".companion-status-chip.is-error",
        ".companion-section",
        ".companion-row-list",
        ".companion-row",
        ".settings-dialog-content .companion-row-label",
        ".companion-row-value",
        ".companion-footer",
        "button.companion-action-button",
        "entry.companion-input-entry",
        ".companion-dialog-window.windows-gtk-shell .companion-section",
        ".companion-dialog-window.theme-light .companion-summary",
        "window.companion-dialog-window.theme-light .settings-dialog-content .companion-row-label",
        ".companion-dialog-window.theme-light .companion-row-value",
    ] {
        assert!(
            STYLE_CSS.contains(selector),
            "companion dialog visual contract should include selector: {selector}"
        );
    }

    assert_css_declaration(
        ".settings-dialog-content .companion-row-label",
        "color",
        "rgba(241, 193, 104, 0.78)",
        "companion row labels should beat generic settings field-hint styling in the dark theme",
    );
    assert_css_declaration(
        "window.companion-dialog-window.theme-light .settings-dialog-content .companion-row-label",
        "color",
        "rgba(158, 100, 16, 0.92)",
        "companion row labels should beat generic settings field-hint styling in the light theme",
    );
}

#[test]
fn dialog_smoke_can_require_companion_dialog_for_pro_builds() {
    assert!(
        UI_MOD_RS.contains("pub(crate) mod dialog_smoke;")
            && COMPANION_DIALOG_RS.contains("use crate::ui::dialog_smoke;")
            && COMPANION_DIALOG_RS.contains("dialog_smoke::register_companion_dialog(&dialog)")
            && WINDOW_RS.contains("gio::SimpleAction::new(\"open-companion\", None)")
            && WINDOWS_GTK_APP_RS.contains("gio::SimpleAction::new(\"open-companion\", None)")
            && DIALOG_SMOKE_RS.contains("TERMINALTILER_DIALOG_COMPANION_SMOKE")
            && DIALOG_SMOKE_RS.contains("register_companion_dialog")
            && DIALOG_SMOKE_RS.contains("\"win.open-companion\"")
            && DIALOG_SMOKE_RS.contains("PASS companion close-attempt")
            && WINDOWS_GTK_APP_RS.contains("dialog_smoke::start(&window)"),
        "dialog smoke should optionally require Account / Sync to open/close through the shared win.open-companion action for Pro GTK builds"
    );
}

#[test]
fn usage_stats_record_manual_terminal_typing_only() {
    for token in [
        "install_terminal_input_stats_hook(&terminal, state.clone(), stats.clone())",
        "gtk::EventControllerKey::new()",
        "is_manual_printable_key(key, modifier_state)",
        "terminal.connect_commit(move |_, text, _|",
        "stats.record_manual_typing(text)",
    ] {
        assert!(
            TERMINAL_SESSION_RS.contains(token),
            "GTK terminal usage stats should record only armed manual VTE typing: {token}"
        );
    }
    assert!(
        !TERMINAL_SESSION_RS.contains("record_input")
            && !TERMINAL_SESSION_RS.contains("self.stats.record_manual_typing")
            && !TERMINAL_SESSION_RS.contains("record_manual_typing(&payload)"),
        "GTK terminal programmatic sends and dropped-path paste must not record usage stats"
    );

    for token in [
        "fn write_pane_input(",
        "manual_typing_text_from_char_value(value)",
        "crate::stats_hub::recorder().record_manual_typing(&manual_typing)",
        "if write_pane_input(pane, input.as_ref(), &bytes, \"pane input write failed\")",
        "\"pane paste write failed\"",
        "write_pane_input(pane, text, text.as_bytes(), \"pane paste write failed\")",
    ] {
        assert!(
            source_contains(WINDOWS_WORKSPACE_RS, token),
            "Windows native usage stats should record only manual printable WM_CHAR input: {token}"
        );
    }
    assert!(
        !WINDOWS_WORKSPACE_RS.contains("record_input")
            && !WINDOWS_WORKSPACE_RS.contains("record_manual_typing(text)")
            && !WINDOWS_WORKSPACE_RS.contains("record_manual_typing(&payload)"),
        "Windows native startup commands, broadcast/runbook sends, paste, dropped paths, voice, and special keys must not record usage stats"
    );

    for token in [
        "terminal_runtime_manual_typing_text(key, key_state)",
        "crate::stats_hub::recorder().record_manual_typing(&manual_typing)",
        "send_terminal_runtime_payload(&state, payload)",
        "paste_clipboard_into_terminal_runtime",
        "paste_dropped_paths_into_terminal_runtime",
    ] {
        assert!(
            WINDOWS_GTK_RUNTIME_RS.contains(token),
            "Windows GTK runtime usage stats should be keyed off manual printable key handling only: {token}"
        );
    }
    assert!(
        !WINDOWS_GTK_RUNTIME_RS.contains("record_input")
            && !WINDOWS_GTK_RUNTIME_RS.contains("record_manual_typing(&payload)"),
        "Windows GTK programmatic sends, paste, dropped paths, terminal responses, runbooks, broadcasts, snippets, and voice must not record stats"
    );
}

#[test]
fn usage_stats_dialog_uses_stats_specific_spacing() {
    for token in [
        "dialog_chrome::sync_dialog_chrome_classes(window, &dialog, \"stats-dialog-window\")",
        "stats-dialog-scroller",
        "stats-dialog-content",
        "stats-section",
        "stats-section-heading",
        "Reset Statistics",
        "stats-reset-button",
        "dialog_chrome::confirm_destructive_action",
    ] {
        assert!(
            STATS_DIALOG_RS.contains(token),
            "Usage Statistics dialog should use stats-specific chrome/classes: {token}"
        );
    }

    assert!(
        STATS_DIALOG_RS.contains("stats_hub::reset()")
            && WINDOW_RS.contains("stats_dialog::present_shared(&window_for_stats)")
            && WINDOWS_GTK_APP_RS.contains("stats_dialog::present_shared(&window)")
            && WINDOWS_GTK_TRAY_RS.contains("stats_dialog::present_shared(&window)"),
        "Usage Statistics reset should be centralized in the shared dialog and wired from Linux, Windows GTK, and tray entry points"
    );

    for token in [
        ".stats-dialog-content",
        ".stats-section-heading",
        "min-height: 18px;",
        "padding: 3px 0;",
    ] {
        assert!(
            STYLE_CSS.contains(token),
            "Usage Statistics CSS should reserve vertical heading room to avoid clipping: {token}"
        );
    }
}

#[test]
fn windows_gtk_shell_exposes_shared_command_palette() {
    assert!(
        UI_MOD_RS.contains("pub mod about_dialog;")
            && UI_MOD_RS.contains("pub mod command_palette;")
            && UI_MOD_RS
                .matches("all(target_os = \"windows\", feature = \"windows-gtk-shell\")")
                .count()
                >= 2,
        "about dialog and command palette should be compiled for both Linux GTK and Windows GTK shells"
    );

    for token in [
        "use crate::ui::{",
        "about_dialog, assets_manager, command_palette, companion_dialog, context_menu",
        "dialog_smoke, settings_dialog, tab_rename_dialog",
        "ShortcutControllerHandle",
        "present_command_palette",
        "command_palette::PaletteAction",
        "command_palette::app_actions(command_palette::AppActionCallbacks",
        "command_palette::active_tab_actions",
        "command_palette::workspace_actions(",
        "present_windows_tab_rename",
        "preview.focus_next_alert()",
        "preview.add_terminal_tile()",
        "preview.add_web_tile(DEFAULT_WEB_URL)",
        "preview.run_runbook(&runbook_for_callback)",
        ".runbooks()",
        "Switch to {label}",
        "open_command_palette_handle",
        "struct WindowsSettingsDialogContext",
        "fn present_settings_dialog(context: WindowsSettingsDialogContext)",
        "move || present_settings_dialog(settings_context.clone())",
        "save_command_palette_shortcut(&shortcut)",
        "Command palette shortcut set to {shortcut}",
        "defaults.command_palette_shortcut",
        "install_command_palette_shortcut",
        "command_palette_shortcut_accelerators",
        "workspace_add_terminal_tile_shortcut_controller",
        "install_workspace_add_terminal_tile_shortcut",
        "DEFAULT_ADD_TERMINAL_TILE_ACCEL",
        "<Ctrl><Shift>P",
    ] {
        assert!(
            WINDOWS_GTK_APP_RS.contains(token),
            "Windows GTK should expose the shared Linux command palette affordance: {token}"
        );
    }

    assert!(
        COMMAND_PALETTE_RS.contains("pub struct AppActionCallbacks")
            && COMMAND_PALETTE_RS.contains("pub product_display_name: String")
            && COMMAND_PALETTE_RS.contains("pub fn app_actions(callbacks: AppActionCallbacks)")
            && COMMAND_PALETTE_RS.contains(
                "let about_title = format!(\"About {}\", callbacks.product_display_name)"
            )
            && source_contains(
                COMMAND_PALETTE_RS,
                "title: \"Open Settings\".into(),\n            subtitle: \"Application preferences and shortcuts.\".into(),"
            )
            && COMMAND_PALETTE_RS.find("title: \"Open Settings\".into()")
                < COMMAND_PALETTE_RS.find("title: \"Open Assets Manager\".into()")
            && COMMAND_PALETTE_RS.find("title: \"Open Assets Manager\".into()")
                < COMMAND_PALETTE_RS.find("title: about_title")
            && COMMAND_PALETTE_RS.find("title: about_title")
                < COMMAND_PALETTE_RS.find("title: \"New Tab\".into()")
            && COMMAND_PALETTE_RS.contains("title: \"Open Account / Sync\".into()")
            && WINDOW_RS
                .contains("command_palette::app_actions(command_palette::AppActionCallbacks")
            && WINDOW_RS.contains("product_display_name: options.product.display_name.clone()")
            && WINDOWS_GTK_APP_RS
                .contains("command_palette::app_actions(command_palette::AppActionCallbacks")
            && WINDOWS_GTK_APP_RS
                .contains("product_display_name: options.product.display_name.clone()"),
        "Linux and Windows GTK command palettes should share one source-of-truth base action ordering and copy while honoring Pro product branding"
    );

    assert!(
        COMMAND_PALETTE_RS.contains("pub fn active_tab_actions(rename_active_tab: Rc<dyn Fn()>)")
            && COMMAND_PALETTE_RS
                .contains("pub fn workspace_actions(callbacks: WorkspaceActionCallbacks)")
            && COMMAND_PALETTE_RS.contains("pub struct WorkspaceActionCallbacks")
            && COMMAND_PALETTE_RS.contains("pub struct RunbookAction")
            && COMMAND_PALETTE_RS.contains("title: \"Rename Active Tab\".into()")
            && COMMAND_PALETTE_RS.contains("subtitle: \"Set a custom workspace title.\".into()")
            && COMMAND_PALETTE_RS.contains("title: \"Focus Next Alert\".into()")
            && COMMAND_PALETTE_RS
                .contains("subtitle: \"Jump to the next unread workspace alert.\".into()")
            && COMMAND_PALETTE_RS.contains("title: \"Add Terminal Tile\".into()")
            && COMMAND_PALETTE_RS.contains(
                "subtitle: \"Insert a new terminal pane beside the focused pane.\".into()"
            )
            && COMMAND_PALETTE_RS.contains("title: \"Add Web Tile\".into()")
            && COMMAND_PALETTE_RS.contains(
                "subtitle: \"Insert a new browser tile beside the focused pane.\".into()"
            )
            && COMMAND_PALETTE_RS.contains("format!(\"Run Runbook: {}\"")
            && COMMAND_PALETTE_RS.contains("fn runbook_subtitle(runbook: &Runbook) -> String")
            && WINDOW_RS.contains("command_palette::active_tab_actions")
            && WINDOW_RS.contains("command_palette::workspace_actions(")
            && WINDOW_RS.contains("add_terminal_tile: Rc::new(")
            && WINDOWS_GTK_APP_RS.contains("command_palette::active_tab_actions")
            && WINDOWS_GTK_APP_RS.contains("command_palette::workspace_actions(")
            && WINDOWS_GTK_APP_RS.contains("add_terminal_tile: Rc::new("),
        "Linux and Windows GTK active workspace palette actions should share copy, ordering, and runbook subtitle rules"
    );

    assert!(
        source_contains(
            WINDOW_RS,
            "let mut actions = command_palette::app_actions(command_palette::AppActionCallbacks"
        ) && source_contains(
            WINDOWS_GTK_APP_RS,
            "let mut actions = command_palette::app_actions(command_palette::AppActionCallbacks"
        ),
        "platform-specific command palettes should only provide callbacks around the shared base action list"
    );

    assert!(
        WORKSPACE_PREVIEW_RS.contains("pub fn add_web_tile(&self, initial_url: &str) -> bool")
            && WORKSPACE_PREVIEW_RS.contains("pub fn add_terminal_tile(&self) -> bool")
            && WORKSPACE_PREVIEW_RS
                .contains("pub fn tab_title(&self, index: usize) -> Option<String>")
            && WORKSPACE_PREVIEW_RS.contains(
                "pub fn rename_tab(&self, index: usize, requested_title: Option<String>) -> bool"
            )
            && WORKSPACE_PREVIEW_RS.contains("pub fn focus_next_alert(&self) -> bool")
            && WORKSPACE_PREVIEW_RS
                .contains("pub fn run_runbook(&self, runbook: &Runbook) -> bool")
            && WORKSPACE_PREVIEW_RS.contains("pub fn runbooks(&self) -> Vec<Runbook>")
            && WORKSPACE_PREVIEW_RS.contains("workspace preview tab renamed")
            && WORKSPACE_PREVIEW_RS.contains("workspace preview web tile added")
            && WORKSPACE_PREVIEW_RS.contains("workspace preview terminal tile added")
            && WORKSPACE_PREVIEW_RS.contains("AlertSourceKind::Runbook")
            && WORKSPACE_PREVIEW_RS.contains("send_command_to_active_runtime_surfaces(")
            && UI_MOD_RS.contains("pub(crate) mod tab_rename_dialog")
            && TAB_RENAME_DIALOG_RS.contains("dialog.set_title(\"Rename Workspace\")")
            && TAB_RENAME_DIALOG_RS.contains(
                "Enter a new workspace tab name. Leave it blank to restore automatic naming."
            )
            && TAB_RENAME_DIALOG_RS.contains("let apply_button = icons::labeled_button(")
            && TAB_RENAME_DIALOG_RS.contains("\"Apply\"")
            && WINDOW_RS.contains("tab_rename_dialog::present(")
            && WINDOW_RS.contains("&window_for_rename")
            && WINDOWS_GTK_APP_RS.contains("tab_rename_dialog::present(window")
            && !WINDOWS_GTK_APP_RS.contains("fn prompt_windows_tab_rename")
            && !WINDOW_RS.contains("fn prompt_tab_rename")
            && TITLE_CHROME_RS.contains("let rename_click = gtk::GestureClick::builder()")
            && TITLE_CHROME_RS.contains(".propagation_phase(gtk::PropagationPhase::Capture)")
            && TITLE_CHROME_RS.contains("let middle_close = gtk::GestureClick::builder()")
            && TITLE_CHROME_RS.contains(".button(2)")
            && TITLE_CHROME_RS.contains("on_middle_close()")
            && WINDOWS_GTK_APP_RS.contains("preview.rename_tab(index, requested_title)"),
        "shared GTK workspace preview should expose the same rename, middle-click close, add-web-tile, alert focus, and runbook mutations used by Linux workspace command palette/title actions"
    );

    assert!(
        UI_MOD_RS.contains("pub(crate) mod runbook_controls;")
            && UI_MOD_RS.contains("pub(crate) mod runbook_dialog;")
            && RUNBOOK_CONTROLS_RS.contains("pub(crate) fn sync_runbook_selector")
            && RUNBOOK_CONTROLS_RS.contains("selector.append(Some(\"\"), \"Runbook\")")
            && RUNBOOK_CONTROLS_RS.contains("selector.append(Some(&runbook.id), &runbook.name)")
            && RUNBOOK_CONTROLS_RS.contains("run_button.set_sensitive(!runbooks.is_empty())")
            && WORKSPACE_VIEW_RS.contains("runbook_controls::sync_runbook_selector(")
            && WORKSPACE_PREVIEW_RS.contains("runbook_controls::sync_runbook_selector(")
            && !WORKSPACE_VIEW_RS.contains("runbook_selector.append(Some(\"\"), \"Runbook\")")
            && !WORKSPACE_PREVIEW_RS.contains("runbook_selector.append(Some(\"\"), \"Runbook\")")
            && RUNBOOK_DIALOG_RS.contains("pub(crate) fn present(")
            && RUNBOOK_DIALOG_RS.contains("RunbookConfirmPolicy::Never")
            && RUNBOOK_DIALOG_RS.contains("execute(TemplateVariableValues::new())")
            && RUNBOOK_DIALOG_RS.contains("dialog.set_title(&format!(\"Run {}\", runbook.name))")
            && RUNBOOK_DIALOG_RS.contains("Target: {}  •  Steps: {}  •  {}")
            && RUNBOOK_DIALOG_RS.contains("label(format!(\"Preview:\\n{preview}\"))")
            && RUNBOOK_DIALOG_RS.contains("icons::labeled_button(\"Cancel\"")
            && RUNBOOK_DIALOG_RS.contains("icons::labeled_button(\"Run\"")
            && WORKSPACE_VIEW_RS.contains("present_runbook_dialog(button, runbook")
            && WORKSPACE_VIEW_RS.contains("runbook_dialog::present(")
            && WORKSPACE_PREVIEW_RS.contains("runbook_dialog::present(")
            && !WORKSPACE_VIEW_RS.contains("let dialog = adw::Dialog::new()")
            && !WORKSPACE_PREVIEW_RS.contains("let dialog = adw::Dialog::new()"),
        "Linux and Windows GTK workspace runbook dialogs should share one GTK dialog implementation while platform surfaces only provide execution callbacks"
    );
}

#[test]
fn windows_gtk_workspace_toolbar_controls_are_wired_to_runtime_state() {
    for token in [
        "pub struct TileRuntimeSurface",
        "command_sender: Option<Rc<dyn Fn(&str) -> bool>>",
        "pub type DroppedPathsSender",
        "web_settings_applier: Option<Rc<dyn Fn(&str, Option<u32>)>>",
        "shutdown: Option<Rc<dyn Fn(&str)>>",
        "active_process_checker: Option<Rc<dyn Fn() -> bool>>",
        "recovery_binder: Option<TileRuntimeRecoveryBinder>",
        "pub struct TileRuntimeRecoveryBinder",
        "pub type SessionChangeHandler",
        "with_runtime_assets_and_change_handler",
        "notify_session_changed",
        "pub fn terminate_all(&self, reason: &str)",
        "pub fn has_active_processes(&self) -> bool",
        "send_command_to_active_runtime_surfaces",
        "send_command_to_active_runtime_surface",
        "BroadcastTarget::AllPanes",
        "BroadcastTarget::SavedGroup",
        "target_from_selector_id(combo.active_id().as_deref())",
        "quick_send_payload(&broadcast_entry.text())",
        "sent_status_label(&target.label(), sent)",
        "target.includes(tile)",
        "add_web_tile_to_active_session",
        "split_web_tile(",
        "pub(crate) mod tile_drag;",
        "TileDragPayload::new",
        "TileDragPayload::static_type()",
        "install_preview_tile_drag_and_drop",
        "swap_active_session_tiles",
        "swap_tile_positions(dragged_id, target_id)",
        "workspace preview tile order changed",
        "dropped_paths_sender: Option<DroppedPathsSender>",
        "Option<&dyn Fn()>",
        "show_recovery_prompt",
        "install_preview_dropped_file_target",
        "gtk::gdk::FileList::static_type()",
        "gtk::gio::File::static_type()",
        "text/uri-list",
        "x-special/gnome-copied-files",
        "read_drop_stream_text",
        "local_paths_from_uri_list_text",
        "update_active_split_ratio",
        "update_split_ratio(",
        "update_active_web_tile_url",
        "close_active_session_tile",
        "close_tile(&session_ref.tabs[tab_index].preset.layout, tile_id)",
        "connect_preview_tile_close",
        "prune_runtime_surfaces",
        "bind_preview_runbook_controls",
        "runbook_controls::sync_runbook_selector",
        "present_preview_runbook_dialog",
        "execute_preview_runbook",
        "resolve_runbook(runbook, &variables, &tile_specs)",
        "TemplateVariableValues::new()",
        "bind_preview_alert_controls",
        "mark_all_read_button",
        "alert_store.mark_all_read()",
        "workspace_alerts::bind_alert_list",
        "WorkspaceAlertListInput",
        "bind_preview_terminal_snippets",
        "snippet_popover::install(",
        "SnippetPopoverInput",
        "execute_preview_snippet",
        "resolve_snippet(snippet, &variables)",
        "pub(crate) mod snippet_popover",
        "active_tab_tile_specs",
        "format!(\"{command}\\n\")",
        "pub(crate) mod context_menu;",
    ] {
        assert!(
            WORKSPACE_PREVIEW_RS.contains(token)
                || UI_MOD_RS.contains(token)
                || WORKSPACE_ALERTS_RS.contains(token)
                || BROADCAST_RS.contains(token),
            "Windows GTK workspace preview should wire shared toolbar/tile controls through runtime/session state: {token}"
        );
    }

    assert!(
        UI_MOD_RS.contains("pub(crate) mod workspace_navigation;")
            && WORKSPACE_NAVIGATION_RS.contains("pub(crate) fn sync_web_navigation_controls")
            && WORKSPACE_NAVIGATION_RS.contains("path_label.set_visible(!has_web_tiles)")
            && WORKSPACE_NAVIGATION_RS.contains("url_entry.set_visible(has_web_tiles)")
            && WORKSPACE_NAVIGATION_RS.contains("url_reload_button.set_visible(has_web_tiles)")
            && WORKSPACE_NAVIGATION_RS.contains("url_entry.set_sensitive(controls_enabled)")
            && WORKSPACE_NAVIGATION_RS
                .contains("url_reload_button.set_sensitive(controls_enabled)")
            && WORKSPACE_VIEW_RS.contains("workspace_navigation::sync_web_navigation_controls(")
            && WORKSPACE_PREVIEW_RS.contains("workspace_navigation::sync_web_navigation_controls(")
            && !WORKSPACE_VIEW_RS.contains("path_label.set_visible(!has_web_tiles)")
            && !WORKSPACE_PREVIEW_RS.contains("path_label.set_visible(!has_web_tiles)"),
        "Linux and Windows GTK workspace summaries should share web navigation visibility/sensitivity syncing"
    );

    assert!(
        BROADCAST_RS.contains("pub fn target_from_selector_id")
            && BROADCAST_RS.contains("pub fn quick_send_payload")
            && BROADCAST_RS.contains("pub fn sent_status_label")
            && BROADCAST_RS.contains("pub fn quick_send_detail")
            && WORKSPACE_VIEW_RS.contains("target_from_selector_id(combo.active_id().as_deref())")
            && WORKSPACE_PREVIEW_RS
                .contains("target_from_selector_id(combo.active_id().as_deref())")
            && WORKSPACE_VIEW_RS.contains("quick_send_payload(&broadcast_entry.text())")
            && WORKSPACE_PREVIEW_RS.contains("quick_send_payload(&broadcast_entry.text())")
            && WORKSPACE_VIEW_RS.contains("sent_status_label(&target.label(), sent)")
            && WORKSPACE_PREVIEW_RS.contains("sent_status_label(&target.label(), sent)")
            && !WORKSPACE_PREVIEW_RS.contains("broadcast_entry.set_text(\"\")"),
        "Linux and Windows GTK quick-send controls should share target parsing, payload/status copy, and preserve the Linux source-of-truth entry retention behavior"
    );

    assert!(
        UI_MOD_RS.contains("pub(crate) mod workspace_alerts;")
            && WORKSPACE_ALERTS_RS.contains("pub(crate) fn bind_alert_list")
            && WORKSPACE_ALERTS_RS.contains("pub(crate) struct WorkspaceAlertListInput")
            && WORKSPACE_ALERTS_RS.contains("pub(crate) struct AlertRowAction")
            && WORKSPACE_ALERTS_RS.contains("alert_store.subscribe(refresh.clone())")
            && WORKSPACE_ALERTS_RS.contains("No detail available.")
            && WORKSPACE_ALERTS_RS.contains("icons::labeled_button(")
            && WORKSPACE_ALERTS_RS.contains("Mark Read")
            && WORKSPACE_VIEW_RS.contains("action_provider: Some(Rc::new")
            && WORKSPACE_VIEW_RS.contains("label: \"Jump\"")
            && WORKSPACE_VIEW_RS.contains("label: \"Reconnect\"")
            && WORKSPACE_PREVIEW_RS.contains("action_provider: None")
            && !WORKSPACE_VIEW_RS.contains("No detail available.")
            && !WORKSPACE_PREVIEW_RS.contains("No detail available."),
        "Linux and Windows GTK workspace alert lists should share row rendering while Linux supplies runtime-only jump/reconnect actions"
    );

    for selector in [
        "window.window-shell.theme-light .alert-count-badge",
        "window.window-shell.theme-light .alert-empty-body",
    ] {
        assert_css_block_contains(
            selector,
            "rgba(25, 35, 50",
            "Alert Center-specific labels must override the dark palette in light theme",
        );
    }
    assert_css_block_contains(
        "window.window-shell.theme-light .alert-empty-title",
        "#192332",
        "Alert Center empty-state title must override the dark palette in light theme",
    );

    assert!(
        SNIPPET_POPOVER_RS.contains(".label(\"CLI Snippets\")")
            && SNIPPET_POPOVER_RS.contains("No snippets configured yet. Add them in Assets.")
            && SNIPPET_POPOVER_RS.contains("snippet-variable-form")
            && SNIPPET_POPOVER_RS.contains("icons::labeled_button(\"Back\"")
            && SNIPPET_POPOVER_RS.contains("icons::labeled_button(\"Run\"")
            && TILE_VIEW_RS.contains("snippet_popover::install(")
            && WORKSPACE_PREVIEW_RS.contains("snippet_popover::install(")
            && !TILE_VIEW_RS.contains("fn build_snippet_popover")
            && !WORKSPACE_PREVIEW_RS.contains("fn refresh_preview_snippet_list"),
        "Linux and Windows GTK terminal snippet popovers should share one visual/control implementation"
    );

    assert!(
        WINDOWS_GTK_RUNTIME_RS.contains("TileRuntimeSurface")
            && WINDOWS_GTK_RUNTIME_RS.contains("install_terminal_output_context_menu")
            && UI_MOD_RS.contains("pub(crate) mod terminal_context_menu")
            && TERMINAL_CONTEXT_MENU_RS.contains("TerminalContextMenuInput")
            && TERMINAL_CONTEXT_MENU_RS.contains("context_menu::popover(parent)")
            && TERMINAL_CONTEXT_MENU_RS.contains("context_menu::action_button(\"Copy\"")
            && TERMINAL_CONTEXT_MENU_RS.contains("context_menu::action_button(\"Paste\"")
            && TERMINAL_CONTEXT_MENU_RS.contains("context_menu::action_button(\"Reconnect\"")
            && TERMINAL_CONTEXT_MENU_RS
                .contains(r#"context_menu::action_button("Open Local Shell", None)"#)
            && TERMINAL_CONTEXT_MENU_RS.contains("context_menu::action_button(\"Show Transcript\"")
            && TERMINAL_CONTEXT_MENU_RS.contains("Focus Command Input")
            && TILE_VIEW_RS.contains("terminal_context_menu::install(")
            && WINDOWS_GTK_RUNTIME_RS.contains("terminal_context_menu::install(")
            && WINDOWS_GTK_RUNTIME_RS.contains("focus_command_input: None")
            && !WINDOWS_GTK_RUNTIME_RS.contains("focus_command_input: Some")
            && !WINDOWS_GTK_RUNTIME_RS.contains("Send command to this Windows terminal pane")
            && !WINDOWS_GTK_RUNTIME_RS.contains("send_entry_text")
            && !WINDOWS_GTK_RUNTIME_RS.contains("workspace-broadcast-entry")
            && WINDOWS_GTK_RUNTIME_RS.contains("TranscriptBuffer")
            && UI_MOD_RS.contains("pub(crate) mod transcript_dialog")
            && TRANSCRIPT_DIALOG_RS.contains("dialog.set_title(\"Recent Transcript\")")
            && TRANSCRIPT_DIALOG_RS.contains("dialog.set_content_width(820)")
            && TRANSCRIPT_DIALOG_RS.contains("dialog.set_content_height(480)")
            && TRANSCRIPT_DIALOG_RS.contains(".monospace(true)")
            && TRANSCRIPT_DIALOG_RS.contains("icons::labeled_button(\"Close\"")
            && TILE_VIEW_RS.contains("transcript_dialog::present(&terminal")
            && WINDOWS_GTK_RUNTIME_RS.contains("transcript_dialog::present(&output")
            && !TILE_VIEW_RS.contains("fn present_transcript_dialog")
            && !WINDOWS_GTK_RUNTIME_RS.contains("fn present_transcript_dialog")
            && WINDOWS_GTK_RUNTIME_RS.contains("recent_transcript(240)")
            && WINDOWS_GTK_RUNTIME_RS.contains("copy_terminal_output_selection")
            && WINDOWS_GTK_RUNTIME_RS.contains("paste_clipboard_into_terminal_runtime")
            && WINDOWS_GTK_RUNTIME_RS.contains("read_text_async(None::<&gio::Cancellable>")
            && WINDOWS_GTK_RUNTIME_RS.contains("DEFAULT_TERMINAL_COPY_SHORTCUT")
            && WINDOWS_GTK_RUNTIME_RS.contains("DEFAULT_TERMINAL_PASTE_SHORTCUT")
            && WINDOWS_GTK_RUNTIME_RS.contains("command_sender: Some(command_sender)")
            && WINDOWS_GTK_RUNTIME_RS.contains("send_terminal_runtime_payload(&state")
            && WINDOWS_GTK_RUNTIME_RS.contains("state.active")
            && WINDOWS_GTK_RUNTIME_RS.contains("VtBuffer::new(")
            && WINDOWS_GTK_RUNTIME_RS.contains("TERMINAL_RUNTIME_COLUMNS")
            && WINDOWS_GTK_RUNTIME_RS.contains("TERMINAL_RUNTIME_ROWS")
            && WINDOWS_GTK_RUNTIME_RS.contains("terminal_buffer.process(&chunk)")
            && WINDOWS_GTK_RUNTIME_RS.contains("status_snapshot_for_terminal_runtime")
            && WINDOWS_GTK_RUNTIME_RS.contains("sync_terminal_runtime_title")
            && WINDOWS_GTK_RUNTIME_RS.contains("terminal.window_title()")
            && WINDOWS_GTK_RUNTIME_RS.contains("terminal.current_working_directory()")
            && WINDOWS_GTK_RUNTIME_RS.contains("helper_summary_text(&matches)")
            && WINDOWS_GTK_RUNTIME_RS.contains("terminal_recent_output(terminal, 32)")
            && WINDOWS_GTK_RUNTIME_RS.contains("context.output_helpers.scan(&recent)")
            && WINDOWS_GTK_RUNTIME_RS.contains("render_terminal_runtime_buffer")
            && WINDOWS_GTK_RUNTIME_RS.contains("TerminalTextStyleKey")
            && WINDOWS_GTK_RUNTIME_RS.contains("HashMap<TerminalTextStyleKey, gtk::TextTag>")
            && WINDOWS_GTK_RUNTIME_RS.contains("terminal_palette(use_dark_palette)")
            && WINDOWS_GTK_RUNTIME_RS.contains("gtk::TextTag::builder()")
            && WINDOWS_GTK_RUNTIME_RS.contains("foreground_rgba")
            && WINDOWS_GTK_RUNTIME_RS.contains("background_rgba")
            && WINDOWS_GTK_RUNTIME_RS.contains("gtk::pango::Underline::Single")
            && WINDOWS_GTK_RUNTIME_RS.contains("buffer.apply_tag")
            && WORKSPACE_PREVIEW_RS.contains("resolved_theme_uses_dark_palette(tab.preset.theme)")
            && WINDOWS_GTK_RUNTIME_RS.contains("terminal.total_rows()")
            && WINDOWS_GTK_RUNTIME_RS.contains("terminal.history_len()")
            && WINDOWS_GTK_RUNTIME_RS.contains("terminal.display_cell(row, column)")
            && WINDOWS_GTK_RUNTIME_RS.contains("reader.read(&mut chunk)")
            && WINDOWS_GTK_RUNTIME_RS.contains("String::from_utf8_lossy")
            && WINDOWS_GTK_RUNTIME_RS.contains("take_pending_input")
            && WINDOWS_GTK_RUNTIME_RS.contains("take_pending_clipboard_write")
            && WINDOWS_GTK_RUNTIME_RS.contains("install_terminal_input_key_controller")
            && WINDOWS_GTK_RUNTIME_RS.contains("terminal_runtime_key_payload")
            && WINDOWS_GTK_RUNTIME_RS.contains("gtk::EventControllerKey::new()")
            && WINDOWS_GTK_RUNTIME_RS.contains("terminal_key_sequence(terminal")
            && WINDOWS_GTK_RUNTIME_RS.contains("control_character_payload")
            && WINDOWS_GTK_RUNTIME_RS
                .contains("gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter => Some(\"\\r\")")
            && WINDOWS_GTK_RUNTIME_RS.contains(".focusable(true)")
            && WINDOWS_GTK_RUNTIME_RS.contains("gtk::GestureClick::builder().button(1)")
            && !WINDOWS_GTK_RUNTIME_RS.contains("read_line")
            && WINDOWS_GTK_RUNTIME_RS.contains("TerminalRuntimeEvent::ProcessStarted")
            && WINDOWS_GTK_RUNTIME_RS.contains("TerminalRuntimeEvent::ProcessEnded")
            && WINDOWS_GTK_RUNTIME_RS.contains("process_handle")
            && WINDOWS_GTK_RUNTIME_RS.contains("TerminateProcess")
            && WINDOWS_GTK_RUNTIME_RS.contains("terminate_terminal_runtime")
            && WINDOWS_GTK_RUNTIME_RS.contains("shutdown: Some(shutdown)")
            && WINDOWS_GTK_RUNTIME_RS
                .contains("active_process_checker: Some(active_process_checker)")
            && WINDOWS_GTK_RUNTIME_RS.contains("recovery_binder: Some(TileRuntimeRecoveryBinder")
            && WINDOWS_GTK_RUNTIME_RS.contains("bind_terminal_recovery_controls")
            && UI_MOD_RS.contains("pub(crate) mod terminal_recovery_popover")
            && TERMINAL_RECOVERY_POPOVER_RS.contains("terminal-recovery-popover")
            && TERMINAL_RECOVERY_POPOVER_RS.contains(".label(\"Session ended\")")
            && TERMINAL_RECOVERY_POPOVER_RS
                .contains("Reconnect the configured session or open a local shell in this pane.")
            && TERMINAL_RECOVERY_POPOVER_RS.contains("\"Reconnect Session\"")
            && TERMINAL_RECOVERY_POPOVER_RS.contains("\"Open Local Shell\"")
            && TILE_VIEW_RS.contains("terminal_recovery_popover::build(")
            && WINDOWS_GTK_RUNTIME_RS.contains("terminal_recovery_popover::build(")
            && !TILE_VIEW_RS.contains("fn build_terminal_recovery_popover")
            && !WINDOWS_GTK_RUNTIME_RS.contains("fn build_terminal_recovery_popover")
            && WINDOWS_GTK_RUNTIME_RS.contains("TerminalLaunchMode::LocalShell")
            && WINDOWS_GTK_RUNTIME_RS.contains("wsl::build_local_shell_command")
            && WORKSPACE_PREVIEW_RS.contains("(recovery_binder.bind)")
            && !WINDOWS_GTK_RUNTIME_RS.contains("stdin.write_all(b\"\\r\\n\")")
            && WINDOWS_GTK_RUNTIME_RS.contains("dropped_paths::serialize_for_target")
            && WINDOWS_GTK_RUNTIME_RS.contains("paste_dropped_paths_into_terminal_runtime")
            && WINDOWS_GTK_RUNTIME_RS.contains("launch_runtime")
            && WINDOWS_GTK_RUNTIME_RS.contains("show_recovery_prompt();")
            && WINDOWS_GTK_RUNTIME_RS.contains("dropped_paths_sender: Some(dropped_paths_sender)")
            && WINDOWS_GTK_RUNTIME_RS.contains("DroppedPathTarget::Wsl")
            && WINDOWS_GTK_RUNTIME_RS.contains("DroppedPathTarget::PowerShell")
            && WINDOWS_GTK_RUNTIME_RS.contains("DroppedPathTarget::Posix")
            && WINDOWS_GTK_RUNTIME_RS.contains("wsl::WindowsLaunchRuntime::Wsl")
            && WINDOWS_GTK_RUNTIME_RS.contains("wsl::WindowsLaunchRuntime::PowerShell")
            && WINDOWS_GTK_RUNTIME_RS.contains("wsl::WindowsLaunchRuntime::Ssh")
            && WINDOWS_GTK_RUNTIME_RS
                .contains("TileKind::WebView => build_web_runtime_surface(tile)")
            && WINDOWS_GTK_RUNTIME_RS.contains("url_applier: Some(url_applier)")
            && WINDOWS_GTK_RUNTIME_RS.contains("web_settings_applier: Some(web_settings_applier)")
            && WINDOWS_GTK_RUNTIME_RS.contains("CreateCoreWebView2EnvironmentWithOptions")
            && WINDOWS_GTK_RUNTIME_RS.contains("CreateCoreWebView2ControllerCompletedHandler")
            && WINDOWS_GTK_RUNTIME_RS.contains("create_gtk_webview_controller_async")
            && WINDOWS_GTK_RUNTIME_RS.contains("complete_gtk_webview_initialization")
            && WINDOWS_GTK_RUNTIME_RS.contains("Windows GTK WebView2 creating environment")
            && WINDOWS_GTK_RUNTIME_RS
                .contains("const WEBVIEW_ENVIRONMENT_CALLBACK_TIMEOUT_SECONDS: u64 = 45")
            && WINDOWS_GTK_RUNTIME_RS
                .contains("const WEBVIEW_CONTROLLER_CALLBACK_TIMEOUT_SECONDS: u64 = 45")
            && WINDOWS_GTK_RUNTIME_RS.contains("app_paths::webview2_user_data_dir()")
            && WINDOWS_GTK_RUNTIME_RS
                .contains("Recoverable WebView2 initialization error: {message}. Open Externally remains available")
            && WINDOWS_GTK_RUNTIME_RS
                .contains("Windows GTK WebView2 controller callback")
            && WINDOWS_GTK_RUNTIME_RS
                .contains("Windows GTK WebView2 runtime surface still waiting for parent HWND")
            && !WINDOWS_GTK_RUNTIME_RS.contains("wait_with_pump")
            && WINDOWS_GTK_RUNTIME_RS.contains("gdk_win32_surface_get_handle")
            && WINDOWS_GTK_RUNTIME_RS.contains("gtk_widget_root_bounds")
            && WINDOWS_GTK_RUNTIME_RS.contains("controller.SetBounds(bounds)")
            && WINDOWS_GTK_RUNTIME_RS.contains("webview.Navigate(&HSTRING::from")
            && WINDOWS_GTK_RUNTIME_RS.contains("webview.Reload()")
            && WINDOWS_GTK_RUNTIME_RS.contains("build_web_runtime_context_menu")
            && UI_MOD_RS.contains("pub(crate) mod web_context_menu")
            && WEB_CONTEXT_MENU_RS.contains("WebContextMenuInput")
            && WEB_CONTEXT_MENU_RS.contains(r#"context_menu::action_button("Reload", Some("F5"))"#)
            && WEB_CONTEXT_MENU_RS.contains(r#"context_menu::action_button("Copy URL", None)"#)
            && WEB_CONTEXT_MENU_RS
                .contains(r#"context_menu::action_button("Open in Browser", None)"#)
            && WEB_CONTEXT_MENU_RS.contains("gio::AppInfo::launch_default_for_uri")
            && WEB_CONTEXT_MENU_RS.contains("open_error_context")
            && WEB_TILE_RS.contains("web_context_menu::install_right_click(")
            && WINDOWS_GTK_RUNTIME_RS.contains("web_context_menu::build(")
            && WINDOWS_GTK_RUNTIME_RS.contains("Windows GTK WebView2 context")
            && WINDOWS_GTK_RUNTIME_RS.contains("ContextMenuRequestedEventHandler")
            && WINDOWS_GTK_RUNTIME_RS.contains("NewWindowRequestedEventHandler")
            && WINDOWS_GTK_RUNTIME_RS.contains("handle_gtk_webview_new_window_request")
            && WINDOWS_GTK_RUNTIME_RS.contains("remove_NewWindowRequested")
            && WINDOWS_GTK_RUNTIME_RS.contains("remove_ContextMenuRequested")
            && WINDOWS_GTK_RUNTIME_RS.contains("controller.Close()"),
        "Windows GTK terminal/runtime surfaces should expose shared command controls and match Linux web pane context actions while embedding WebView2-backed web panes instead of leaving browser tiles as external placeholders"
    );
}

#[test]
fn windows_builds_embed_and_package_terminaltiler_icon() {
    assert!(
        std::path::Path::new("resources/windows/terminaltiler.ico").exists()
            && std::path::Path::new("resources/windows/terminaltiler.rc").exists(),
        "Windows builds should keep a checked-in multi-size TerminalTiler .ico and rc resource for taskbar/shortcut parity"
    );

    assert!(
        BUILD_RS.contains("resources/windows/terminaltiler.rc")
            && BUILD_RS.contains("resources/windows/terminaltiler.ico")
            && BUILD_RS.contains("rc.exe")
            && BUILD_RS.contains("find_windows_kit_resource_compiler")
            && BUILD_RS.contains("Windows Kits")
            && BUILD_RS.contains("cargo:rustc-link-arg-bin=terminaltiler=")
            && BUILD_RS.contains("host.contains(\"windows\")")
            && WINDOWS_RC.contains("1 ICON \"terminaltiler.ico\""),
        "Cargo should embed the TerminalTiler icon in Windows MSVC binaries while letting non-Windows cross-checks skip rc.exe"
    );

    assert!(
        WINDOWS_BUILD_PS1.contains("resources\\windows\\terminaltiler.ico")
            && WINDOWS_BUILD_PS1.contains("$WindowsIconPath")
            && WINDOWS_BUILD_PS1.contains("/DICON_FILE=$WindowsIconPath")
            && WINDOWS_BUILD_PS1.contains("-dIconFile=$WindowsIconPath")
            && WINDOWS_BUILD_PS1.contains("share\\terminaltiler.ico")
            && WINDOWS_BUILD_PS1
                .contains("share\\icons\\hicolor\\scalable\\apps\\terminaltiler.svg")
            && WINDOWS_SMOKE_PS1.contains("share\\terminaltiler.ico")
            && WINDOWS_SMOKE_PS1
                .contains("share\\icons\\hicolor\\scalable\\apps\\terminaltiler.svg")
            && WINDOWS_SMOKE_PS1.contains("function Assert-NsisIconMetadata")
            && WINDOWS_SMOKE_PS1.contains("WScript.Shell")
            && WINDOWS_SMOKE_PS1
                .contains("TerminalTiler\\Uninstall TerminalTiler.lnk")
            && WINDOWS_SMOKE_PS1.contains("IconLocation")
            && WINDOWS_SMOKE_PS1.contains("DisplayIcon")
            && WINDOWS_SMOKE_PS1
                .contains("Assert-NsisIconMetadata -InstallRoot $NsisInstallRoot")
            && WINDOWS_PORTABLE_NSI.contains("Icon \"${ICON_FILE}\"")
            && WINDOWS_INSTALLER_NSI.contains("Icon \"${ICON_FILE}\"")
            && WINDOWS_INSTALLER_NSI.contains("UninstallIcon \"${ICON_FILE}\"")
            && WINDOWS_INSTALLER_NSI.contains(
                "CreateShortcut \"$SMPROGRAMS\\TerminalTiler\\TerminalTiler.lnk\" \"$INSTDIR\\TerminalTiler.exe\" \"\" \"$INSTDIR\\share\\terminaltiler.ico\""
            )
            && WINDOWS_INSTALLER_NSI.contains(
                "WriteRegStr HKCU \"Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\TerminalTiler\" \"DisplayIcon\" \"$INSTDIR\\share\\terminaltiler.ico\""
            )
            && WINDOWS_INSTALLER_WXS
                .contains(r#"<Icon Id="TerminalTilerIcon" SourceFile="$(var.IconFile)" />"#)
            && WINDOWS_INSTALLER_WXS.contains(r#"ARPPRODUCTICON"#)
            && WINDOWS_INSTALLER_WXS.contains(r#"Icon="TerminalTilerIcon""#),
        "Windows portable exe, installer, MSI shortcut, staged payload, and smoke checks should all carry the TerminalTiler icon"
    );

    assert!(
        GTK_SHELL_RS.contains("pub const APP_ICON_NAME: &str = \"terminaltiler\"")
            && GTK_SHELL_RS.contains("pub fn configure_application_icons()")
            && GTK_SHELL_RS.contains("gtk::IconTheme::for_display")
            && GTK_SHELL_RS.contains("icon_theme.add_search_path(path)")
            && GTK_SHELL_RS.contains("gtk::Window::set_default_icon_name(APP_ICON_NAME)")
            && WINDOW_RS.contains(".icon_name(gtk_shell::APP_ICON_NAME)")
            && WINDOWS_GTK_APP_RS.contains("crate::gtk_shell::configure_application_icons()")
            && WINDOWS_GTK_APP_RS.contains(".icon_name(crate::gtk_shell::APP_ICON_NAME)")
            && WINDOWS_GTK_APP_RS
                .contains("const WINDOWS_APP_USER_MODEL_ID: &str = \"Zethrus.TerminalTiler\"")
            && WINDOWS_GTK_APP_RS
                .contains("configure_windows_taskbar_identity(taskbar_app_user_model_id)")
            && source_contains(
                WINDOWS_GTK_APP_RS,
                ".app_id\n            .as_deref()\n            .unwrap_or(WINDOWS_APP_USER_MODEL_ID)",
            )
            && WINDOWS_GTK_APP_RS
                .contains("windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID")
            && CARGO_TOML.contains("\"Win32_UI_Shell\""),
        "GTK windows should set the same app icon-name that Windows portable packages stage in the icon theme, and Windows builds should set a stable product-aware taskbar AppUserModelID so icon parity does not depend only on installer metadata"
    );
}

#[test]
fn windows_gtk_shell_has_targeted_density_normalization_without_touching_linux() {
    for selector in [
        "window.windows-gtk-shell.profile-standard headerbar.app-headerbar",
        "window.windows-gtk-shell.profile-standard .app-tab-shell",
        "window.windows-gtk-shell.profile-standard .app-tab-icon",
        "window.windows-gtk-shell.profile-standard .app-tab-title",
        "window.windows-gtk-shell.profile-standard button.app-tab-close",
        "window.windows-gtk-shell.profile-standard button.app-tab-add",
        "window.windows-gtk-shell.profile-standard headerbar.app-headerbar button.titlebar-icon-button",
        "window.windows-gtk-shell button.primary-cta-button",
        "window.windows-gtk-shell button.pill-button.compact-action-button",
        "window.windows-gtk-shell button.pill-button.compact-icon-button",
        "window.windows-gtk-shell button.surface-button",
        "window.windows-gtk-shell combobox.surface-select-control button.combo",
        "window.windows-gtk-shell .workspace-summary",
        "window.windows-gtk-shell .launch-dashboard-hero",
        "window.windows-gtk-shell .launch-dashboard-title",
        "window.windows-gtk-shell .launch-dashboard-copy",
        "window.windows-gtk-shell .terminal-header",
        "window.windows-gtk-shell .saved-workspaces-panel .section-title",
        "window.windows-gtk-shell .saved-workspace-card",
        "window.windows-gtk-shell .saved-workspace-card .card-meta",
        "window.windows-gtk-shell .saved-workspace-root",
        "window.windows-gtk-shell .saved-workspace-tile-chip",
        "window.windows-gtk-shell .settings-section",
        ".parity-dialog-window.windows-gtk-shell button",
        ".parity-dialog-window.windows-gtk-shell entry",
        ".parity-dialog-window.windows-gtk-shell .field-hint",
        ".settings-dialog-window.windows-gtk-shell .settings-section",
        ".settings-dialog-window.windows-gtk-shell .settings-dialog-content .config-panel",
        ".settings-dialog-window.windows-gtk-shell .settings-shortcut-chip",
    ] {
        assert!(
            STYLE_CSS.contains(selector),
            "Windows GTK density normalization should be scoped to the Windows shell selector: {selector}"
        );
    }

    let normalized_style = STYLE_CSS.replace("\r\n", "\n");
    let normalized_settings_dialog = SETTINGS_DIALOG_RS.replace("\r\n", "\n");
    let normalized_icons = ICONS_RS.replace("\r\n", "\n");
    assert!(
        normalized_style.contains("window.windows-gtk-shell .saved-workspace-card {\n  min-height: 118px;\n  padding: 13px;")
            && normalized_style.contains("window.windows-gtk-shell.profile-standard button.app-tab-add {\n  min-width: 24px;\n  min-height: 24px;\n  padding: 0;")
            && normalized_style.contains("window.windows-gtk-shell.profile-standard button.app-tab-close {\n  min-width: 18px;\n  min-height: 18px;")
            && normalized_style.contains("window.windows-gtk-shell.profile-standard headerbar.app-headerbar button.titlebar-icon-button {\n  min-width: 28px;\n  padding: 0;")
            && normalized_style.contains("window.windows-gtk-shell button.pill-button.compact-icon-button {\n  min-width: 28px;\n  min-height: 28px;")
            && normalized_style.contains("window.windows-gtk-shell combobox.surface-select-control button.combo {\n  min-height: 34px;")
            && normalized_style.contains("window.windows-gtk-shell .launch-dashboard-hero {\n  padding: 12px 15px;")
            && normalized_style.contains("window.windows-gtk-shell .launch-dashboard-title {\n  font-size: 20px;")
            && normalized_style.contains("window.windows-gtk-shell .saved-workspaces-panel {\n  padding: 12px;")
            && normalized_style.contains("window.windows-gtk-shell .saved-workspaces-panel .section-title {\n  font-size: 17px;")
            && normalized_style.contains("window.windows-gtk-shell .saved-workspace-card .card-meta,\nwindow.windows-gtk-shell .saved-workspace-root {\n  font-size: 11px;")
            && normalized_style.contains("window.windows-gtk-shell .saved-workspace-tile-chip {\n  padding: 3px 8px;")
            && normalized_style.contains(".settings-dialog-window.windows-gtk-shell .settings-section {\n  padding: 12px;\n  border-radius: 18px;")
            && normalized_style.contains(".parity-dialog-window.windows-gtk-shell button {\n  min-height: 30px;")
            && normalized_style.contains(".parity-dialog-window.windows-gtk-shell entry {\n  min-height: 34px;")
            && normalized_settings_dialog.contains("\"windows-gtk-shell\"")
            && normalized_icons.contains("fn button_icon_pixel_size() -> i32")
            && normalized_icons.contains("target_os = \"windows\", feature = \"windows-gtk-shell\"")
            && normalized_icons.contains("13\n    } else {\n        15")
            && normalized_icons.contains("fn button_icon_spacing() -> i32")
            && normalized_icons.contains("5\n    } else {\n        6")
            && DIALOG_CHROME_RS.contains("source_has_chrome_class(parent.as_ref(), class_name)"),
        "Windows-only CSS should trim the card/action/select metrics that made the screenshots look chunkier than Linux"
    );
    assert!(
        normalized_settings_dialog.contains(
            "let min_width = if window.has_css_class(\"windows-gtk-shell\") {\n        640"
        ) && normalized_settings_dialog.contains(
            "let min_height = if window.has_css_class(\"windows-gtk-shell\") {\n        620"
        ) && normalized_settings_dialog.contains("saved_width.max(min_width)")
            && normalized_settings_dialog.contains("saved_height.max(min_height)"),
        "Windows GTK settings dialogs should keep a Linux-like readable footprint even when older saved Windows preferences are narrower"
    );
    assert!(
        UI_MOD_RS.contains("pub(crate) mod dialog_chrome;")
            && DIALOG_CHROME_RS.contains("PARITY_DIALOG_CLASS")
            && DIALOG_CHROME_RS.contains("dialog.as_ref()")
            && DIALOG_CHROME_RS.contains("source_has_chrome_class")
            && DIALOG_CHROME_RS.contains("root.has_css_class(class_name)")
            && DIALOG_CHROME_RS.contains("pub(crate) fn sync_popover_chrome_classes")
            && DIALOG_CHROME_RS.contains("\"parity-dialog-window\"")
            && DIALOG_CHROME_RS.contains("\"windows-gtk-shell\"")
            && ABOUT_DIALOG_RS.contains("dialog_chrome::sync_dialog_chrome_classes(window, &dialog, \"about-dialog-window\")")
            && ASSETS_MANAGER_RS.contains("dialog_chrome::sync_dialog_chrome_classes(window, &dialog, \"assets-manager-window\")")
            && ASSETS_MANAGER_RS.matches("dialog_chrome::sync_dialog_chrome_classes(dialog, &prompt, \"assets-discard-prompt-window\")").count() == 2
            && COMMAND_PALETTE_RS.contains("dialog_chrome::sync_dialog_chrome_classes(window, &dialog, \"command-palette-window\")")
            && COMPANION_DIALOG_RS.contains("dialog_chrome::sync_dialog_chrome_classes(window, &dialog, \"companion-dialog-window\")")
            && COMPANION_DIALOG_RS.contains("dialog_chrome::sync_dialog_chrome_classes(window, &dialog, \"companion-input-dialog-window\")")
            && RUNBOOK_DIALOG_RS.contains("dialog_chrome::sync_dialog_chrome_classes(&window, &dialog, \"runbook-dialog-window\")")
            && TAB_RENAME_DIALOG_RS.contains("dialog_chrome::sync_dialog_chrome_classes(window, &dialog, \"tab-rename-dialog-window\")")
            && TRANSCRIPT_DIALOG_RS.contains("dialog_chrome::sync_dialog_chrome_classes(&window, &dialog, \"transcript-dialog-window\")")
            && SETTINGS_DIALOG_RS.contains("dialog_chrome::sync_dialog_chrome_classes(window, dialog, \"settings-dialog-window\")")
            && LAUNCH_SCREEN_RS.contains("dialog_chrome::sync_dialog_chrome_classes(win, &dialog, \"launch-delete-preset-dialog\")")
            && LAUNCH_SCREEN_RS.contains("dialog_chrome::sync_dialog_chrome_classes(win, &dialog, \"launch-folder-picker-dialog\")")
            && LAUNCH_SCREEN_RS.contains("dialog_chrome::sync_dialog_chrome_classes(win, &dialog, \"launch-save-preset-dialog\")")
            && DIALOG_CHROME_RS.contains("sync_dialog_chrome_classes(window, &dialog, \"destructive-confirm-dialog\")")
            && WINDOW_RS.contains("dialog_chrome::confirm_destructive_action")
            && WINDOW_RS.contains("dialog_chrome::sync_dialog_chrome_classes(window, &dialog, \"tab-close-confirm-dialog\")")
            && WINDOW_RS.contains("dialog_chrome::sync_dialog_chrome_classes(window, &dialog, \"session-resume-dialog\")")
            && WINDOW_RS.contains("dialog_chrome::sync_dialog_chrome_classes(window, &dialog, \"startup-notice-dialog\")")
            && CONTEXT_MENU_RS.contains("dialog_chrome::sync_popover_chrome_classes(parent, &popover, \"terminal-context-popover-window\")")
            && SNIPPET_POPOVER_RS.contains("dialog_chrome::sync_popover_chrome_classes(button, &popover, \"snippet-popover-window\")")
            && TERMINAL_RECOVERY_POPOVER_RS.contains("dialog_chrome::sync_popover_chrome_classes(")
            && TERMINAL_RECOVERY_POPOVER_RS.contains("\"terminal-recovery-popover-window\"")
            && SETTINGS_DIALOG_RS.contains("dialog_chrome::sync_popover_chrome_classes(")
            && SETTINGS_DIALOG_RS.contains("\"settings-help-popover-window\"")
            && source_contains(SETTINGS_DIALOG_RS, "build_shortcut_recorder_row(\n        &dialog,")
            && SETTINGS_DIALOG_RS.contains("build_shortcut_entry_row(parent,")
            && TILE_CHROME_RS.contains("dialog_chrome::sync_popover_chrome_classes(")
            && TILE_CHROME_RS.contains("\"web-tile-settings-popover-window\""),
        "shared GTK dialogs, prompts, and popovers should inherit platform/theme/density classes so Windows parity fixes apply beyond the main shell"
    );
}

#[test]
fn windows_packaging_stages_shared_gtk_resources_and_smoke_checks_payload() {
    assert!(
        CI_YML.contains("verify-windows-gtk")
            && CI_YML.contains("setup-windows-gtk.ps1 -InstallWithGvsbuild")
            && CI_YML.contains("windows-gtk-runtime-gvsbuild-v4")
            && CI_YML.contains("actions/cache@v5")
            && !CI_YML.contains("save-always:")
            && CI_YML.contains("actions/upload-artifact@v6")
            && CI_YML.contains("cargo check --target x86_64-pc-windows-msvc --features voice-cpal,windows-gtk-shell")
            && CI_YML.contains("build-windows.ps1 -UseGtkShell")
            && CI_YML.contains("windows-smoke-test.ps1 -UseGtkShell"),
        "CI should include native Windows GTK build, package, smoke coverage, and Node 24-ready artifact/cache actions"
    );

    assert!(
        WINDOWS_SETUP_GTK_PS1.contains("gvsbuild")
            && WINDOWS_SETUP_GTK_PS1.contains("TERMINALTILER_GTK_RUNTIME_ROOT")
            && WINDOWS_SETUP_GTK_PS1.contains("PKG_CONFIG_PATH")
            && WINDOWS_SETUP_GTK_PS1.contains("libadwaita-1")
            && WINDOWS_SETUP_GTK_PS1.contains("RUSTUP_TOOLCHAIN = \"stable\"")
            && WINDOWS_SETUP_GTK_PS1.contains("RUSTUP_HOME")
            && WINDOWS_SETUP_GTK_PS1.contains("CARGO_HOME")
            && WINDOWS_SETUP_GTK_PS1.contains("GITHUB_ENV"),
        "Windows GTK setup script should provision/export native GTK/libadwaita build environment"
    );

    for payload in [
        "resources\\style.css",
        "resources\\terminaltiler.svg",
        "resources\\hover-icons\\*.svg",
        "Copy-WindowsGtkRuntime",
        "TERMINALTILER_GTK_RUNTIME_ROOT",
        "[switch]$UseWin32Shell",
        "$BuildGtkShell = -not $UseWin32Shell",
        "Assert-DirectoryExists",
        "Test-DirectoryHasFiles",
        "Assert-DirectoryHasFiles",
        "Assert-GtkRuntimeResource",
        "Assert-WindowsStagedPayload",
        "portable.nsi",
    ] {
        assert!(
            WINDOWS_BUILD_PS1.contains(payload),
            "Windows packaging should default to staging the canonical GTK/libadwaita parity payload: {payload}"
        );
    }

    assert!(
        WINDOWS_BUILD_PS1
            .contains("GTK runtime root is required for the canonical Windows GTK payload")
            && WINDOWS_BUILD_PS1.contains("Use -UseWin32Shell only for an explicit fallback build")
            && WINDOWS_BUILD_PS1.contains("Assert-GtkRuntimeResource -Path $source")
            && WINDOWS_BUILD_PS1
                .contains("Assert-DirectoryHasFiles -Path (Join-Path $PortableRoot $relative)")
            && WINDOWS_BUILD_PS1.contains("\"-ke\"")
            && WINDOWS_BUILD_PS1.contains("@{ Path = \"lib\\gio\"; AllowEmpty = $true }")
            && WINDOWS_BUILD_PS1.contains("@{ Path = \"lib\\gtk-4.0\"; AllowEmpty = $true }")
            && WINDOWS_BUILD_PS1.contains("@{ Path = \"share\\themes\"; AllowEmpty = $true }")
            && WINDOWS_BUILD_PS1.contains("retaining empty payload directory")
            && WINDOWS_BUILD_PS1
                .contains("Assert-DirectoryExists -Path (Join-Path $PortableRoot \"lib\\gio\")")
            && WINDOWS_BUILD_PS1.contains(
                "Assert-DirectoryExists -Path (Join-Path $PortableRoot \"lib\\gtk-4.0\")"
            )
            && WINDOWS_BUILD_PS1.contains(
                "Assert-DirectoryExists -Path (Join-Path $PortableRoot \"share\\themes\")"
            )
            && !WINDOWS_BUILD_PS1.contains("terminaltiler-runtime-dir.txt")
            && !WINDOWS_BUILD_PS1.contains("staging directory sentinel"),
        "Windows packaging should require real GTK runtime/resource payload directories before building release artifacts while allowing legitimately empty/missing optional module dirs"
    );

    assert!(
        WINDOWS_PORTABLE_NSI.contains("InitPluginsDir")
            && WINDOWS_PORTABLE_NSI.contains(r#"File /r "${STAGE_DIR}\*""#)
            && WINDOWS_PORTABLE_NSI.contains(r#"SetOutPath "$EXEDIR""#)
            && WINDOWS_PORTABLE_NSI.contains("default workspace root")
            && WINDOWS_PORTABLE_NSI.contains(r#"ExecWait '"$PLUGINSDIR\TerminalTiler.exe"' $0"#)
            && WINDOWS_PORTABLE_NSI.contains(r#"RMDir /r "$PLUGINSDIR""#)
            && WINDOWS_PORTABLE_NSI.contains("SetErrorLevel $0"),
        "direct portable exe should self-extract the staged payload, launch with a stable wrapper-directory cwd instead of the temp extraction root, and clean its temp extraction root"
    );

    assert!(
        WINDOWS_GTK_SMOKE_PS1.contains("setup-windows-gtk.ps1")
            && WINDOWS_GTK_SMOKE_PS1.contains("build-windows.ps1")
            && WINDOWS_GTK_SMOKE_PS1.contains("windows-smoke-test.ps1")
            && WINDOWS_GTK_SMOKE_PS1.contains("UseGtkShell"),
        "dedicated Windows GTK smoke script should run setup, package build, and package smoke"
    );

    for payload in [
        "share\\style.css",
        "share\\terminaltiler.svg",
        "share\\hover-icons\\terminal.svg",
        "share\\hover-icons\\layout-dashboard.svg",
        "share\\hover-icons\\save.svg",
        "etc",
        "lib\\gdk-pixbuf-2.0",
        "lib\\gio",
        "lib\\gtk-4.0",
        "share\\icons",
        "share\\themes",
        "share\\glib-2.0",
    ] {
        assert!(
            WINDOWS_SMOKE_PS1.contains(payload),
            "Windows smoke test should assert GTK parity payload: {payload}"
        );
    }
    assert!(
        WINDOWS_SMOKE_PS1.contains("function Assert-DirectoryHasFiles")
            && WINDOWS_SMOKE_PS1.contains("did not contain any files")
            && WINDOWS_SMOKE_PS1
                .contains("Assert-DirectoryHasFiles -Path (Join-Path $PayloadRoot $relative)")
            && WINDOWS_SMOKE_PS1
                .contains("Assert-Path -Path (Join-Path $PayloadRoot \"lib\\gio\")")
            && WINDOWS_SMOKE_PS1
                .contains("Assert-Path -Path (Join-Path $PayloadRoot \"lib\\gtk-4.0\")")
            && WINDOWS_SMOKE_PS1
                .contains("Assert-Path -Path (Join-Path $PayloadRoot \"share\\themes\")"),
        "Windows smoke test should verify GTK runtime/resource directories are populated where gvsbuild ships files and present for optional module dirs"
    );

    assert!(
        WINDOWS_SMOKE_PS1.contains("windows GTK shell startup")
            && WINDOWS_SMOKE_PS1.contains("windows GTK shell loaded canonical GTK CSS")
            && LAUNCH_SCREEN_RS.contains("GTK launch deck default workspace root resolved to")
            && WINDOWS_SMOKE_PS1.contains("GTK launch deck default workspace root resolved to")
            && WINDOWS_SMOKE_PS1.contains("[string]$ExpectedLaunchRoot = \"\"")
            && WINDOWS_SMOKE_PS1
                .contains("did not launch from the expected stable wrapper directory")
            && WINDOWS_SMOKE_PS1.contains("temporary NSIS extraction directory")
            && WINDOWS_SMOKE_PS1.contains(
                "-ExpectedLaunchRoot (Split-Path -Parent (Resolve-Path $PortableExePath).Path)"
            )
            && WINDOWS_SMOKE_PS1
                .contains("Windows GTK shell restored interactive GTK workspace with")
            && WINDOWS_SMOKE_PS1.contains("unexpectedly opened the legacy Win32 workspace host")
            && WINDOWS_SMOKE_PS1.contains("Test-ProcessTreeHasMainWindow")
            && WINDOWS_SMOKE_PS1
                .contains("$mainWindowTimeoutSeconds = if ($expectGtkShell) { 20 } else { 8 }")
            && WINDOWS_SMOKE_PS1.contains("continuing with GTK session-log validation."),
        "Windows smoke test should validate GTK startup/restored-runtime logs even for self-extracting portable launchers"
    );

    let smoke_script = WINDOWS_SMOKE_PS1.replace("\r\n", "\n");
    let pattern_helper_start = smoke_script
        .find("function Get-LaunchSmokeRequiredPattern")
        .expect("Windows smoke test should keep launch-smoke wait patterns centralized");
    let invoke_launch_smoke_start = smoke_script
        .find("function Invoke-LaunchSmoke")
        .expect("Windows smoke test should keep Invoke-LaunchSmoke");
    assert!(
        pattern_helper_start < invoke_launch_smoke_start,
        "launch-smoke wait-pattern helper should be defined before Invoke-LaunchSmoke"
    );
    let pattern_helper = &smoke_script[pattern_helper_start..invoke_launch_smoke_start];
    let gtk_mixed_wait = pattern_helper
        .find("Windows GTK WebView2 tile navigating to https://example.com")
        .expect("GTK mixed smoke should wait for WebView2 navigation");
    let gtk_terminal_wait = pattern_helper
        .find("Windows GTK shell restored interactive GTK workspace with")
        .expect("GTK terminal-only smoke should wait for generic workspace restore");
    assert!(
        gtk_mixed_wait < gtk_terminal_wait
            && source_contains(
                pattern_helper,
                "if ($ProfileKind -eq \"mixed\") {\n            return \"Windows GTK WebView2 tile navigating to https://example.com\"\n        }\n        return \"Windows GTK shell restored interactive GTK workspace with\"",
            )
            && WINDOWS_SMOKE_PS1.contains(
                "$requiredPattern = Get-LaunchSmokeRequiredPattern -ExpectGtkShell $expectGtkShell -ProfileKind $ProfileKind",
            )
            && WINDOWS_SMOKE_PS1.contains("$GtkMixedWebView2SmokeTimeoutSeconds = 75")
            && WINDOWS_SMOKE_PS1.contains("Resolved WebView2 user data folder:")
            && WINDOWS_SMOKE_PS1.contains("local-data\\webview2"),
        "GTK mixed launch smoke must wait for the WebView2 navigation log long enough to cover WebView2 initialization, instead of using the earlier generic GTK restore signal"
    );

    assert!(
        WINDOWS_SMOKE_PS1.contains("function Stop-TerminalTilerSmokeProcesses")
            && WINDOWS_SMOKE_PS1.contains("TerminalTiler*.exe")
            && WINDOWS_SMOKE_PS1.contains("function Wait-ProcessOrTimeout")
            && WINDOWS_SMOKE_PS1.contains("timed out after $TimeoutSeconds seconds")
            && WINDOWS_SMOKE_PS1.contains("Wait-ProcessOrTimeout -Process $InstallerProcess")
            && WINDOWS_SMOKE_PS1.contains("function Invoke-MsiExecWithRetry")
            && WINDOWS_SMOKE_PS1.contains("Wait-ProcessOrTimeout -Process $process")
            && WINDOWS_SMOKE_PS1.contains("retrying after runner cleanup.")
            && WINDOWS_SMOKE_PS1.contains("Invoke-MsiExecWithRetry -ArgumentList"),
        "Windows smoke test should bound installer waits, retry transient msiexec runner failures, and clean up TerminalTiler smoke processes so CI cannot hang indefinitely"
    );

    for workflow in [RELEASE_YML, PACKAGE_ARTIFACTS_YML] {
        assert!(
            workflow.contains("setup-windows-gtk.ps1 -InstallWithGvsbuild -SkipBuildIfPresent")
                && workflow.contains("build-windows.ps1")
                && workflow.contains("-UseGtkShell -GtkRuntimeRoot $env:TERMINALTILER_GTK_RUNTIME_ROOT -RequireInstallers")
                && workflow.contains("windows-smoke-test.ps1")
                && workflow.contains("-UseGtkShell -GtkRuntimeRoot $env:TERMINALTILER_GTK_RUNTIME_ROOT -SmokeProfileKind terminal-only -SkipBuild"),
            "release/package workflows should publish only GTK/libadwaita parity Windows artifacts by default"
        );
        assert!(
            !workflow.contains("-UseWin32Shell"),
            "release/package workflows must not publish the explicit Win32 fallback path"
        );
        assert!(
            workflow.contains("actions/cache@v5")
                && workflow.contains("continue-on-error: true")
                && !workflow.contains("actions/cache@v4")
                && !workflow.contains("actions/upload-artifact@v4")
                && !workflow.contains("save-always:"),
            "release/package workflows should keep Windows GTK artifact publishing on Node 24-ready cache/artifact actions"
        );
    }

    assert!(
        WINDOWS_BUILD_PS1.contains("function Save-WebView2Bootstrapper")
            && WINDOWS_BUILD_PS1.contains("Invoke-WebRequest -Uri $Uri")
            && WINDOWS_BUILD_PS1.contains("Assert-NonEmptyFile")
            && WINDOWS_BUILD_PS1.contains("MicrosoftEdgeWebview2Setup.exe")
            && WINDOWS_BUILD_PS1.contains("https://go.microsoft.com/fwlink/p/?LinkId=2124703")
            && WINDOWS_BUILD_PS1.contains(r#""/DWEBVIEW2_BOOTSTRAPPER=$WebView2BootstrapperPath""#)
            && WINDOWS_INSTALLER_NSI.contains("WEBVIEW2_BOOTSTRAPPER")
            && WINDOWS_INSTALLER_NSI.contains("{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}")
            && WINDOWS_INSTALLER_NSI.contains("Function DetectWebView2Runtime")
            && WINDOWS_INSTALLER_NSI.contains("SetRegView 32")
            && WINDOWS_INSTALLER_NSI.contains("SetRegView 64")
            && WINDOWS_INSTALLER_NSI.contains(r#"ReadRegStr $1 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2_CLIENT_GUID}" "pv""#)
            && WINDOWS_INSTALLER_NSI.contains(r#"ReadRegStr $1 HKCU "Software\Microsoft\EdgeUpdate\Clients\${WEBVIEW2_CLIENT_GUID}" "pv""#)
            && WINDOWS_INSTALLER_NSI.contains("Call EnsureWebView2Runtime")
            && WINDOWS_INSTALLER_NSI.contains("File /oname=MicrosoftEdgeWebview2Setup.exe")
            && WINDOWS_INSTALLER_NSI.contains(r#"ExecWait '"$PLUGINSDIR\MicrosoftEdgeWebview2Setup.exe" /silent /install' $1"#)
            && WINDOWS_INSTALLER_NSI.contains("SetErrorLevel 2")
            && WINDOWS_INSTALLER_NSI.contains("MessageBox MB_ICONEXCLAMATION|MB_OK")
            && !WINDOWS_INSTALLER_NSI.contains("Browser tiles require Microsoft Edge WebView2 Runtime. Install the Evergreen runtime")
            && WINDOWS_SMOKE_PS1.contains(r#"$NsisSmokeProfileKind = "mixed""#)
            && WINDOWS_SMOKE_PS1.contains("Windows GTK WebView2 tile navigating to https://example.com"),
        "Windows setup installer should bundle the WebView2 Evergreen bootstrapper, silently install it only when the runtime is missing, and smoke the installed browser tile path"
    );

    assert!(
        WINDOWS_INSTALLER_TOOLS_PS1.contains("heat.exe")
            && WINDOWS_BUILD_PS1.contains("HarvestedPayloadComponents")
            && WINDOWS_BUILD_PS1.contains(r#""-var" "var.StageDir""#)
            && WINDOWS_BUILD_PS1.contains(r#""-arch" "x64""#)
            && WINDOWS_BUILD_PS1.contains(r#""-sice:ICE38""#)
            && WINDOWS_BUILD_PS1.contains(r#""-sice:ICE64""#)
            && WINDOWS_INSTALLER_WXS
                .contains(r#"ComponentGroupRef Id="HarvestedPayloadComponents""#),
        "MSI packaging should harvest the full staged payload, including CSS, logo, hover icons, and GTK runtime files"
    );
}

#[test]
fn release_publishes_only_after_all_platform_artifacts_are_available() {
    let linux_release = workflow_job_block(RELEASE_YML, "release-linux");
    let windows_release = workflow_job_block(RELEASE_YML, "release-windows");
    let publish_release = workflow_job_block(RELEASE_YML, "publish-release");
    let publish_assets_step = publish_release
        .split("- name: Publish release assets")
        .nth(1)
        .expect("publish-release should keep a final GitHub Release step");

    assert!(
        workflow_job_block(RELEASE_YML, "resolve-release")
            .contains("tag: ${{ steps.release_meta.outputs.tag }}")
            && linux_release.contains("needs: resolve-release")
            && windows_release.contains("needs: resolve-release"),
        "release workflow should resolve the release tag/package paths once before platform jobs build artifacts"
    );

    assert!(
        linux_release.contains("Upload Linux release artifacts")
            && linux_release.contains("actions/upload-artifact@v6")
            && linux_release.contains(
                "terminaltiler-release-linux-${{ needs.resolve-release.outputs.package_version }}"
            )
            && linux_release.contains("${{ needs.resolve-release.outputs.deb_path }}")
            && linux_release.contains("${{ needs.resolve-release.outputs.appimage_path }}")
            && linux_release.contains("if-no-files-found: error")
            && !linux_release.contains("softprops/action-gh-release"),
        "Linux release job should upload validated artifacts for the final publisher instead of independently creating/updating a GitHub Release"
    );

    assert!(
        windows_release.contains("Upload Windows release artifacts")
            && windows_release.contains("actions/upload-artifact@v6")
            && windows_release.contains(
                "terminaltiler-release-windows-${{ needs.resolve-release.outputs.package_version }}"
            )
            && windows_release
                .contains("${{ needs.resolve-release.outputs.windows_portable_exe_path }}")
            && windows_release.contains("${{ needs.resolve-release.outputs.windows_zip_path }}")
            && windows_release
                .contains("${{ needs.resolve-release.outputs.windows_installer_path }}")
            && windows_release.contains("${{ needs.resolve-release.outputs.windows_msi_path }}")
            && windows_release.contains("if-no-files-found: error")
            && !windows_release.contains("softprops/action-gh-release"),
        "Windows release job should upload validated artifacts for the final publisher instead of racing Linux to publish"
    );

    assert!(
        publish_release.contains("needs: [resolve-release, release-linux, release-windows]")
            && publish_release.contains("actions/download-artifact@v8")
            && publish_release.contains("pattern: terminaltiler-release-*-${{ needs.resolve-release.outputs.package_version }}")
            && publish_release.contains("merge-multiple: true")
            && publish_release.contains("Verify release assets before publishing")
            && publish_release.contains("Missing release asset: $file")
            && publish_release.contains("softprops/action-gh-release@v3")
            && !RELEASE_YML.contains("softprops/action-gh-release@v2")
            && !RELEASE_YML.contains("actions/download-artifact@v4")
            && !RELEASE_YML.contains("actions/upload-artifact@v4"),
        "Release publishing should wait for both platform jobs, download merged artifacts with Node 24-ready actions, verify them, and publish once"
    );

    for asset_output in [
        "${{ needs.resolve-release.outputs.deb_path }}",
        "${{ needs.resolve-release.outputs.appimage_path }}",
        "${{ needs.resolve-release.outputs.windows_portable_exe_path }}",
        "${{ needs.resolve-release.outputs.windows_zip_path }}",
        "${{ needs.resolve-release.outputs.windows_installer_path }}",
        "${{ needs.resolve-release.outputs.windows_msi_path }}",
    ] {
        assert!(
            publish_assets_step.contains(asset_output),
            "final release publisher should include expected asset output {asset_output}"
        );
    }
}

#[test]
fn package_artifacts_waits_for_successful_ci_on_main() {
    assert!(
        PACKAGE_ARTIFACTS_YML.contains("workflow_run:")
            && PACKAGE_ARTIFACTS_YML.contains("workflows: [CI]")
            && PACKAGE_ARTIFACTS_YML.contains("types: [completed]")
            && PACKAGE_ARTIFACTS_YML.contains("github.event.workflow_run.conclusion == 'success'")
            && PACKAGE_ARTIFACTS_YML.contains(
                "github.event.workflow_run.head_branch == github.event.repository.default_branch"
            )
            && PACKAGE_ARTIFACTS_YML
                .contains("ref: ${{ github.event.workflow_run.head_sha || github.ref }}")
            && PACKAGE_ARTIFACTS_YML.contains(
                "CI_BUILD_NUMBER: ${{ github.event.workflow_run.run_number || github.run_number }}"
            ),
        "Package Artifacts should build the exact commit that completed the full CI workflow successfully on the default branch"
    );

    assert!(
        !PACKAGE_ARTIFACTS_YML.contains("\n  push:"),
        "Package Artifacts should not race CI by triggering directly on push"
    );

    assert!(
        RELEASE_SMOKE_TEST_SH.contains(r#"SMOKE_LAUNCH_TIMEOUT="${SMOKE_LAUNCH_TIMEOUT:-60s}""#)
            && RELEASE_SMOKE_TEST_SH
                .contains("did not complete restore within $SMOKE_LAUNCH_TIMEOUT"),
        "Linux release smoke tests should leave enough hosted-runner budget after xvfb/dbus startup before declaring restore failed"
    );
}

#[test]
fn ci_requires_linux_and_windows_verify_jobs_as_hard_gates() {
    let linux_verify = workflow_job_block(CI_YML, "verify");
    assert!(
        linux_verify.contains("runs-on: ubuntu-latest")
            && linux_verify.contains("cargo test --features voice-cpal")
            && linux_verify.contains("cargo clippy --all-targets --all-features -- -D warnings")
            && !source_contains(
                &linux_verify,
                "runs-on: ubuntu-latest\n    continue-on-error: true"
            ),
        "Linux verify should remain a required CI gate for package gating"
    );

    for job_name in ["verify-windows", "verify-windows-gtk"] {
        let windows_verify = workflow_job_block(CI_YML, job_name);
        assert!(
            windows_verify.contains("runs-on: windows-2022")
                && !source_contains(
                    &windows_verify,
                    "runs-on: windows-2022\n    continue-on-error: true",
                ),
            "{job_name} should be an authoritative required CI gate rather than a non-blocking signal job"
        );
    }
}

#[test]
fn windows_smoke_failures_stage_non_hidden_diagnostics_artifacts() {
    assert!(
        WINDOWS_SMOKE_PS1.contains(
            "$DiagnosticsRoot = Join-Path $RootDir \"artifacts\\windows-smoke-diagnostics\""
        ) && WINDOWS_SMOKE_PS1.contains("$script:DiagnosticsRoot = $DiagnosticsRoot")
            && WINDOWS_SMOKE_PS1.contains("Staged Windows smoke diagnostics at $diagnosticRoot")
            && WINDOWS_SMOKE_PS1.contains("summary.txt")
            && WINDOWS_SMOKE_PS1.contains("process-snapshot.txt")
            && WINDOWS_SMOKE_PS1.contains("application-event-log.txt")
            && WINDOWS_SMOKE_PS1.contains("sandbox-tree.txt")
            && WINDOWS_SMOKE_PS1.contains("webview2-tree.txt")
            && WINDOWS_SMOKE_PS1.contains("TEMP = $env:TEMP")
            && WINDOWS_SMOKE_PS1.contains("TMP = $env:TMP")
            && WINDOWS_SMOKE_PS1.contains("$env:TEMP = $profile.Temp")
            && WINDOWS_SMOKE_PS1.contains("$env:TMP = $profile.Tmp")
            && WINDOWS_SMOKE_PS1.contains("Test-PreLogLaunchFailure")
            && WINDOWS_SMOKE_PS1.contains("0xC0000142")
            && WINDOWS_SMOKE_PS1.contains("retrying once after isolated cleanup")
            && WINDOWS_SMOKE_PS1.contains("-LaunchStartTime $launchStartTime")
            && WINDOWS_SMOKE_PS1.contains("StartTime = $LaunchStartTime")
            && WINDOWS_SMOKE_PS1.contains("*$resolvedExePath*")
            && WINDOWS_SMOKE_PS1.contains("Stop-TerminalTilerSmokeProcesses -ThrowOnTimeout"),
        "Windows smoke should isolate TEMP/TMP, retry only pre-log launch initialization failures, filter event logs by launch/exe, and stage diagnostics outside hidden build dirs"
    );

    for job_name in ["verify-windows", "verify-windows-gtk"] {
        let windows_verify = workflow_job_block(CI_YML, job_name);
        assert!(
            windows_verify.contains("actions/upload-artifact@v6")
                && windows_verify.contains("path: artifacts/windows-smoke-diagnostics")
                && windows_verify.contains("if-no-files-found: warn")
                && !windows_verify.contains("path: packaging/.build/windows-smoke"),
            "{job_name} should upload staged Windows smoke diagnostics from a non-hidden artifact directory"
        );
    }
}

#[test]
fn windows_gtk_visual_qa_harness_documents_and_captures_required_views() {
    assert!(
        DOC_WINDOWS_GTK_VISUAL_QA
            .contains("Ubuntu/Linux GTK shell as the canonical visual baseline")
            && DOC_WINDOWS_GTK_VISUAL_QA.contains("capture-linux-gtk-visuals.sh")
            && DOC_WINDOWS_GTK_VISUAL_QA.contains("capture-windows-gtk-visuals.ps1")
            && DOC_WINDOWS_GTK_VISUAL_QA.contains("compare-gtk-visuals.sh")
            && DOC_WINDOWS_GTK_VISUAL_QA.contains("packaging/.build/linux-gtk-visuals/")
            && DOC_WINDOWS_GTK_VISUAL_QA.contains("packaging/.build/gtk-visual-diffs/report.tsv")
            && DOC_WINDOWS_GTK_VISUAL_QA.contains("Launch dashboard")
            && DOC_WINDOWS_GTK_VISUAL_QA.contains("Saved workspace cards")
            && DOC_WINDOWS_GTK_VISUAL_QA.contains("saved-workspaces")
            && DOC_WINDOWS_GTK_VISUAL_QA.contains("workspace-with-web")
            && DOC_WINDOWS_GTK_VISUAL_QA.contains("New/edit wizard")
            && DOC_WINDOWS_GTK_VISUAL_QA
                .contains("Active/restored 3-pane workspace in the shared GTK shell")
            && DOC_WINDOWS_GTK_VISUAL_QA
                .contains("Restored terminal + web workspace in the shared GTK shell")
            && DOC_WINDOWS_GTK_VISUAL_QA.contains("Dark and light themes")
            && DOC_WINDOWS_GTK_VISUAL_QA
                .contains("Comfortable, standard, and compact density modes")
            && DOC_WINDOWS_GTK_VISUAL_QA.contains(
                "Release artifact parity across `portable-exe`, `portable-zip`, `nsis-install`, and `msi-install`"
            )
            && DOC_WINDOWS_GTK_VISUAL_QA.contains(
                "Taskbar, window, installer, and portable-exe icons all show the TerminalTiler icon"
            )
            && DOC_WINDOWS_GTK_VISUAL_QA.contains(
                "never an `nsx*.tmp` self-extraction directory"
            )
            && DOC_WINDOWS_GTK_VISUAL_QA.contains("published self-extracting portable `.exe`"),
        "visual QA documentation should define baseline, capture command, and required comparison screens"
    );

    assert!(
        WINDOWS_CAPTURE_VISUALS_PS1.contains("launch-dashboard")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("saved-workspaces")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("restored-workspace")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("workspace-with-web")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("System.Drawing")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("PrintWindow")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("default_theme")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("default_density")
            && WINDOWS_CAPTURE_VISUALS_PS1
                .contains(r#"[ValidateSet("system", "light", "dark", "all")]"#)
            && WINDOWS_CAPTURE_VISUALS_PS1
                .contains(r#"[ValidateSet("comfortable", "standard", "compact", "all")]"#)
            && WINDOWS_CAPTURE_VISUALS_PS1.contains(r#"$themes = if ($Theme -eq "all")"#)
            && WINDOWS_CAPTURE_VISUALS_PS1.contains(r#"$densities = if ($Density -eq "all")"#)
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("Visual QA Saved Fleet")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("Visual QA Web Workspace")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("tile_kind = \"web-view\"")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("url = \"about:blank\"")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains(
                "\"{0:D2}-{1}-{2}-{3}-{4}.png\" -f $index, $Scenario, $Theme, $Density, $safeTitle"
            )
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("Visual QA Restore")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("return ($Path -replace '\\\\', '\\\\\\\\')")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("Get-ProcessTreeIds")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("Get-DescendantProcessIds")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("Stop-ProcessTree")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("Get-ProcessWindows -ProcessIds $processIds")
            && WINDOWS_CAPTURE_VISUALS_PS1
                .contains(r#"if (-not ("WindowCaptureNative" -as [type]))"#),
        "visual capture helper should seed isolated profiles and capture launcher/workspace windows, including self-extracting launcher child processes"
    );

    assert!(
        WINDOWS_CAPTURE_RELEASE_VISUALS_PS1
            .contains("TerminalTiler-$ResolvedVersion-portable-x86_64.exe")
            && WINDOWS_CAPTURE_RELEASE_VISUALS_PS1
                .contains("TerminalTiler-$ResolvedVersion-windows-x86_64.zip")
            && WINDOWS_CAPTURE_RELEASE_VISUALS_PS1
                .contains("TerminalTiler-setup-$ResolvedVersion-x86_64.exe")
            && WINDOWS_CAPTURE_RELEASE_VISUALS_PS1
                .contains("TerminalTiler-setup-$ResolvedVersion-x86_64.msi")
            && WINDOWS_CAPTURE_RELEASE_VISUALS_PS1.contains("portable-exe")
            && WINDOWS_CAPTURE_RELEASE_VISUALS_PS1.contains("portable-zip")
            && WINDOWS_CAPTURE_RELEASE_VISUALS_PS1.contains("nsis-install")
            && WINDOWS_CAPTURE_RELEASE_VISUALS_PS1.contains("msi-install")
            && WINDOWS_CAPTURE_RELEASE_VISUALS_PS1.contains("saved-workspaces")
            && WINDOWS_CAPTURE_RELEASE_VISUALS_PS1.contains("workspace-with-web")
            && WINDOWS_CAPTURE_RELEASE_VISUALS_PS1
                .contains(r#"[ValidateSet("system", "light", "dark", "all")]"#)
            && WINDOWS_CAPTURE_RELEASE_VISUALS_PS1
                .contains(r#"[ValidateSet("comfortable", "standard", "compact", "all")]"#)
            && WINDOWS_CAPTURE_RELEASE_VISUALS_PS1.contains("Expand-Archive")
            && WINDOWS_CAPTURE_RELEASE_VISUALS_PS1.contains("msiexec.exe")
            && WINDOWS_CAPTURE_RELEASE_VISUALS_PS1
                .contains("-OutputDir (Join-Path $OutputDir $Label)")
            && DOC_WINDOWS_GTK_VISUAL_QA.contains("capture-windows-release-gtk-visuals.ps1"),
        "Windows release visual QA should capture every published GTK artifact shape into separate comparable bundles"
    );

    assert!(
        PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("launch-dashboard")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("saved-workspaces")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("restored-workspace")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("workspace-with-web")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("default_theme")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("default_density")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("--theme system|light|dark|all")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH
                .contains("--density comfortable|standard|compact|all")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("themes=(system light dark)")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH
                .contains("densities=(comfortable standard compact)")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("Visual QA Saved Fleet")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("Visual QA Web Workspace")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("tile_kind = \"web-view\"")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("url = \"about:blank\"")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("'%02d-%s-%s-%s-%s.png'")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("Visual QA Restore")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("TERMINALTILER_PROFILE_ROOT")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("xdotool search --onlyvisible --pid")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("import -window")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("gnome-screenshot")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("Linux GTK visual captures written"),
        "Linux visual capture helper should seed matching baseline profiles and capture comparable GTK reference windows"
    );

    assert!(
        PACKAGE_COMPARE_GTK_VISUALS_SH.contains("normalized RMSE")
            || PACKAGE_COMPARE_GTK_VISUALS_SH.contains("normalized_rmse"),
        "GTK visual comparison helper should report normalized RMSE for screenshot pairs"
    );
    assert!(
        PACKAGE_COMPARE_GTK_VISUALS_SH.contains("launch-dashboard")
            && PACKAGE_COMPARE_GTK_VISUALS_SH.contains("saved-workspaces")
            && PACKAGE_COMPARE_GTK_VISUALS_SH.contains("restored-workspace")
            && PACKAGE_COMPARE_GTK_VISUALS_SH.contains("workspace-with-web")
            && PACKAGE_COMPARE_GTK_VISUALS_SH
                .contains("<index>-<scenario>-<theme>-<density>-*.png")
            && PACKAGE_COMPARE_GTK_VISUALS_SH.contains("--theme system|light|dark|all")
            && PACKAGE_COMPARE_GTK_VISUALS_SH
                .contains("--density comfortable|standard|compact|all")
            && PACKAGE_COMPARE_GTK_VISUALS_SH.contains("themes=(system light dark)")
            && PACKAGE_COMPARE_GTK_VISUALS_SH.contains("densities=(comfortable standard compact)")
            && PACKAGE_COMPARE_GTK_VISUALS_SH
                .contains("scenario\\tindex\\ttheme\\tdensity\\tstatus\\tnormalized_rmse")
            && PACKAGE_COMPARE_GTK_VISUALS_SH.contains("$index-$scenario-$theme-$density-")
            && PACKAGE_COMPARE_GTK_VISUALS_SH.contains("compare -metric RMSE")
            && PACKAGE_COMPARE_GTK_VISUALS_SH.contains("identify -format '%wx%h'")
            && PACKAGE_COMPARE_GTK_VISUALS_SH.contains("fail-dimensions")
            && PACKAGE_COMPARE_GTK_VISUALS_SH.contains("fail-missing-windows")
            && PACKAGE_COMPARE_GTK_VISUALS_SH.contains("fail-threshold")
            && PACKAGE_COMPARE_GTK_VISUALS_SH.contains("report.tsv")
            && PACKAGE_COMPARE_GTK_VISUALS_SH.contains("GTK visual comparison passed."),
        "GTK visual comparison helper should pair Linux/Windows captures and fail on missing, dimension-mismatched, or over-threshold screenshots"
    );
}

#[test]
fn wizard_stepper_uses_dedicated_non_truncating_step_buttons() {
    assert!(
        LAUNCH_SCREEN_RS.contains("fn build_wizard_step_button")
            && LAUNCH_SCREEN_RS.contains("\"wizard-step-chip-content\"")
            && LAUNCH_SCREEN_RS.contains("\"wizard-step-index\"")
            && LAUNCH_SCREEN_RS.contains("\"wizard-step-icon\"")
            && LAUNCH_SCREEN_RS.contains("\"wizard-step-label\""),
        "wizard steps should use a dedicated child layout instead of the generic ellipsizing labeled button"
    );
    assert!(
        !LAUNCH_SCREEN_RS.contains("&format!(\"{}  {}\", index + 1, label)"),
        "wizard steps should not combine number and title into the generic button label"
    );

    assert_css_block_contains(
        ".wizard-step-chip-content",
        "min-width: 0",
        "wizard step content should be a dedicated layout that can shrink cleanly before clipping",
    );

    for selector in [
        ".wizard-step-index",
        ".wizard-step-label",
        ".wizard-step-chip.is-active .wizard-step-index",
        ".wizard-step-chip.is-complete .wizard-step-label",
        "window.window-shell.theme-light .wizard-step-label",
    ] {
        assert_css_block_contains(
            selector,
            "color",
            "dedicated wizard step children should have explicit premium styling hooks",
        );
    }
    assert_css_block_contains(
        ".wizard-step-icon",
        "opacity: 0.70",
        "wizard step icons should have their own visual treatment instead of inheriting generic button icon styles",
    );

    assert_css_block_contains(
        ".wizard-step-chip",
        "min-width: 118px",
        "wizard step buttons should request enough width for full labels at normal wizard sizes",
    );
    assert_css_block_contains(
        ".wizard-step-label",
        "letter-spacing: 0.045em",
        "wizard step labels should remain readable rather than cramped uppercase microcopy",
    );
    assert_css_block_contains(
        "button.wizard-step-chip:focus",
        "outline-color: alpha(@tt_amber, 0.58)",
        "wizard step keyboard focus should share the premium amber focus treatment",
    );
}

#[test]
fn workspace_tab_context_menu_reuses_terminal_context_styles() {
    assert!(
        WINDOW_RS.contains("context_menu::action_button(\"Detach\", None)")
            && WINDOW_RS.contains("context_menu::action_button(\"Reattach\", None)")
            && WINDOW_RS.contains("context_menu::popover(&shell)")
            && WINDOW_RS.contains("context_menu::popover(&title_shell)")
            && WINDOW_RS.contains("build_window_shell()")
            && WINDOW_RS.contains("window_shell.append(&header)")
            && CONTEXT_MENU_RS.contains("terminal-context-popover")
            && CONTEXT_MENU_RS.contains("terminal-context-menu")
            && CONTEXT_MENU_RS.contains("terminal-context-action")
            && CONTEXT_MENU_RS.contains("terminal-context-label"),
        "workspace tab Detach and detached header Reattach should use the shared terminal-context menu styling hooks"
    );
}

#[test]
fn detached_workspace_window_keeps_header_in_adw_content() {
    assert!(
        !WINDOW_RS.contains("window.set_titlebar(Some(&header))")
            && WINDOW_RS.contains("presented detached workspace window"),
        "detached AdwApplicationWindow should keep its header inside content instead of using unsupported gtk_window_set_titlebar"
    );
}

#[test]
fn workspace_tab_drag_stays_left_button_and_uses_title_drop_surface() {
    assert!(
        WINDOW_RS.contains("gtk::DragSource::builder()")
            && WINDOW_RS.contains(".actions(gdk::DragAction::MOVE)")
            && WINDOW_RS.contains(".button(1)")
            && WINDOW_RS.contains("drop_target.connect_enter")
            && WINDOW_RS
                .contains("drop_target.set_propagation_phase(gtk::PropagationPhase::Capture)")
            && WINDOW_RS.contains("translate_coordinates(&self.tabs_box")
            && WINDOW_RS.contains("drop_surface.add_controller(drop_target)")
            && WINDOW_RS.contains("fn suppress_native_tab_drag_icon")
            && WINDOW_RS.contains("gdk::Paintable::new_empty(1, 1)")
            && WINDOW_RS.contains("source.set_icon(Some(&empty_icon), 0, 0)")
            && !WINDOW_RS.contains("gtk::WidgetPaintable::new(Some(&preview))")
            && !WINDOW_RS.contains("gtk::DragIcon::for_drag")
            && !STYLE_CSS.contains(".app-tab-drag-icon")
            && WINDOW_RS.contains("context_menu::action_button(\"Detach\", None)")
            && WINDOW_RS.contains("build_interactive_title_tab(TitleTabInput")
            && TITLE_CHROME_RS.contains("let rename_click = gtk::GestureClick::builder()"),
        "workspace tab drag should be left-button-only, suppress the native multi-monitor drag ghost, update over the full title chrome, and preserve Detach/Rename handlers"
    );
}

#[test]
fn dynamic_tile_header_labels_are_ellipsized_capped_and_tooltipped() {
    assert!(
        TILE_CHROME_RS.contains("fn configure_dynamic_header_label")
            && TILE_CHROME_RS.contains("set_ellipsize(ellipsize)")
            && TILE_CHROME_RS.contains("set_max_width_chars(max_width_chars)")
            && TILE_CHROME_RS.contains("set_single_line_mode(true)")
            && TILE_CHROME_RS.contains("set_tooltip_text(Some(full_text))"),
        "shared tile chrome helper should ellipsize, cap width, stay single-line, and keep full values in tooltips"
    );

    for (source_name, source) in [("terminal tile", TILE_VIEW_RS), ("web tile", WEB_TILE_RS)] {
        assert!(
            source.contains("build_tile_header_chrome(TileHeaderInput"),
            "{source_name} should build visible header labels through the shared tile header helper"
        );
        assert!(
            source.contains("set_tooltip_text(Some(&new_title))"),
            "{source_name} should preserve updated title text in tooltips"
        );
    }

    assert!(
        TILE_CHROME_RS.contains("build_pane_group_chip(&input.tile.pane_groups)")
            && TILE_CHROME_RS.contains("HEADER_GROUP_MAX_CHARS")
            && TILE_CHROME_RS
                .contains("set_tooltip_text(Some(&format!(\"Pane groups: {pane_groups}\")))")
            && TILE_VIEW_RS.contains("set_tooltip_text(Some(&status_line))"),
        "terminal tile pane-group and status chips should truncate text while keeping full tooltip values"
    );
    assert!(
        WEB_TILE_RS.contains("set_tooltip_text(Some(uri.as_str()))"),
        "web tile domain chip should keep the full URL in its tooltip after navigation"
    );
}

fn css_blocks(css: &str) -> Vec<(&str, &str)> {
    let mut blocks = Vec::new();
    let mut remainder = css;

    while let Some(open_index) = remainder.find('{') {
        let selectors = remainder[..open_index].trim();
        let after_open = &remainder[open_index + 1..];
        let Some(close_index) = after_open.find('}') else {
            break;
        };
        let body = &after_open[..close_index];
        blocks.push((selectors, body));
        remainder = &after_open[close_index + 1..];
    }

    blocks
}

fn assert_css_declaration(selector: &str, property: &str, expected_value: &str, reason: &str) {
    let found = css_blocks(STYLE_CSS).into_iter().any(|(selectors, body)| {
        selectors
            .split(',')
            .map(str::trim)
            .any(|candidate| candidate == selector)
            && declaration_value(body, property).is_some_and(|value| value == expected_value)
    });

    assert!(
        found,
        "{selector} must set {property}: {expected_value}; {reason}"
    );
}

fn assert_css_block_contains(selector: &str, expected: &str, reason: &str) {
    let found = css_blocks(STYLE_CSS).into_iter().any(|(selectors, body)| {
        selectors
            .split(',')
            .map(str::trim)
            .any(|candidate| candidate == selector)
            && body.contains(expected)
    });

    assert!(found, "{selector} must contain {expected:?}; {reason}");
}

fn source_contains(source: &str, needle: &str) -> bool {
    source.replace("\r\n", "\n").contains(needle)
}

fn workflow_job_block(workflow: &str, job_name: &str) -> String {
    let normalized = workflow.replace("\r\n", "\n");
    let marker = format!("{job_name}:");
    let mut found = false;
    let mut block = String::new();

    for line in normalized.lines() {
        let is_job_header =
            line.starts_with("  ") && !line.starts_with("    ") && line.trim_end().ends_with(':');

        if is_job_header {
            if found {
                break;
            }

            found = line.trim() == marker;
        }

        if found {
            block.push_str(line);
            block.push('\n');
        }
    }

    assert!(found, "workflow should define job {job_name}");
    block
}

fn declaration_value<'a>(body: &'a str, property: &str) -> Option<&'a str> {
    body.lines().find_map(|line| {
        let declaration = line.trim();
        if declaration.is_empty() || declaration.starts_with("/*") || declaration.starts_with('*') {
            return None;
        }
        let (name, value) = declaration.split_once(':')?;
        if name.trim() == property {
            Some(value.trim().trim_end_matches(';').trim())
        } else {
            None
        }
    })
}

fn is_full_card_state_selector(selector: &str, state: &str) -> bool {
    let Some(index) = selector.find(state) else {
        return false;
    };

    let suffix = &selector[index + state.len()..];
    suffix.trim().is_empty()
        || suffix.starts_with(':')
        || suffix.starts_with('.')
        || suffix.starts_with('[')
        || suffix.starts_with('#')
}

fn assert_forbidden_full_card_ring_properties(selector: &str, body: &str) {
    for property in declaration_property_names(body) {
        assert_ne!(
            property, "box-shadow",
            "{selector} must not add an outer box-shadow ring"
        );
        assert_ne!(
            property, "border-color",
            "{selector} must not recolor the full card border"
        );
        assert_ne!(
            property, "border",
            "{selector} must not add a full-card border"
        );
    }
}

fn declaration_property_names(body: &str) -> impl Iterator<Item = &str> {
    body.lines().filter_map(|line| {
        let declaration = line.trim();
        if declaration.is_empty() || declaration.starts_with("/*") || declaration.starts_with('*') {
            return None;
        }
        declaration
            .split_once(':')
            .map(|(property, _)| property.trim())
    })
}

#[test]
fn kanban_board_styling_extends_existing_design_system() {
    for class in [
        ".kanban-board",
        ".kanban-column",
        ".kanban-column-header",
        ".kanban-count-badge",
        ".kanban-card",
        ".kanban-empty-title",
        ".kanban-agents-panel",
        ".agent-run-row",
    ] {
        assert!(
            STYLE_CSS.contains(class),
            "Kanban board must style {class} so it reads as native chrome"
        );
    }
    // Reuse palette tokens rather than inventing colors.
    assert!(
        STYLE_CSS.contains(".kanban-card.kanban-status-in-progress {")
            && STYLE_CSS.contains("border-left-color: @tt_amber;"),
        "Kanban cards should accent with the shared amber token"
    );
    // The count badge mirrors the alert badge's squared look.
    assert!(
        STYLE_CSS.contains(".kanban-count-badge {") && STYLE_CSS.contains("border-radius: 0;"),
        "Kanban count badge should use the app's squared badge style"
    );
}

#[test]
fn kanban_board_uses_shared_icon_and_dialog_chrome() {
    for (surface, source) in [
        ("board view", BOARD_VIEW_RS),
        ("new task dialog", NEW_TASK_DIALOG_RS),
        ("agent setup dialog", AGENT_SETUP_DIALOG_RS),
    ] {
        assert!(
            source.contains("icons::labeled_button") || source.contains("icons::icon_button"),
            "{surface} should build buttons with the shared symbolic icon helpers"
        );
    }
    // The card/column chrome reuses the app's shared text classes rather than new styles.
    assert!(
        BOARD_CHROME_RS.contains("card-title")
            && BOARD_CHROME_RS.contains("field-hint")
            && BOARD_CHROME_RS.contains("status-chip"),
        "board chrome should reuse the shared card/text classes for parity"
    );
    for (surface, source) in [
        ("new task dialog", NEW_TASK_DIALOG_RS),
        ("agent setup dialog", AGENT_SETUP_DIALOG_RS),
    ] {
        assert!(
            source.contains("dialog_chrome::sync_dialog_chrome_classes"),
            "{surface} should inherit theme/density chrome like other dialogs"
        );
    }
}

#[test]
fn kanban_board_modules_and_mcp_binary_are_wired() {
    assert!(
        UI_MOD_RS.contains("pub mod board_view;")
            && UI_MOD_RS.contains("pub(crate) mod board_chrome;")
            && UI_MOD_RS.contains("pub(crate) mod new_task_dialog;")
            && UI_MOD_RS.contains("pub(crate) mod agent_setup_dialog;"),
        "board UI modules must be declared in ui/mod.rs"
    );
    // The MCP server ships as a bundled binary and the version reflects this large feature.
    assert!(
        CARGO_TOML.contains("name = \"terminaltiler-mcp\""),
        "the bundled MCP binary target must be declared in Cargo.toml"
    );
    assert!(
        CARGO_TOML.contains("version = \"0.3.0\""),
        "the app version should be bumped to 0.3.0 for the Kanban + MCP feature"
    );
}
