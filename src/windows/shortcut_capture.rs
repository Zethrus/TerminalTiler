#[cfg(target_os = "windows")]
mod imp {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_ADD, VK_CONTROL, VK_ESCAPE, VK_F1, VK_LWIN, VK_MENU, VK_MULTIPLY, VK_RWIN,
        VK_SHIFT, VK_SUBTRACT,
    };

    const VK_OEM_PLUS: u32 = 0xBB;
    const VK_OEM_MINUS: u32 = 0xBD;
    const VK_F24: u32 = 0x87;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct ShortcutSpec {
        ctrl: bool,
        shift: bool,
        alt: bool,
        super_key: bool,
        virtual_key: u32,
    }

    pub fn matches_keydown(shortcut: &str, virtual_key: u32) -> bool {
        let Some(spec) = parse_shortcut(shortcut) else {
            return false;
        };
        if spec.virtual_key != virtual_key {
            return false;
        }
        modifier_pressed(VK_CONTROL.into()) == spec.ctrl
            && modifier_pressed(VK_SHIFT.into()) == spec.shift
            && modifier_pressed(VK_MENU.into()) == spec.alt
            && super_pressed() == spec.super_key
    }

    pub fn capture_shortcut_from_keydown(virtual_key: u32) -> Option<String> {
        if matches!(virtual_key, key if key == u32::from(VK_CONTROL)
            || key == u32::from(VK_SHIFT)
            || key == u32::from(VK_MENU)
            || key == u32::from(VK_LWIN)
            || key == u32::from(VK_RWIN))
        {
            return None;
        }
        let ctrl = modifier_pressed(VK_CONTROL.into());
        let shift = modifier_pressed(VK_SHIFT.into());
        let alt = modifier_pressed(VK_MENU.into());
        let super_key = super_pressed();
        if virtual_key == u32::from(VK_ESCAPE) && !ctrl && !shift && !alt {
            return Some(String::new());
        }

        let key = render_virtual_key(virtual_key, shift)?;

        let mut rendered = String::new();
        if ctrl {
            rendered.push_str("<Ctrl>");
        }
        if shift {
            rendered.push_str("<Shift>");
        }
        if alt {
            rendered.push_str("<Alt>");
        }
        if super_key {
            rendered.push_str("<Super>");
        }
        rendered.push_str(&key);
        Some(rendered)
    }

    pub fn display_label(shortcut: &str) -> String {
        let trimmed = shortcut.trim();
        if trimmed.is_empty() {
            return "Not set".into();
        }

        let mut remaining = trimmed;
        let mut parts = Vec::new();
        while let Some(stripped) = remaining.strip_prefix('<') {
            let Some(end) = stripped.find('>') else {
                break;
            };
            let token = &stripped[..end];
            let token_lower = token.to_ascii_lowercase();
            let label = match token_lower.as_str() {
                "ctrl" | "control" | "primary" => "Ctrl",
                "shift" => "Shift",
                "alt" => "Alt",
                "super" => "Super",
                _ => token,
            };
            parts.push(label.to_string());
            remaining = stripped[end + 1..].trim_start();
        }
        let key = match remaining {
            "plus" => "+",
            "equal" => "=",
            "minus" => "-",
            "KP_Add" => "Num +",
            "KP_Subtract" => "Num -",
            "KP_Multiply" => "Num *",
            other => other,
        };
        parts.push(key.to_string());
        parts.join("+")
    }

    fn parse_shortcut(shortcut: &str) -> Option<ShortcutSpec> {
        let mut remaining = shortcut.trim();
        if remaining.is_empty() {
            return None;
        }
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut super_key = false;

        while let Some(stripped) = remaining.strip_prefix('<') {
            let end = stripped.find('>')?;
            let token = &stripped[..end];
            match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "primary" => ctrl = true,
                "shift" => shift = true,
                "alt" => alt = true,
                "super" => super_key = true,
                _ => return None,
            }
            remaining = stripped[end + 1..].trim_start();
        }

        let virtual_key = parse_virtual_key(remaining)?;

        Some(ShortcutSpec {
            ctrl,
            shift,
            alt,
            super_key,
            virtual_key,
        })
    }

    fn modifier_pressed(virtual_key: i32) -> bool {
        let state = unsafe { GetKeyState(virtual_key) };
        state < 0
    }

    fn super_pressed() -> bool {
        modifier_pressed(VK_LWIN.into()) || modifier_pressed(VK_RWIN.into())
    }

    fn parse_virtual_key(token: &str) -> Option<u32> {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.len() == 1 {
            let character = trimmed.chars().next()?.to_ascii_uppercase();
            if character.is_ascii_uppercase() || character.is_ascii_digit() {
                return Some(character as u32);
            }
        }

        let lower = trimmed.to_ascii_lowercase();
        if let Some(function) = lower.strip_prefix('f')
            && let Ok(number) = function.parse::<u32>()
            && (1..=24).contains(&number)
        {
            return Some(u32::from(VK_F1) + number - 1);
        }

        match lower.as_str() {
            "plus" | "equal" => Some(VK_OEM_PLUS),
            "minus" => Some(VK_OEM_MINUS),
            "kp_add" => Some(u32::from(VK_ADD)),
            "kp_subtract" => Some(u32::from(VK_SUBTRACT)),
            "kp_multiply" => Some(u32::from(VK_MULTIPLY)),
            _ => None,
        }
    }

    fn render_virtual_key(virtual_key: u32, shift: bool) -> Option<String> {
        match virtual_key {
            key if key >= u32::from(VK_F1) && key <= VK_F24 => {
                Some(format!("F{}", key - u32::from(VK_F1) + 1))
            }
            0x30..=0x39 | 0x41..=0x5A => Some(char::from_u32(virtual_key)?.to_string()),
            VK_OEM_PLUS => Some(if shift { "plus" } else { "equal" }.into()),
            VK_OEM_MINUS => Some("minus".into()),
            key if key == u32::from(VK_ADD) => Some("KP_Add".into()),
            key if key == u32::from(VK_SUBTRACT) => Some("KP_Subtract".into()),
            key if key == u32::from(VK_MULTIPLY) => Some("KP_Multiply".into()),
            _ => None,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::parse_shortcut;

        #[test]
        fn parses_expected_shortcuts() {
            assert!(parse_shortcut("F11").is_some());
            assert!(parse_shortcut("<Ctrl><Shift>D").is_some());
            assert!(parse_shortcut("<Ctrl>equal").is_some());
            assert!(parse_shortcut("<Ctrl>plus").is_some());
            assert!(parse_shortcut("<Ctrl>KP_Add").is_some());
            assert!(parse_shortcut("<Ctrl><Alt>KP_Multiply").is_some());
            assert!(parse_shortcut("<Alt><Super>D").is_some());
            assert!(parse_shortcut("<Ctrl>P").is_some());
        }

        #[test]
        fn rejects_unknown_shortcuts() {
            assert!(parse_shortcut("Tab").is_none());
            assert!(parse_shortcut("<Ctrl><Shift>").is_none());
        }
    }
}

#[cfg(target_os = "windows")]
pub use imp::{capture_shortcut_from_keydown, display_label, matches_keydown};
