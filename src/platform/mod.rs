mod common;
#[cfg(target_os = "windows")]
mod wsl_paths;

#[cfg(test)]
pub use common::canonicalize_existing_dir;
pub use common::{configure_webkit_process_environment, home_dir, resolve_workspace_root};
#[cfg(target_os = "windows")]
#[allow(unused_imports)]
pub use wsl_paths::{
    WslUncPath, looks_like_wsl_absolute_path, parse_wsl_unc_path, translate_path_for_wsl,
};
