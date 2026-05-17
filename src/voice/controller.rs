use crate::voice::preferences::{VoiceActivationMode, VoicePreferences};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalTargetStatus {
    FocusedTerminal,
    NoTerminalTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceState {
    Disabled,
    Idle,
    Listening,
    Transcribing,
    NoTarget,
    Error(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceControllerOutput {
    StartCapture,
    StopCaptureAndFlush,
    CancelCapture,
    InsertFinalText(String),
    ShowPartialTranscript(String),
    ShowStatus(String),
}

#[derive(Clone, Debug)]
pub struct VoiceController {
    preferences: VoicePreferences,
    state: VoiceState,
    target_status: TerminalTargetStatus,
}

impl VoiceController {
    pub fn new(preferences: VoicePreferences) -> Self {
        let state = if preferences.enabled {
            VoiceState::Idle
        } else {
            VoiceState::Disabled
        };
        Self {
            preferences,
            state,
            target_status: TerminalTargetStatus::NoTerminalTarget,
        }
    }

    pub fn state(&self) -> &VoiceState {
        &self.state
    }

    pub fn preferences(&self) -> &VoicePreferences {
        &self.preferences
    }

    pub fn set_preferences(&mut self, preferences: VoicePreferences) -> Vec<VoiceControllerOutput> {
        self.preferences = preferences;
        if !self.preferences.enabled {
            self.state = VoiceState::Disabled;
            return vec![
                VoiceControllerOutput::CancelCapture,
                VoiceControllerOutput::ShowStatus("Voice disabled".into()),
            ];
        }
        if matches!(self.state, VoiceState::Disabled) {
            self.state = VoiceState::Idle;
        }
        vec![VoiceControllerOutput::ShowStatus("Voice ready".into())]
    }

    pub fn set_target_status(
        &mut self,
        target_status: TerminalTargetStatus,
    ) -> Vec<VoiceControllerOutput> {
        self.target_status = target_status;
        if self.preferences.enabled
            && matches!(target_status, TerminalTargetStatus::NoTerminalTarget)
            && matches!(self.state, VoiceState::Listening | VoiceState::Transcribing)
        {
            self.state = VoiceState::NoTarget;
            return vec![
                VoiceControllerOutput::StopCaptureAndFlush,
                VoiceControllerOutput::ShowStatus("No focused terminal target".into()),
            ];
        }
        Vec::new()
    }

    pub fn hotkey_pressed(&mut self) -> Vec<VoiceControllerOutput> {
        if !self.preferences.enabled {
            self.state = VoiceState::Disabled;
            return vec![VoiceControllerOutput::ShowStatus("Voice disabled".into())];
        }
        if matches!(self.target_status, TerminalTargetStatus::NoTerminalTarget) {
            self.state = VoiceState::NoTarget;
            return vec![VoiceControllerOutput::ShowStatus(
                "No focused terminal target".into(),
            )];
        }

        match self.preferences.activation_mode {
            VoiceActivationMode::PushToTalk => match self.state {
                VoiceState::Idle | VoiceState::NoTarget | VoiceState::Error(_) => {
                    self.state = VoiceState::Listening;
                    vec![
                        VoiceControllerOutput::StartCapture,
                        VoiceControllerOutput::ShowStatus("Listening".into()),
                    ]
                }
                VoiceState::Listening | VoiceState::Transcribing => Vec::new(),
                VoiceState::Disabled => {
                    vec![VoiceControllerOutput::ShowStatus("Voice disabled".into())]
                }
            },
            VoiceActivationMode::Toggle => match self.state {
                VoiceState::Listening | VoiceState::Transcribing => {
                    self.state = VoiceState::Transcribing;
                    vec![
                        VoiceControllerOutput::StopCaptureAndFlush,
                        VoiceControllerOutput::ShowStatus("Transcribing".into()),
                    ]
                }
                _ => {
                    self.state = VoiceState::Listening;
                    vec![
                        VoiceControllerOutput::StartCapture,
                        VoiceControllerOutput::ShowStatus("Listening".into()),
                    ]
                }
            },
        }
    }

    pub fn hotkey_released(&mut self) -> Vec<VoiceControllerOutput> {
        if self.preferences.activation_mode != VoiceActivationMode::PushToTalk {
            return Vec::new();
        }
        if matches!(self.state, VoiceState::Listening) {
            self.state = VoiceState::Transcribing;
            return vec![
                VoiceControllerOutput::StopCaptureAndFlush,
                VoiceControllerOutput::ShowStatus("Transcribing".into()),
            ];
        }
        Vec::new()
    }

    pub fn partial_transcript(&mut self, text: impl Into<String>) -> Vec<VoiceControllerOutput> {
        if matches!(self.state, VoiceState::Listening | VoiceState::Transcribing) {
            return vec![VoiceControllerOutput::ShowPartialTranscript(text.into())];
        }
        Vec::new()
    }

    pub fn final_transcript(&mut self, text: impl Into<String>) -> Vec<VoiceControllerOutput> {
        let text = text.into();
        self.state = if self.preferences.enabled {
            VoiceState::Idle
        } else {
            VoiceState::Disabled
        };
        if text.trim().is_empty() {
            return vec![VoiceControllerOutput::ShowStatus(
                "No speech detected".into(),
            )];
        }
        if matches!(self.target_status, TerminalTargetStatus::FocusedTerminal) {
            vec![
                VoiceControllerOutput::InsertFinalText(text),
                VoiceControllerOutput::ShowStatus("Inserted voice text".into()),
            ]
        } else {
            vec![VoiceControllerOutput::ShowStatus(
                "No focused terminal target".into(),
            )]
        }
    }

    pub fn engine_error(&mut self, message: impl Into<String>) -> Vec<VoiceControllerOutput> {
        let message = message.into();
        self.state = VoiceState::Error(message.clone());
        vec![
            VoiceControllerOutput::CancelCapture,
            VoiceControllerOutput::ShowStatus(format!("Voice engine error: {message}")),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::preferences::{VoiceActivationMode, VoicePreferences};

    fn enabled_preferences(mode: VoiceActivationMode) -> VoicePreferences {
        VoicePreferences {
            enabled: true,
            activation_mode: mode,
            ..VoicePreferences::default()
        }
    }

    #[test]
    fn push_to_talk_starts_on_press_and_flushes_on_release() {
        let mut controller =
            VoiceController::new(enabled_preferences(VoiceActivationMode::PushToTalk));
        controller.set_target_status(TerminalTargetStatus::FocusedTerminal);

        assert_eq!(
            controller.hotkey_pressed()[0],
            VoiceControllerOutput::StartCapture
        );
        assert_eq!(controller.state(), &VoiceState::Listening);
        assert!(controller.hotkey_pressed().is_empty());
        assert_eq!(
            controller.hotkey_released()[0],
            VoiceControllerOutput::StopCaptureAndFlush
        );
        assert_eq!(controller.state(), &VoiceState::Transcribing);
    }

    #[test]
    fn toggle_mode_uses_hotkey_press_for_start_and_stop() {
        let mut controller = VoiceController::new(enabled_preferences(VoiceActivationMode::Toggle));
        controller.set_target_status(TerminalTargetStatus::FocusedTerminal);

        assert_eq!(
            controller.hotkey_pressed()[0],
            VoiceControllerOutput::StartCapture
        );
        assert_eq!(
            controller.hotkey_pressed()[0],
            VoiceControllerOutput::StopCaptureAndFlush
        );
    }

    #[test]
    fn final_transcript_inserts_only_when_terminal_is_focused() {
        let mut controller = VoiceController::new(enabled_preferences(VoiceActivationMode::Toggle));
        controller.set_target_status(TerminalTargetStatus::FocusedTerminal);
        controller.hotkey_pressed();
        assert_eq!(
            controller.final_transcript("cargo test")[0],
            VoiceControllerOutput::InsertFinalText("cargo test".into())
        );

        controller.set_target_status(TerminalTargetStatus::NoTerminalTarget);
        assert!(
            !controller
                .final_transcript("cargo test")
                .iter()
                .any(|output| matches!(output, VoiceControllerOutput::InsertFinalText(_)))
        );
    }

    #[test]
    fn disabled_preferences_do_not_start_capture() {
        let mut controller = VoiceController::new(VoicePreferences::default());
        controller.set_target_status(TerminalTargetStatus::FocusedTerminal);
        assert!(
            !controller
                .hotkey_pressed()
                .iter()
                .any(|output| matches!(output, VoiceControllerOutput::StartCapture))
        );
        assert_eq!(controller.state(), &VoiceState::Disabled);
    }
}
