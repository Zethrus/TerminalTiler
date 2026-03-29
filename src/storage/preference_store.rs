use std::fs;
use std::io;
use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::app::logging;
use crate::model::preset::{ApplicationDensity, ThemeMode};
use crate::storage::fs_utils::{atomic_write_private, preserve_corrupt_file};

const STORE_VERSION: u32 = 1;
const DEFAULT_WORKSPACE_FULLSCREEN_SHORTCUT: &str = "F11";
const DEFAULT_WORKSPACE_DENSITY_SHORTCUT: &str = "<Ctrl><Shift>D";
const DEFAULT_WORKSPACE_ZOOM_IN_SHORTCUT: &str = "<Ctrl>plus";
const DEFAULT_WORKSPACE_ZOOM_OUT_SHORTCUT: &str = "<Ctrl>minus";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPreferences {
    pub default_density: ApplicationDensity,
    pub default_theme: ThemeMode,
    pub workspace_fullscreen_shortcut: String,
    pub workspace_density_shortcut: String,
    pub workspace_zoom_in_shortcut: String,
    pub workspace_zoom_out_shortcut: String,
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
    #[serde(default = "default_fullscreen_shortcut")]
    workspace_fullscreen_shortcut: String,
    #[serde(default = "default_density_shortcut")]
    workspace_density_shortcut: String,
    #[serde(default = "default_zoom_in_shortcut")]
    workspace_zoom_in_shortcut: String,
    #[serde(default = "default_zoom_out_shortcut")]
    workspace_zoom_out_shortcut: String,
}

fn default_density() -> ApplicationDensity {
    ApplicationDensity::Compact
}

fn default_theme() -> ThemeMode {
    ThemeMode::System
}

fn default_fullscreen_shortcut() -> String {
    DEFAULT_WORKSPACE_FULLSCREEN_SHORTCUT.into()
}

fn default_density_shortcut() -> String {
    DEFAULT_WORKSPACE_DENSITY_SHORTCUT.into()
}

fn default_zoom_in_shortcut() -> String {
    DEFAULT_WORKSPACE_ZOOM_IN_SHORTCUT.into()
}

fn default_zoom_out_shortcut() -> String {
    DEFAULT_WORKSPACE_ZOOM_OUT_SHORTCUT.into()
}

fn normalize_fullscreen_shortcut(shortcut: &str) -> String {
    match shortcut.trim() {
        "f11" | "F11" => "F11".into(),
        "shift-f11" => "<Shift>F11".into(),
        "ctrl-f11" => "<Ctrl>F11".into(),
        other => other.to_string(),
    }
}

fn normalize_density_shortcut(shortcut: &str) -> String {
    match shortcut.trim() {
        "ctrl-shift-d" => "<Ctrl><Shift>D".into(),
        "ctrl-shift-m" => "<Ctrl><Shift>M".into(),
        "shift-f8" => "<Shift>F8".into(),
        "<Control><Alt>ClearGrab" | "<Ctrl><Alt>ClearGrab" => "<Ctrl><Alt>KP_Multiply".into(),
        other => other.to_string(),
    }
}

fn normalize_zoom_in_shortcut(shortcut: &str) -> String {
    match shortcut.trim() {
        "ctrl-plus" => "<Ctrl>plus".into(),
        "ctrl-equal" => "<Ctrl>equal".into(),
        "ctrl-kp-add" => "<Ctrl>KP_Add".into(),
        other => other.to_string(),
    }
}

fn normalize_zoom_out_shortcut(shortcut: &str) -> String {
    match shortcut.trim() {
        "ctrl-minus" => "<Ctrl>minus".into(),
        "ctrl-kp-subtract" => "<Ctrl>KP_Subtract".into(),
        other => other.to_string(),
    }
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
                workspace_fullscreen_shortcut: normalize_fullscreen_shortcut(
                    &document.workspace_fullscreen_shortcut,
                ),
                workspace_density_shortcut: normalize_density_shortcut(
                    &document.workspace_density_shortcut,
                ),
                workspace_zoom_in_shortcut: normalize_zoom_in_shortcut(
                    &document.workspace_zoom_in_shortcut,
                ),
                workspace_zoom_out_shortcut: normalize_zoom_out_shortcut(
                    &document.workspace_zoom_out_shortcut,
                ),
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

    pub fn save_workspace_fullscreen_shortcut(&self, shortcut: &str) {
        let mut preferences = self.load();
        preferences.workspace_fullscreen_shortcut = shortcut.trim().to_string();
        self.save(&preferences);
    }

    pub fn save_workspace_density_shortcut(&self, shortcut: &str) {
        let mut preferences = self.load();
        preferences.workspace_density_shortcut = shortcut.trim().to_string();
        self.save(&preferences);
    }

    pub fn save_workspace_zoom_in_shortcut(&self, shortcut: &str) {
        let mut preferences = self.load();
        preferences.workspace_zoom_in_shortcut = shortcut.trim().to_string();
        self.save(&preferences);
    }

    pub fn save_workspace_zoom_out_shortcut(&self, shortcut: &str) {
        let mut preferences = self.load();
        preferences.workspace_zoom_out_shortcut = shortcut.trim().to_string();
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
            workspace_fullscreen_shortcut: preferences.workspace_fullscreen_shortcut.clone(),
            workspace_density_shortcut: preferences.workspace_density_shortcut.clone(),
            workspace_zoom_in_shortcut: preferences.workspace_zoom_in_shortcut.clone(),
            workspace_zoom_out_shortcut: preferences.workspace_zoom_out_shortcut.clone(),
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
            workspace_fullscreen_shortcut: default_fullscreen_shortcut(),
            workspace_density_shortcut: default_density_shortcut(),
            workspace_zoom_in_shortcut: default_zoom_in_shortcut(),
            workspace_zoom_out_shortcut: default_zoom_out_shortcut(),
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
        assert_eq!(store.load().workspace_fullscreen_shortcut, "F11");
        assert_eq!(store.load().workspace_density_shortcut, "<Ctrl><Shift>D");
        assert_eq!(store.load().workspace_zoom_in_shortcut, "<Ctrl>plus");
        assert_eq!(store.load().workspace_zoom_out_shortcut, "<Ctrl>minus");
    }

    #[test]
    fn persists_default_preferences() {
        let dir = temp_dir("pref-roundtrip");
        let store = PreferenceStore::from_path(dir.join("preferences.toml"));

        store.save(&AppPreferences {
            default_density: ApplicationDensity::Comfortable,
            default_theme: ThemeMode::Dark,
            workspace_fullscreen_shortcut: "<Shift>F11".into(),
            workspace_density_shortcut: "<Shift>F8".into(),
            workspace_zoom_in_shortcut: "<Ctrl>equal".into(),
            workspace_zoom_out_shortcut: "<Ctrl>KP_Subtract".into(),
        });

        assert_eq!(
            store.load(),
            AppPreferences {
                default_density: ApplicationDensity::Comfortable,
                default_theme: ThemeMode::Dark,
                workspace_fullscreen_shortcut: "<Shift>F11".into(),
                workspace_density_shortcut: "<Shift>F8".into(),
                workspace_zoom_in_shortcut: "<Ctrl>equal".into(),
                workspace_zoom_out_shortcut: "<Ctrl>KP_Subtract".into(),
            }
        );
    }

    #[test]
    fn loads_legacy_last_density_field() {
        let dir = temp_dir("pref-legacy");
        let path = dir.join("preferences.toml");
        fs::write(&path, "version = 1\nlast_density = \"comfortable\"\n").unwrap();

        let store = PreferenceStore::from_path(path);

        assert_eq!(
            store.load(),
            AppPreferences {
                default_density: ApplicationDensity::Comfortable,
                default_theme: ThemeMode::System,
                workspace_fullscreen_shortcut: "F11".into(),
                workspace_density_shortcut: "<Ctrl><Shift>D".into(),
                workspace_zoom_in_shortcut: "<Ctrl>plus".into(),
                workspace_zoom_out_shortcut: "<Ctrl>minus".into(),
            }
        );
    }

    #[test]
    fn normalizes_legacy_shortcut_enums_to_accelerators() {
        let dir = temp_dir("pref-legacy-shortcuts");
        let path = dir.join("preferences.toml");
        fs::write(
            &path,
            "version = 1\ndefault_theme = \"system\"\ndefault_density = \"compact\"\nworkspace_fullscreen_shortcut = \"shift-f11\"\nworkspace_density_shortcut = \"shift-f8\"\n",
        )
        .unwrap();

        let store = PreferenceStore::from_path(path);

        assert_eq!(store.load().workspace_fullscreen_shortcut, "<Shift>F11");
        assert_eq!(store.load().workspace_density_shortcut, "<Shift>F8");
        assert_eq!(store.load().workspace_zoom_in_shortcut, "<Ctrl>plus");
        assert_eq!(store.load().workspace_zoom_out_shortcut, "<Ctrl>minus");
    }

    #[test]
    fn normalizes_invalid_cleargrab_density_shortcut() {
        let dir = temp_dir("pref-cleargrab-shortcut");
        let path = dir.join("preferences.toml");
        fs::write(
            &path,
            "version = 1\ndefault_theme = \"system\"\ndefault_density = \"comfortable\"\nworkspace_fullscreen_shortcut = \"F11\"\nworkspace_density_shortcut = \"<Control><Alt>ClearGrab\"\n",
        )
        .unwrap();

        let store = PreferenceStore::from_path(path);

        assert_eq!(
            store.load().workspace_density_shortcut,
            "<Ctrl><Alt>KP_Multiply"
        );
    }

    #[test]
    fn normalizes_legacy_zoom_shortcut_enums_to_accelerators() {
        let dir = temp_dir("pref-legacy-zoom-shortcuts");
        let path = dir.join("preferences.toml");
        fs::write(
            &path,
            "version = 1\ndefault_theme = \"system\"\ndefault_density = \"compact\"\nworkspace_fullscreen_shortcut = \"F11\"\nworkspace_density_shortcut = \"<Ctrl><Shift>D\"\nworkspace_zoom_in_shortcut = \"ctrl-equal\"\nworkspace_zoom_out_shortcut = \"ctrl-kp-subtract\"\n",
        )
        .unwrap();

        let store = PreferenceStore::from_path(path);

        assert_eq!(store.load().workspace_zoom_in_shortcut, "<Ctrl>equal");
        assert_eq!(
            store.load().workspace_zoom_out_shortcut,
            "<Ctrl>KP_Subtract"
        );
    }
}
