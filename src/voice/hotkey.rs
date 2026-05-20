use crate::voice::preferences::VoiceActivationMode;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotkeySpec {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub super_key: bool,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub super_key: bool,
    pub key: String,
    pub pressed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceHotkeyAction {
    Pressed,
    Released,
}

impl HotkeySpec {
    pub fn parse(shortcut: &str) -> Option<Self> {
        let mut remaining = shortcut.trim();
        if remaining.is_empty() {
            return None;
        }
        let mut spec = Self {
            ctrl: false,
            shift: false,
            alt: false,
            super_key: false,
            key: String::new(),
        };
        while let Some(stripped) = remaining.strip_prefix('<') {
            let end = stripped.find('>')?;
            match stripped[..end].to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "primary" => spec.ctrl = true,
                "shift" => spec.shift = true,
                "alt" => spec.alt = true,
                "super" | "meta" | "win" => spec.super_key = true,
                _ => return None,
            }
            remaining = stripped[end + 1..].trim_start();
        }
        if remaining.is_empty() {
            return None;
        }
        spec.key = canonical_key(remaining);
        Some(spec)
    }

    pub fn matches(&self, event: &KeyEvent) -> bool {
        self.ctrl == event.ctrl
            && self.shift == event.shift
            && self.alt == event.alt
            && self.super_key == event.super_key
            && self.key == canonical_key(&event.key)
    }
}

pub fn action_for_event(
    shortcut: &str,
    activation_mode: VoiceActivationMode,
    event: &KeyEvent,
) -> Option<VoiceHotkeyAction> {
    let spec = HotkeySpec::parse(shortcut)?;
    if !spec.matches(event) {
        return None;
    }
    match (activation_mode, event.pressed) {
        (_, true) => Some(VoiceHotkeyAction::Pressed),
        (VoiceActivationMode::PushToTalk, false) => Some(VoiceHotkeyAction::Released),
        (VoiceActivationMode::Toggle, false) => None,
    }
}

fn canonical_key(key: &str) -> String {
    match key.trim().to_ascii_lowercase().as_str() {
        "space" => "space".into(),
        "plus" | "+" => "plus".into(),
        "equal" | "=" => "equal".into(),
        "minus" | "-" => "minus".into(),
        "slash" | "/" => "slash".into(),
        "backslash" | "\\" => "backslash".into(),
        "period" | "." => "period".into(),
        "comma" | "," => "comma".into(),
        "semicolon" | ";" => "semicolon".into(),
        "apostrophe" | "'" => "apostrophe".into(),
        "grave" | "`" => "grave".into(),
        "bracketleft" | "[" => "bracketleft".into(),
        "bracketright" | "]" => "bracketright".into(),
        other => other.to_ascii_uppercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::preferences::VoiceActivationMode;

    fn event(pressed: bool) -> KeyEvent {
        KeyEvent {
            ctrl: true,
            shift: true,
            alt: false,
            super_key: false,
            key: "space".into(),
            pressed,
        }
    }

    #[test]
    fn parses_accelerator_style_hotkeys() {
        let spec = HotkeySpec::parse("<Ctrl><Shift>space").unwrap();
        assert!(spec.ctrl);
        assert!(spec.shift);
        assert_eq!(spec.key, "space");
        assert!(HotkeySpec::parse("<Ctrl><Shift>").is_none());
    }

    #[test]
    fn keeps_x11_case_sensitive_symbol_names_for_punctuation() {
        assert_eq!(
            HotkeySpec::parse("<Control><Alt>slash").unwrap().key,
            "slash"
        );
        assert_eq!(HotkeySpec::parse("<Control><Alt>/").unwrap().key, "slash");
        assert_eq!(
            HotkeySpec::parse("<Control><Alt>period").unwrap().key,
            "period"
        );
    }

    #[test]
    fn push_to_talk_matches_press_and_release() {
        assert_eq!(
            action_for_event(
                "<Ctrl><Shift>space",
                VoiceActivationMode::PushToTalk,
                &event(true)
            ),
            Some(VoiceHotkeyAction::Pressed)
        );
        assert_eq!(
            action_for_event(
                "<Ctrl><Shift>space",
                VoiceActivationMode::PushToTalk,
                &event(false)
            ),
            Some(VoiceHotkeyAction::Released)
        );
    }

    #[test]
    fn toggle_ignores_release() {
        assert_eq!(
            action_for_event(
                "<Ctrl><Shift>space",
                VoiceActivationMode::Toggle,
                &event(false)
            ),
            None
        );
    }
}
