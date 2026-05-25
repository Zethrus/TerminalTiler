const STYLE_CSS: &str = include_str!("../resources/style.css");
const ABOUT_DIALOG_RS: &str = include_str!("../src/ui/about_dialog.rs");
const APP_CHROME_RS: &str = include_str!("../src/ui/app_chrome.rs");
const APPEARANCE_RS: &str = include_str!("../src/ui/appearance.rs");
const ASSETS_MANAGER_RS: &str = include_str!("../src/ui/assets_manager.rs");
const COMMAND_PALETTE_RS: &str = include_str!("../src/ui/command_palette.rs");
const CONTEXT_MENU_RS: &str = include_str!("../src/ui/context_menu.rs");
const CARGO_TOML: &str = include_str!("../Cargo.toml");
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
const SETTINGS_DIALOG_RS: &str = include_str!("../src/ui/settings_dialog.rs");
const TERMINAL_SESSION_RS: &str = include_str!("../src/terminal/session.rs");
const TILE_CHROME_RS: &str = include_str!("../src/ui/tile_chrome.rs");
const TITLE_CHROME_RS: &str = include_str!("../src/ui/title_chrome.rs");
const TILE_VIEW_RS: &str = include_str!("../src/ui/tile_view.rs");
const UI_MOD_RS: &str = include_str!("../src/ui/mod.rs");
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
const WINDOWS_GTK_SMOKE_PS1: &str = include_str!("../packaging/build-windows-gtk-smoke.ps1");
const WINDOWS_INSTALLER_TOOLS_PS1: &str = include_str!("../packaging/windows-installer-tools.ps1");
const WINDOWS_INSTALLER_WXS: &str = include_str!("../packaging/windows/installer.wxs");
const WINDOWS_MOD_RS: &str = include_str!("../src/windows/mod.rs");
const WINDOWS_PORTABLE_NSI: &str = include_str!("../packaging/windows/portable.nsi");
const WINDOWS_SETUP_GTK_PS1: &str = include_str!("../packaging/setup-windows-gtk.ps1");
const WINDOWS_SMOKE_PS1: &str = include_str!("../packaging/windows-smoke-test.ps1");
const WORKSPACE_CHROME_RS: &str = include_str!("../src/ui/workspace_chrome.rs");
const WORKSPACE_PREVIEW_RS: &str = include_str!("../src/ui/workspace_preview.rs");
const WORKSPACE_VIEW_RS: &str = include_str!("../src/ui/workspace_view.rs");
const VOICE_ENGINE_RS: &str = include_str!("../src/voice/engine.rs");
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
            && !WINDOWS_GTK_APP_RS.contains("titlebar-action-button")
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
                "&fullscreen_for_click,\n                    &shell_state_for_launch"
            ),
        "Windows GTK should use the same shared workspace fullscreen chrome behavior as Linux for workspace previews and hide it on the launch deck"
    );
}

#[test]
fn terminal_card_state_selectors_do_not_draw_full_card_rings() {
    let mut checked_selectors = Vec::new();

    for (selectors, body) in css_blocks(STYLE_CSS) {
        for selector in selectors.split(',').map(str::trim) {
            for state in TERMINAL_CARD_STATES {
                if is_full_card_state_selector(selector, state) {
                    checked_selectors.push(selector.to_string());
                    assert_forbidden_full_card_ring_properties(selector, body);
                }
            }
        }
    }

    assert!(
        checked_selectors.is_empty(),
        "terminal-card state styling should be header-local; found full-card state selector(s): {}",
        checked_selectors.join(", ")
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
        WINDOWS_APP_RS.contains("ID_SETTINGS_OPEN_LOGS_FOLDER")
            && WINDOWS_APP_RS.contains("open_logs_folder_from_settings")
            && WINDOWS_APP_RS.contains("ShellExecuteW"),
        "Windows settings should expose and handle an Open Logs Folder button"
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
                || source.contains("bind_web_tile_settings_popover"),
            "{surface} should use shared symbolic icon helpers for visible actions"
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
            && WINDOW_RS.contains("build_title_tab_chrome")
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
            && WINDOWS_GTK_APP_RS.contains("save_preview_session")
            && WINDOWS_GTK_APP_RS.contains("terminate_preview_runtimes")
            && WINDOWS_GTK_APP_RS.contains("persist_windows_gtk_session")
            && WINDOWS_GTK_APP_RS.contains("session_store.save(session)")
            && WINDOWS_GTK_APP_RS.contains("session_store.clear()")
            && WINDOWS_GTK_APP_RS.contains("show_launch_deck_tab")
            && WINDOWS_GTK_APP_RS.contains("show_workspace_preview_tab")
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
        WORKSPACE_PREVIEW_RS.contains("build_title_tab_chrome")
            && WORKSPACE_PREVIEW_RS.contains("chrome.select_button.connect_clicked")
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
            && TITLE_CHROME_RS.contains("pub(crate) struct TitleTabChrome")
            && TITLE_CHROME_RS.contains("pub(crate) fn apply_title_tab_state")
            && TITLE_CHROME_RS.contains("chrome.shell.remove_css_class(\"is-inactive\")")
            && TITLE_CHROME_RS.contains("chrome.shell.remove_css_class(\"is-active\")")
            && TITLE_CHROME_RS.contains("chrome.close_button.set_sensitive(close_enabled)")
            && APP_CHROME_RS.contains("let title = TitleChrome::new();")
            && WINDOW_RS.contains("build_title_tab_chrome()")
            && WINDOW_RS.contains("apply_title_tab_state(")
            && WINDOWS_GTK_APP_RS.contains("build_app_header_chrome()")
            && WINDOWS_GTK_APP_RS.contains("build_title_tab_chrome()")
            && WINDOWS_GTK_APP_RS.contains("apply_title_tab_state(")
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
            && WINDOWS_GTK_APP_RS.contains("launch_assets.clone()"),
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
        source_contains(
            WORKSPACE_PREVIEW_RS,
            "if active {\n        shell.add_css_class(\"is-active-tile\");\n    }"
        ) && !source_contains(
            WORKSPACE_PREVIEW_RS,
            "shell.add_css_class(\"is-active-tile\");\n    make_shrinkable"
        ),
        "Windows GTK workspace preview should only mark the active tile with the same header-local active styling as Linux"
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
            && WORKSPACE_PREVIEW_RS.contains("chrome.close_button.connect_clicked")
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
            "summary.append(&name_label);\n    summary.append(&alert_button);\n    summary.append(&broadcast_state);\n    summary.append(&broadcast_selector);\n    summary.append(&broadcast_entry);\n    summary.append(&broadcast_button);\n    summary.append(&add_web_tile_button);\n    summary.append(&url_entry);\n    summary.append(&url_reload_button);\n    summary.append(&runbook_selector);\n    summary.append(&runbook_button);"
        ) && WORKSPACE_PREVIEW_RS.contains("controls_sensitive: true")
            && WORKSPACE_VIEW_RS.contains("controls_sensitive: true"),
        "Windows GTK workspace preview summary should keep the same visible toolbar ordering as Linux GTK workspaces via shared chrome"
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
        "about_dialog, assets_manager, command_palette, companion_dialog, settings_dialog",
        "ShortcutControllerHandle",
        "present_command_palette",
        "command_palette::PaletteAction",
        "Show Templates",
        "Open Settings",
        "Open Assets Manager",
        "About {}",
        "Open Account / Sync",
        "Switch to {label}",
        "open_command_palette_handle",
        "save_command_palette_shortcut(&shortcut)",
        "Command palette shortcut set to {shortcut}",
        "defaults.command_palette_shortcut",
        "install_command_palette_shortcut",
        "command_palette_shortcut_accelerators",
        "<Ctrl><Shift>P",
    ] {
        assert!(
            WINDOWS_GTK_APP_RS.contains(token),
            "Windows GTK should expose the shared Linux command palette affordance: {token}"
        );
    }
}

#[test]
fn windows_gtk_workspace_toolbar_controls_are_wired_to_runtime_state() {
    for token in [
        "pub struct TileRuntimeSurface",
        "command_sender: Option<Rc<dyn Fn(&str) -> bool>>",
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
        "update_active_split_ratio",
        "update_split_ratio(",
        "update_active_web_tile_url",
        "close_active_session_tile",
        "close_tile(&session_ref.tabs[tab_index].preset.layout, tile_id)",
        "connect_preview_tile_close",
        "prune_runtime_surfaces",
        "bind_preview_runbook_controls",
        "present_preview_runbook_dialog",
        "execute_preview_runbook",
        "resolve_runbook(runbook, &variables, &tile_specs)",
        "TemplateVariableValues::new()",
        "bind_preview_alert_controls",
        "mark_all_read_button",
        "alert_store.mark_all_read()",
        "alert_store.subscribe(refresh.clone())",
        "bind_preview_terminal_snippets",
        "refresh_preview_snippet_list",
        "show_preview_snippet_variable_form",
        "execute_preview_snippet",
        "resolve_snippet(snippet, &variables)",
        "active_tab_tile_specs",
        "format!(\"{command}\\n\")",
        "pub(crate) mod context_menu;",
    ] {
        assert!(
            WORKSPACE_PREVIEW_RS.contains(token) || UI_MOD_RS.contains(token),
            "Windows GTK workspace preview should wire shared toolbar/tile controls through runtime/session state: {token}"
        );
    }

    assert!(
        WINDOWS_GTK_RUNTIME_RS.contains("TileRuntimeSurface")
            && WINDOWS_GTK_RUNTIME_RS.contains("install_terminal_output_context_menu")
            && WINDOWS_GTK_RUNTIME_RS.contains("context_menu::popover(output)")
            && WINDOWS_GTK_RUNTIME_RS.contains("context_menu::action_button(\"Copy\"")
            && WINDOWS_GTK_RUNTIME_RS.contains("context_menu::action_button(\"Paste\"")
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
            && WINDOWS_GTK_RUNTIME_RS.contains("build_terminal_recovery_popover")
            && WINDOWS_GTK_RUNTIME_RS.contains("context_menu::action_button(\"Reconnect\"")
            && WINDOWS_GTK_RUNTIME_RS.contains("Reconnect Session")
            && WORKSPACE_PREVIEW_RS.contains("(recovery_binder.bind)")
            && !WINDOWS_GTK_RUNTIME_RS.contains("stdin.write_all(b\"\\r\\n\")")
            && WINDOWS_GTK_RUNTIME_RS.contains("format!(\"{text}\\r\\n\")")
            && WINDOWS_GTK_RUNTIME_RS
                .contains("TileKind::WebView => build_web_runtime_surface(tile)")
            && WINDOWS_GTK_RUNTIME_RS.contains("url_applier: Some(url_applier)")
            && WINDOWS_GTK_RUNTIME_RS.contains("web_settings_applier: Some(web_settings_applier)")
            && WINDOWS_GTK_RUNTIME_RS.contains("CreateCoreWebView2EnvironmentWithOptions")
            && WINDOWS_GTK_RUNTIME_RS.contains("CreateCoreWebView2ControllerCompletedHandler")
            && WINDOWS_GTK_RUNTIME_RS.contains("gdk_win32_surface_get_handle")
            && WINDOWS_GTK_RUNTIME_RS.contains("gtk_widget_root_bounds")
            && WINDOWS_GTK_RUNTIME_RS.contains("controller.SetBounds(bounds)")
            && WINDOWS_GTK_RUNTIME_RS.contains("webview.Navigate(&HSTRING::from")
            && WINDOWS_GTK_RUNTIME_RS.contains("webview.Reload()")
            && WINDOWS_GTK_RUNTIME_RS.contains("build_web_runtime_context_menu")
            && WINDOWS_GTK_RUNTIME_RS
                .contains(r#"context_menu::action_button("Reload", Some("F5"))"#)
            && WINDOWS_GTK_RUNTIME_RS.contains(r#"context_menu::action_button("Copy URL", None)"#)
            && WINDOWS_GTK_RUNTIME_RS.contains("ContextMenuRequestedEventHandler")
            && WINDOWS_GTK_RUNTIME_RS.contains("NewWindowRequestedEventHandler")
            && WINDOWS_GTK_RUNTIME_RS.contains("handle_gtk_webview_new_window_request")
            && WINDOWS_GTK_RUNTIME_RS.contains("remove_NewWindowRequested")
            && WINDOWS_GTK_RUNTIME_RS.contains("remove_ContextMenuRequested")
            && WINDOWS_GTK_RUNTIME_RS.contains("controller.Close()"),
        "Windows GTK terminal/runtime surfaces should expose shared command controls and embed WebView2-backed web panes instead of leaving browser tiles as external placeholders"
    );
}

#[test]
fn windows_packaging_stages_shared_gtk_resources_and_smoke_checks_payload() {
    assert!(
        CI_YML.contains("verify-windows-gtk")
            && CI_YML.contains("setup-windows-gtk.ps1 -InstallWithGvsbuild")
            && CI_YML.contains("windows-gtk-runtime-gvsbuild-v4")
            && CI_YML.contains("save-always: true")
            && CI_YML.contains("cargo check --target x86_64-pc-windows-msvc --features voice-cpal,windows-gtk-shell")
            && CI_YML.contains("build-windows.ps1 -UseGtkShell")
            && CI_YML.contains("windows-smoke-test.ps1 -UseGtkShell"),
        "CI should include native Windows GTK build, package, and smoke coverage"
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
            && WINDOWS_PORTABLE_NSI.contains(r#"ExecWait '"$PLUGINSDIR\TerminalTiler.exe"' $0"#)
            && WINDOWS_PORTABLE_NSI.contains(r#"RMDir /r "$PLUGINSDIR""#)
            && WINDOWS_PORTABLE_NSI.contains("SetErrorLevel $0"),
        "direct portable exe should be a self-extracting launcher for the full staged payload and clean its temp extraction root"
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
            && WINDOWS_SMOKE_PS1
                .contains("Windows GTK shell restored interactive GTK workspace with")
            && WINDOWS_SMOKE_PS1.contains("unexpectedly opened the legacy Win32 workspace host")
            && WINDOWS_SMOKE_PS1.contains("Test-ProcessTreeHasMainWindow")
            && WINDOWS_SMOKE_PS1
                .contains("$mainWindowTimeoutSeconds = if ($expectGtkShell) { 20 } else { 8 }"),
        "Windows smoke test should validate GTK startup/restored-runtime logs even for self-extracting portable launchers"
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
    }

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
            && DOC_WINDOWS_GTK_VISUAL_QA.contains("New/edit wizard")
            && DOC_WINDOWS_GTK_VISUAL_QA
                .contains("Active/restored 3-pane workspace in the shared GTK shell")
            && DOC_WINDOWS_GTK_VISUAL_QA.contains("Dark and light themes")
            && DOC_WINDOWS_GTK_VISUAL_QA
                .contains("Comfortable, standard, and compact density modes")
            && DOC_WINDOWS_GTK_VISUAL_QA.contains(
                "Release artifact parity across `portable-exe`, `portable-zip`, `nsis-install`, and `msi-install`"
            )
            && DOC_WINDOWS_GTK_VISUAL_QA.contains("published self-extracting portable `.exe`"),
        "visual QA documentation should define baseline, capture command, and required comparison screens"
    );

    assert!(
        WINDOWS_CAPTURE_VISUALS_PS1.contains("launch-dashboard")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("restored-workspace")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("System.Drawing")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("PrintWindow")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("default_theme")
            && WINDOWS_CAPTURE_VISUALS_PS1.contains("default_density")
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
            && WINDOWS_CAPTURE_RELEASE_VISUALS_PS1.contains("Expand-Archive")
            && WINDOWS_CAPTURE_RELEASE_VISUALS_PS1.contains("msiexec.exe")
            && WINDOWS_CAPTURE_RELEASE_VISUALS_PS1
                .contains("-OutputDir (Join-Path $OutputDir $Label)")
            && DOC_WINDOWS_GTK_VISUAL_QA.contains("capture-windows-release-gtk-visuals.ps1"),
        "Windows release visual QA should capture every published GTK artifact shape into separate comparable bundles"
    );

    assert!(
        PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("launch-dashboard")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("restored-workspace")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("default_theme")
            && PACKAGE_CAPTURE_LINUX_GTK_VISUALS_SH.contains("default_density")
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
            && PACKAGE_COMPARE_GTK_VISUALS_SH.contains("restored-workspace")
            && PACKAGE_COMPARE_GTK_VISUALS_SH
                .contains("<index>-<scenario>-<theme>-<density>-*.png")
            && PACKAGE_COMPARE_GTK_VISUALS_SH.contains("--density comfortable|standard|compact")
            && PACKAGE_COMPARE_GTK_VISUALS_SH
                .contains("scenario\\tindex\\ttheme\\tdensity\\tstatus\\tnormalized_rmse")
            && PACKAGE_COMPARE_GTK_VISUALS_SH.contains("$index-$scenario-$THEME-$DENSITY-")
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
            && WINDOW_RS.contains("let rename_click = gtk::GestureClick::builder()"),
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
