mod common;
mod process;
#[cfg(any(target_os = "windows", test))]
mod wsl_paths;

pub use process::terminal_agent_candidates;
#[cfg(any(target_os = "linux", test))]
pub use process::{ForegroundProcess, terminal_foreground_process};

#[cfg(test)]
pub use common::canonicalize_existing_dir;
#[cfg(target_os = "linux")]
pub use common::configure_webkit_process_environment;
pub use common::{home_dir, resolve_workspace_root};
#[cfg(any(target_os = "windows", test))]
#[allow(unused_imports)]
pub use wsl_paths::{
    WslPathTranslationError, WslUncPath, looks_like_wsl_absolute_path, parse_wsl_unc_path,
    translate_path_for_wsl,
};
