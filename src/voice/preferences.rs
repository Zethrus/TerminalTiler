use serde::{Deserialize, Serialize};

pub const DEFAULT_VOICE_HOTKEY: &str = "<Ctrl><Shift>space";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VoiceActivationMode {
    #[default]
    PushToTalk,
    Toggle,
}

impl VoiceActivationMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::PushToTalk => "Push to Talk",
            Self::Toggle => "Toggle",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VoiceEngineMode {
    #[default]
    Auto,
    Cuda,
    Cpu,
}

impl VoiceEngineMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Cuda => "CUDA",
            Self::Cpu => "CPU",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "state")]
pub enum VoicePackStatus {
    #[default]
    NotInstalled,
    Downloading {
        percent: u8,
    },
    Installed {
        version: String,
    },
    Error {
        message: String,
    },
}

impl VoicePackStatus {
    pub fn summary(&self) -> String {
        match self {
            Self::NotInstalled => "Voice pack not installed".into(),
            Self::Downloading { percent } => format!("Downloading voice pack ({percent}%)"),
            Self::Installed { version } => format!("Voice pack installed ({version})"),
            Self::Error { message } => format!("Voice pack error: {message}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoicePreferences {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub microphone_id: Option<String>,
    #[serde(default)]
    pub activation_mode: VoiceActivationMode,
    #[serde(default = "default_voice_hotkey")]
    pub hotkey: String,
    #[serde(default)]
    pub prefer_global_hotkey: bool,
    #[serde(default)]
    pub pack_status: VoicePackStatus,
    #[serde(default)]
    pub engine_mode: VoiceEngineMode,
}

impl Default for VoicePreferences {
    fn default() -> Self {
        Self {
            enabled: false,
            microphone_id: None,
            activation_mode: VoiceActivationMode::PushToTalk,
            hotkey: default_voice_hotkey(),
            prefer_global_hotkey: false,
            pack_status: VoicePackStatus::NotInstalled,
            engine_mode: VoiceEngineMode::Auto,
        }
    }
}

pub fn default_voice_hotkey() -> String {
    DEFAULT_VOICE_HOTKEY.into()
}
