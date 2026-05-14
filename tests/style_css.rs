const STYLE_CSS: &str = include_str!("../resources/style.css");
const LAYOUT_TREE_RS: &str = include_str!("../src/ui/layout_tree.rs");
const TERMINAL_SESSION_RS: &str = include_str!("../src/terminal/session.rs");
const TILE_VIEW_RS: &str = include_str!("../src/ui/tile_view.rs");
const WEB_TILE_RS: &str = include_str!("../src/ui/web_tile.rs");

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
