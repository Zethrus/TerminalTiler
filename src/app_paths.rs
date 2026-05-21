use std::path::PathBuf;

use directories::ProjectDirs;

pub const PROFILE_ROOT_ENV: &str = "TERMINALTILER_PROFILE_ROOT";

fn profile_root_override() -> Option<PathBuf> {
    std::env::var_os(PROFILE_ROOT_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("dev", "Zethrus", "TerminalTiler")
}

pub fn config_dir() -> Option<PathBuf> {
    profile_root_override()
        .map(|root| root.join("config"))
        .or_else(|| project_dirs().map(|dirs| dirs.config_dir().to_path_buf()))
}

pub fn data_dir() -> Option<PathBuf> {
    profile_root_override()
        .map(|root| root.join("data"))
        .or_else(|| project_dirs().map(|dirs| dirs.data_dir().to_path_buf()))
}

pub fn data_local_dir() -> Option<PathBuf> {
    profile_root_override()
        .map(|root| root.join("local-data"))
        .or_else(|| project_dirs().map(|dirs| dirs.data_local_dir().to_path_buf()))
}

pub fn state_dir() -> Option<PathBuf> {
    profile_root_override()
        .map(|root| root.join("state"))
        .or_else(|| project_dirs().and_then(|dirs| dirs.state_dir().map(PathBuf::from)))
}

pub fn log_dir() -> Option<PathBuf> {
    state_dir().map(|state_dir| state_dir.join("logs"))
}
