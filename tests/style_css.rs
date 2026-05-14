const STYLE_CSS: &str = include_str!("../resources/style.css");
const ABOUT_DIALOG_RS: &str = include_str!("../src/ui/about_dialog.rs");
const ASSETS_MANAGER_RS: &str = include_str!("../src/ui/assets_manager.rs");
const COMMAND_PALETTE_RS: &str = include_str!("../src/ui/command_palette.rs");
const CONTEXT_MENU_RS: &str = include_str!("../src/ui/context_menu.rs");
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
const WORKSPACE_VIEW_RS: &str = include_str!("../src/ui/workspace_view.rs");

const TERMINAL_CARD_STATES: &[&str] = &[
    ".terminal-card.is-active-tile",
    ".terminal-card.is-disconnected",
    ".terminal-card.is-drop-target",
];

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
            && WINDOW_RS.contains("translate_coordinates(&self.tabs_box")
            && WINDOW_RS.contains("drop_surface.add_controller(drop_target)")
            && WINDOW_RS.contains("context_menu::action_button(\"Detach\", None)")
            && WINDOW_RS.contains("let rename_click = gtk::GestureClick::builder()"),
        "workspace tab drag should be left-button-only, update over the full title chrome, and preserve Detach/Rename handlers"
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
                source.contains(&format!(
                    "configure_dynamic_header_label(\n        {label},"
                )),
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
