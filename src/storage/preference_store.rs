use std::fs;
use std::io;
use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::app::logging;
use crate::model::preset::ApplicationDensity;
use crate::storage::fs_utils::{atomic_write_private, preserve_corrupt_file};

const STORE_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub struct PreferenceStore {
    path: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PreferenceDocument {
    version: u32,
    last_density: ApplicationDensity,
}

impl PreferenceStore {
    pub fn new() -> Self {
        let path = ProjectDirs::from("dev", "Zethrus", "TerminalTiler")
            .map(|dirs| dirs.config_dir().join("preferences.toml"));
        Self { path }
    }

    pub fn load_last_density(&self) -> ApplicationDensity {
        let Some(path) = self.path.as_ref() else {
            return ApplicationDensity::Compact;
        };

        let raw = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return ApplicationDensity::Compact;
            }
            Err(error) => {
                logging::error(format!(
                    "failed to read preferences '{}': {}",
                    path.display(),
                    error
                ));
                return ApplicationDensity::Compact;
            }
        };

        match toml::from_str::<PreferenceDocument>(&raw) {
            Ok(document) if document.version == STORE_VERSION => document.last_density,
            Ok(_) => {
                self.recover_invalid_preferences(path, "invalid preferences version");
                ApplicationDensity::Compact
            }
            Err(error) => {
                self.recover_invalid_preferences(path, &format!("corrupt preferences: {error}"));
                ApplicationDensity::Compact
            }
        }
    }

    pub fn save_last_density(&self, density: ApplicationDensity) {
        let Some(path) = self.path.as_ref() else {
            return;
        };

        let document = PreferenceDocument {
            version: STORE_VERSION,
            last_density: density,
        };

        let serialized = match toml::to_string_pretty(&document) {
            Ok(serialized) => serialized,
            Err(error) => {
                logging::error(format!("failed to serialize preferences: {}", error));
                return;
            }
        };

        if let Err(error) = atomic_write_private(path, &serialized) {
            logging::error(format!(
                "failed to write preferences '{}': {}",
                path.display(),
                error
            ));
        }
    }

    fn recover_invalid_preferences(&self, path: &std::path::Path, reason: &str) {
        let message = match preserve_corrupt_file(path) {
            Ok(Some(preserved)) => format!(
                "{reason}; moved invalid preferences aside to '{}'",
                preserved.display()
            ),
            Ok(None) => reason.to_string(),
            Err(error) => format!(
                "{reason}; failed to preserve invalid preferences '{}': {}",
                path.display(),
                error
            ),
        };
        logging::error(message);
    }
}

#[cfg(test)]
impl PreferenceStore {
    fn from_path(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }
}

#[cfg(test)]
mod tests {
    use super::PreferenceStore;
    use crate::model::preset::ApplicationDensity;
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn temp_dir(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("terminaltiler-{prefix}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn returns_compact_when_preferences_are_missing() {
        let dir = temp_dir("pref-missing");
        let store = PreferenceStore::from_path(dir.join("preferences.toml"));

        assert_eq!(store.load_last_density(), ApplicationDensity::Compact);
    }

    #[test]
    fn persists_last_density() {
        let dir = temp_dir("pref-roundtrip");
        let store = PreferenceStore::from_path(dir.join("preferences.toml"));

        store.save_last_density(ApplicationDensity::Comfortable);

        assert_eq!(store.load_last_density(), ApplicationDensity::Comfortable);
    }
}