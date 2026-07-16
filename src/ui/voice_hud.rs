use gtk::glib;
use gtk::pango;
use gtk::prelude::*;

use super::voice_orb::{LevelSource, VoiceOrb};

/// Semantic state reflected by the status dot and orb activity.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum VoiceHudTone {
    #[default]
    Idle,
    Listening,
    Success,
    Error,
}

impl VoiceHudTone {
    fn css_class(self) -> Option<&'static str> {
        match self {
            Self::Idle => None,
            Self::Listening => Some("is-listening"),
            Self::Success => Some("is-success"),
            Self::Error => Some("is-error"),
        }
    }
}

const TONE_CLASSES: [&str; 3] = ["is-listening", "is-success", "is-error"];

#[derive(Clone)]
pub struct VoiceHud {
    revealer: gtk::Revealer,
    orb: VoiceOrb,
    status_dot: gtk::Label,
    status_label: gtk::Label,
    user_row: gtk::Box,
    user_label: gtk::Label,
    assistant_row: gtk::Box,
    assistant_label: gtk::Label,
    activity_row: gtk::Box,
    activity_label: gtk::Label,
    controls: gtk::Box,
    mic_button: gtk::Button,
    end_button: gtk::Button,
}

impl VoiceHud {
    pub fn new() -> Self {
        let revealer = gtk::Revealer::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .reveal_child(false)
            .can_target(true)
            .build();
        let shell = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_bottom(56)
            .css_classes(["voice-hud"])
            .build();

        let orb = VoiceOrb::new();
        let orb_widget = orb.widget();
        orb_widget.set_tooltip_text(Some("TerminalTiler workspace orchestrator"));
        orb_widget.add_css_class("voice-hud-orb");
        shell.append(&orb_widget);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(5)
            .hexpand(true)
            .build();
        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(7)
            .build();
        let status_dot = gtk::Label::builder()
            .label("●")
            .css_classes(["voice-hud-status-dot"])
            .build();
        let status_label = gtk::Label::builder()
            .label("Voice ready")
            .halign(gtk::Align::Start)
            .css_classes(["voice-hud-status"])
            .build();
        header.append(&status_dot);
        header.append(&status_label);
        for text in ["Workspace live", "Ctrl+`"] {
            header.append(
                &gtk::Label::builder()
                    .label(text)
                    .css_classes(["voice-hud-chip"])
                    .build(),
            );
        }
        content.append(&header);

        let (user_row, user_label) = transcript_row("YOU", "voice-hud-user");
        let (assistant_row, assistant_label) = transcript_row("BRIDGE", "voice-hud-assistant");
        let (activity_row, activity_label) = transcript_row("LIVE", "voice-hud-activity");
        content.append(&user_row);
        content.append(&assistant_row);
        content.append(&activity_row);
        shell.append(&content);

        let controls = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .valign(gtk::Align::Center)
            .visible(false)
            .build();
        let mic_button = gtk::Button::builder()
            .icon_name("audio-input-microphone-symbolic")
            .tooltip_text("Start or stop voice capture")
            .css_classes(["voice-hud-button", "voice-hud-mic"])
            .build();
        let end_button = gtk::Button::builder()
            .icon_name("call-stop-symbolic")
            .tooltip_text("End voice orchestration session")
            .css_classes(["voice-hud-button", "voice-hud-end"])
            .build();
        controls.append(&mic_button);
        controls.append(&end_button);
        shell.append(&controls);
        revealer.set_child(Some(&shell));

        Self {
            revealer,
            orb,
            status_dot,
            status_label,
            user_row,
            user_label,
            assistant_row,
            assistant_label,
            activity_row,
            activity_label,
            controls,
            mic_button,
            end_button,
        }
    }

    pub fn widget(&self) -> gtk::Widget {
        self.revealer.clone().upcast()
    }

    /// Reflects semantic state on the status dot and orb. `Listening` marks
    /// the orb active so its fog reacts to the live level source.
    pub fn set_tone(&self, tone: VoiceHudTone) {
        for class in TONE_CLASSES {
            self.status_dot.remove_css_class(class);
        }
        if let Some(class) = tone.css_class() {
            self.status_dot.add_css_class(class);
        }
        self.orb.set_active(tone == VoiceHudTone::Listening);
    }

    /// Installs (or clears) the mic loudness supplier polled by the orb.
    pub fn set_level_source(&self, source: Option<LevelSource>) {
        self.orb.set_level_source(source);
    }

    /// Compatibility surface for local dictation. Companion voice should use
    /// the role-specific methods below so user, assistant, and activity text
    /// remain visually distinct.
    pub fn show(&self, status: &str, transcript: Option<&str>) {
        self.set_status(status);
        set_row_text(
            &self.activity_row,
            &self.activity_label,
            compatibility_transcript_text(transcript),
        );
        self.reveal();
    }

    pub fn show_user(&self, status: &str, transcript: &str) {
        self.set_status(status);
        set_row_text(&self.user_row, &self.user_label, transcript);
        self.reveal();
    }

    pub fn show_assistant(&self, status: &str, transcript: &str) {
        self.set_status(status);
        set_row_text(&self.assistant_row, &self.assistant_label, transcript);
        self.reveal();
    }

    pub fn show_activity(&self, status: &str, activity: &str) {
        self.set_status(status);
        set_row_text(&self.activity_row, &self.activity_label, activity);
        self.reveal();
    }

    pub fn set_status(&self, status: &str) {
        self.status_label.set_label(status);
        self.reveal();
    }

    fn reveal(&self) {
        self.orb.start_animation();
        self.revealer.set_reveal_child(true);
    }

    pub fn set_controls_visible(&self, visible: bool) {
        self.controls.set_visible(visible);
    }

    pub fn set_mic_active(&self, active: bool) {
        if active {
            self.mic_button.add_css_class("active");
            self.mic_button
                .set_icon_name("microphone-sensitivity-high-symbolic");
            self.mic_button
                .set_tooltip_text(Some("Commit voice request"));
        } else {
            self.mic_button.remove_css_class("active");
            self.mic_button
                .set_icon_name("audio-input-microphone-symbolic");
            self.mic_button
                .set_tooltip_text(Some("Start voice capture"));
        }
    }

    pub fn connect_mic_clicked<F: Fn() + 'static>(&self, callback: F) {
        self.mic_button.connect_clicked(move |_| callback());
    }

    pub fn connect_end_clicked<F: Fn() + 'static>(&self, callback: F) {
        self.end_button.connect_clicked(move |_| callback());
    }

    pub fn hide(&self) {
        self.orb.stop_animation();
        self.revealer.set_reveal_child(false);
    }

    pub fn hide_later(&self) {
        let hud = self.clone();
        glib::timeout_add_seconds_local_once(3, move || {
            hud.hide();
        });
    }
}

fn transcript_row(prefix: &str, class: &str) -> (gtk::Box, gtk::Label) {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(7)
        .visible(false)
        .build();
    row.append(
        &gtk::Label::builder()
            .label(prefix)
            .valign(gtk::Align::Start)
            .css_classes(["voice-hud-prefix"])
            .build(),
    );
    let label = gtk::Label::builder()
        .label("")
        .halign(gtk::Align::Start)
        .hexpand(true)
        .ellipsize(pango::EllipsizeMode::End)
        .max_width_chars(76)
        .selectable(true)
        .css_classes(["voice-hud-transcript", class])
        .build();
    row.append(&label);
    (row, label)
}

fn set_row_text(row: &gtk::Box, label: &gtk::Label, text: &str) {
    let text = text.trim();
    label.set_label(text);
    label.set_tooltip_text((!text.is_empty()).then_some(text));
    row.set_visible(!text.is_empty());
}

fn compatibility_transcript_text(transcript: Option<&str>) -> &str {
    transcript
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::compatibility_transcript_text;

    #[test]
    fn status_only_compatibility_updates_clear_the_activity_transcript() {
        assert_eq!(compatibility_transcript_text(None), "");
        assert_eq!(compatibility_transcript_text(Some("")), "");
        assert_eq!(compatibility_transcript_text(Some("   \n\t")), "");
        assert_eq!(
            compatibility_transcript_text(Some("  captured speech  ")),
            "captured speech"
        );
    }
}
