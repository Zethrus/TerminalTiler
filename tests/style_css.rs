const STYLE_CSS: &str = include_str!("../resources/style.css");
const ABOUT_DIALOG_RS: &str = include_str!("../src/ui/about_dialog.rs");
const ASSETS_MANAGER_RS: &str = include_str!("../src/ui/assets_manager.rs");
const COMMAND_PALETTE_RS: &str = include_str!("../src/ui/command_palette.rs");
const CONTEXT_MENU_RS: &str = include_str!("../src/ui/context_menu.rs");
const DESIGN_MD: &str = include_str!("../DESIGN.md");
const ICONS_RS: &str = include_str!("../src/ui/icons.rs");
const LAYOUT_TREE_RS: &str = include_str!("../src/ui/layout_tree.rs");
const LAUNCH_SCREEN_RS: &str = include_str!("../src/ui/launch_screen.rs");
const PACKAGE_APPIMAGE_SH: &str = include_str!("../packaging/build-appimage.sh");
const PACKAGE_DEB_SH: &str = include_str!("../packaging/build-deb.sh");
const SETTINGS_DIALOG_RS: &str = include_str!("../src/ui/settings_dialog.rs");
const TERMINAL_SESSION_RS: &str = include_str!("../src/terminal/session.rs");
const TILE_VIEW_RS: &str = include_str!("../src/ui/tile_view.rs");
const WEB_TILE_RS: &str = include_str!("../src/ui/web_tile.rs");
const WINDOW_RS: &str = include_str!("../src/ui/window.rs");
const WINDOWS_APP_RS: &str = include_str!("../src/windows/app.rs");
const WORKSPACE_VIEW_RS: &str = include_str!("../src/ui/workspace_view.rs");

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
            WINDOW_RS,
            "icon_name::SETTINGS,\n        \"Open application settings\"",
        ) && source_contains(
            WINDOW_RS,
            "icon_name::ASSETS,\n        \"Open assets manager\"",
        ),
        "main app header icon-only actions should explain their purpose on hover"
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
    for source in [TILE_VIEW_RS, WEB_TILE_RS, LAYOUT_TREE_RS] {
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
                || source.contains("build_header_icon_button(icon_name::"),
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
        "color: rgba(255, 255, 255, 0.36)",
        "dark disabled buttons should remain deliberately muted and legible",
    );
    assert_css_block_contains(
        "button.primary-cta-button:focus",
        "outline-color: rgba(240, 179, 75, 0.58)",
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
        "0 10px 20px rgba(0, 0, 0, 0.26)",
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
        "outline-color: rgba(240, 179, 75, 0.58)",
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
            && WINDOW_RS.matches("window_shell.append(&header)").count() >= 2
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
    for (source_name, source) in [("terminal tile", TILE_VIEW_RS), ("web tile", WEB_TILE_RS)] {
        assert!(
            source.contains("fn configure_dynamic_header_label"),
            "{source_name} should centralize dynamic header label constraints"
        );
        assert!(
            source.contains("set_ellipsize(ellipsize)")
                && source.contains("set_max_width_chars(max_width_chars)")
                && source.contains("set_single_line_mode(true)")
                && source.contains("set_tooltip_text(Some(full_text))"),
            "{source_name} dynamic header labels should ellipsize, cap width, stay single-line, and keep full values in tooltips"
        );
        for label in ["&title", "&status", "&badge"] {
            assert!(
                source_contains(
                    source,
                    &format!("configure_dynamic_header_label(\n        {label},"),
                ),
                "{source_name} should constrain dynamic header label {label}"
            );
        }
        assert!(
            source.contains("set_tooltip_text(Some(&new_title))"),
            "{source_name} should preserve updated title text in tooltips"
        );
    }

    assert!(
        TILE_VIEW_RS.contains("&pane_group_label")
            && TILE_VIEW_RS.contains("HEADER_GROUP_MAX_CHARS")
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
