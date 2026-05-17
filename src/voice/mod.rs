//! Local voice-to-text foundation.
//!
//! The production ASR runtime is delivered as a settings-managed voice pack. The
//! Rust side owns preferences, hotkey/controller state, audio-device discovery
//! boundaries, and a framed helper-process protocol that can be exercised with a
//! fake engine in tests before a real downloadable model pack is present.

pub mod audio;
pub mod controller;
pub mod engine;
pub mod hotkey;
#[cfg(target_os = "linux")]
pub mod linux_global_hotkey;
pub mod pack;
pub mod preferences;
pub mod transcriber;

pub use controller::{TerminalTargetStatus, VoiceController, VoiceControllerOutput, VoiceState};
pub use preferences::{VoiceActivationMode, VoiceEngineMode, VoicePackStatus, VoicePreferences};
pub use transcriber::{ParakeetTranscriber, ParakeetTranscriberError};
