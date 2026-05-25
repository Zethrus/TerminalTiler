use std::fs::OpenOptions;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

const VOICE_ENGINE_LOG_FILE: &str = "voice-engine.log";

pub(crate) fn apply_background_spawn(command: &mut Command) -> &mut Command {
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

pub(crate) fn voice_pack_log_path(file_name: &str) -> Option<PathBuf> {
    crate::logging::ensure_log_directory()
        .ok()
        .map(|dir| dir.join(file_name))
}

pub(crate) fn append_log_stdio(file_name: &str) -> Stdio {
    voice_pack_log_path(file_name)
        .and_then(|path| {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            OpenOptions::new().create(true).append(true).open(path).ok()
        })
        .map(Stdio::from)
        .unwrap_or_else(Stdio::null)
}

pub(crate) fn voice_engine_stderr() -> Stdio {
    append_log_stdio(VOICE_ENGINE_LOG_FILE)
}
