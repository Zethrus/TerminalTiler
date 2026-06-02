use gtk::glib;
use gtk::pango;
use gtk::prelude::*;

#[derive(Clone)]
pub struct VoiceHud {
    revealer: gtk::Revealer,
    status_label: gtk::Label,
    transcript_label: gtk::Label,
}

impl VoiceHud {
    pub fn new() -> Self {
        let revealer = gtk::Revealer::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Start)
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .reveal_child(false)
            .can_target(false)
            .build();
        let shell = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .margin_top(18)
            .css_classes(["voice-hud"])
            .build();
        let status_label = gtk::Label::builder()
            .label("Voice ready")
            .halign(gtk::Align::Start)
            .css_classes(["voice-hud-status"])
            .build();
        let transcript_label = gtk::Label::builder()
            .label("")
            .halign(gtk::Align::Start)
            .ellipsize(pango::EllipsizeMode::End)
            .max_width_chars(72)
            .css_classes(["voice-hud-transcript"])
            .build();
        shell.append(&status_label);
        shell.append(&transcript_label);
        revealer.set_child(Some(&shell));
        Self {
            revealer,
            status_label,
            transcript_label,
        }
    }

    pub fn widget(&self) -> gtk::Widget {
        self.revealer.clone().upcast()
    }

    pub fn show(&self, status: &str, transcript: Option<&str>) {
        self.status_label.set_label(status);
        self.transcript_label.set_label(transcript.unwrap_or(""));
        self.transcript_label
            .set_visible(transcript.is_some_and(|value| !value.trim().is_empty()));
        self.revealer.set_reveal_child(true);
    }

    pub fn hide_later(&self) {
        let revealer = self.revealer.clone();
        glib::timeout_add_seconds_local_once(3, move || {
            revealer.set_reveal_child(false);
        });
    }
}
