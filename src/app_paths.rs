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

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn webview2_user_data_dir() -> Option<PathBuf> {
    data_local_dir().map(|dir| dir.join("webview2"))
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
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};

    use super::{PROFILE_ROOT_ENV, data_local_dir, webview2_user_data_dir};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_profile_root<T>(root: &Path, test: impl FnOnce() -> T) -> T {
        let _guard = env_lock().lock().unwrap();
        let previous = std::env::var_os(PROFILE_ROOT_ENV);
        unsafe {
            std::env::set_var(PROFILE_ROOT_ENV, root);
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
    fn webview2_user_data_dir_uses_local_data_dir() {
        without_profile_root(|| {
            let local_data = data_local_dir().expect("should resolve local data directory");
            assert_eq!(webview2_user_data_dir(), Some(local_data.join("webview2")));
        });
    }

    #[test]
    fn webview2_user_data_dir_respects_profile_root_override() {
        let root = std::env::temp_dir().join("terminaltiler-app-paths-webview2-profile");
        with_profile_root(&root, || {
            assert_eq!(
                webview2_user_data_dir(),
                Some(root.join("local-data").join("webview2"))
            );
        });
    }

    #[cfg(target_os = "windows")]
    mod windows {
        use std::path::Path;

        use super::super::{log_dir, state_dir};
        use super::without_profile_root;

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
