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
        .or_else(platform_state_dir)
}

#[cfg(target_os = "windows")]
fn platform_state_dir() -> Option<PathBuf> {
    project_dirs().and_then(|dirs| {
        dirs.state_dir().map(PathBuf::from).or_else(|| {
            dirs.data_local_dir()
                .parent()
                .map(|parent| parent.join("state"))
        })
    })
}

#[cfg(not(target_os = "windows"))]
fn platform_state_dir() -> Option<PathBuf> {
    project_dirs().and_then(|dirs| dirs.state_dir().map(PathBuf::from))
}

pub fn log_dir() -> Option<PathBuf> {
    state_dir().map(|state_dir| state_dir.join("logs"))
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    mod windows {
        use std::path::Path;
        use std::sync::{Mutex, OnceLock};

        use super::super::{PROFILE_ROOT_ENV, log_dir, state_dir};

        fn env_lock() -> &'static Mutex<()> {
            static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
            LOCK.get_or_init(|| Mutex::new(()))
        }

        fn without_profile_root<T>(test: impl FnOnce() -> T) -> T {
            let _guard = env_lock().lock().unwrap();
            let previous = std::env::var_os(PROFILE_ROOT_ENV);
            unsafe {
                std::env::remove_var(PROFILE_ROOT_ENV);
            }
            let result = test();
            unsafe {
                match previous {
                    Some(value) => std::env::set_var(PROFILE_ROOT_ENV, value),
                    None => std::env::remove_var(PROFILE_ROOT_ENV),
                }
            }
            result
        }

        #[test]
        fn windows_state_dir_uses_local_project_state_when_not_profile_overridden() {
            without_profile_root(|| {
                let state = state_dir().expect("Windows should resolve a local state directory");
                assert!(
                    state.ends_with(Path::new("Zethrus").join("TerminalTiler").join("state")),
                    "unexpected Windows state dir: {}",
                    state.display()
                );

                let logs = log_dir().expect("Windows should resolve a logs directory");
                assert_eq!(logs, state.join("logs"));
            });
        }
    }
}
