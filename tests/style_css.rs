const STYLE_CSS: &str = include_str!("../resources/style.css");

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
