use std::fs;
use std::io;
use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::app::logging;
use crate::model::preset::{ApplicationDensity, ThemeMode};
use crate::storage::fs_utils::{atomic_write_private, preserve_corrupt_file};

const STORE_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppPreferences {
    pub default_density: ApplicationDensity,
    pub default_theme: ThemeMode,
}

#[derive(Clone, Debug)]
pub struct PreferenceStore {
    path: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PreferenceDocument {
    version: u32,
    #[serde(default = "default_density", alias = "last_density")]
    default_density: ApplicationDensity,
    #[serde(default = "default_theme")]
    default_theme: ThemeMode,
}

fn default_density() -> ApplicationDensity {
    ApplicationDensity::Compact
}

fn default_theme() -> ThemeMode {
    ThemeMode::System
}

impl PreferenceStore {
    pub fn new() -> Self {
        let path = ProjectDirs::from("dev", "Zethrus", "TerminalTiler")
            .map(|dirs| dirs.config_dir().join("preferences.toml"));
        Self { path }
    }

    pub fn load(&self) -> AppPreferences {
        let Some(path) = self.path.as_ref() else {
            return AppPreferences::default();
        };

        let raw = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return AppPreferences::default();
            }
            Err(error) => {
                logging::error(format!(
                    "failed to read preferences '{}': {}",
                    path.display(),
                    error
                ));
                return AppPreferences::default();
            }
        };

        match toml::from_str::<PreferenceDocument>(&raw) {
            Ok(document) if document.version == STORE_VERSION => AppPreferences {
                default_density: document.default_density,
                default_theme: document.default_theme,
            },
            Ok(_) => {
                self.recover_invalid_preferences(path, "invalid preferences version");
                AppPreferences::default()
            }
            Err(error) => {
                self.recover_invalid_preferences(path, &format!("corrupt preferences: {error}"));
                AppPreferences::default()
            }
        }
    }

    pub fn save_default_density(&self, density: ApplicationDensity) {
        let mut preferences = self.load();
        preferences.default_density = density;
        self.save(&preferences);
    }

    pub fn save_default_theme(&self, theme: ThemeMode) {
        let mut preferences = self.load();
        preferences.default_theme = theme;
        self.save(&preferences);
    }

    pub fn save(&self, preferences: &AppPreferences) {
        let Some(path) = self.path.as_ref() else {
            return;
        };

        let document = PreferenceDocument {
            version: STORE_VERSION,
            default_density: preferences.default_density,
            default_theme: preferences.default_theme,
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

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            default_density: default_density(),
            default_theme: default_theme(),
        }
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
    use super::{AppPreferences, PreferenceStore};
    use crate::model::preset::{ApplicationDensity, ThemeMode};
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

        assert_eq!(store.load().default_density, ApplicationDensity::Compact);
        assert_eq!(store.load().default_theme, ThemeMode::System);
    }

    #[test]
    fn persists_default_preferences() {
        let dir = temp_dir("pref-roundtrip");
        let store = PreferenceStore::from_path(dir.join("preferences.toml"));

        store.save(&AppPreferences {
            default_density: ApplicationDensity::Comfortable,
            default_theme: ThemeMode::Dark,
        });

        assert_eq!(
            store.load(),
            AppPreferences {
                default_density: ApplicationDensity::Comfortable,
                default_theme: ThemeMode::Dark,
            }
        );
    }

    #[test]
    fn loads_legacy_last_density_field() {
        let dir = temp_dir("pref-legacy");
        let path = dir.join("preferences.toml");
        fs::write(
            &path,
            "version = 1\nlast_density = \"comfortable\"\n",
        )
        .unwrap();

        let store = PreferenceStore::from_path(path);

        assert_eq!(
            store.load(),
            AppPreferences {
                default_density: ApplicationDensity::Comfortable,
                default_theme: ThemeMode::System,
            }
        );
    }
}