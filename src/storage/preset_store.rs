use std::fs;
use std::io;
use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::logging;
use crate::model::preset::{WorkspacePreset, builtin_presets, is_builtin_preset_id};
use crate::storage::fs_utils::{atomic_write_private, preserve_corrupt_file};

const STORE_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub struct PresetStore {
    path: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PresetDocument {
    version: u32,
    presets: Vec<WorkspacePreset>,
}

#[derive(Debug)]
pub struct PresetLoadOutcome {
    pub presets: Vec<WorkspacePreset>,
    pub warning: Option<String>,
}

impl PresetStore {
    pub fn new() -> Self {
        let path = ProjectDirs::from("dev", "Zethrus", "TerminalTiler")
            .map(|dirs| dirs.config_dir().join("presets.toml"));
        Self { path }
    }

    pub fn ensure_seeded(&self) {
        let Some(path) = &self.path else {
            return;
        };

        if path.exists() {
            return;
        }

        if let Err(error) = self.write_presets_to_path(path, &builtin_presets()) {
            logging::error(format!("failed to seed builtin presets: {}", error));
        }
    }

    pub fn load_presets(&self) -> Vec<WorkspacePreset> {
        self.load_presets_with_status().presets
    }

    pub fn load_presets_with_status(&self) -> PresetLoadOutcome {
        let Some(path) = &self.path else {
            return PresetLoadOutcome {
                presets: builtin_presets(),
                warning: Some(
                    "TerminalTiler could not resolve a config directory for presets.".into(),
                ),
            };
        };

        let raw = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return PresetLoadOutcome {
                    presets: builtin_presets(),
                    warning: None,
                };
            }
            Err(error) => {
                return PresetLoadOutcome {
                    presets: builtin_presets(),
                    warning: Some(format!(
                        "TerminalTiler could not read your preset file '{}': {}",
                        path.display(),
                        error
                    )),
                };
            }
        };

        match toml::from_str::<PresetDocument>(&raw) {
            Ok(document)
                if document.version == STORE_VERSION && !document.presets.is_empty() =>
            {
                PresetLoadOutcome {
                    presets: document.presets,
                    warning: None,
                }
            }
            Ok(_) => self.recover_invalid_preset_document(
                path,
                "TerminalTiler moved an invalid preset file aside and loaded builtin presets.",
            ),
            Err(error) => self.recover_invalid_preset_document(
                path,
                &format!(
                    "TerminalTiler found a corrupt preset file ({}) and moved it aside before loading builtin presets.",
                    error
                ),
            ),
        }
    }

    pub fn upsert_preset(&self, preset: WorkspacePreset) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Err(io::Error::other(
                "TerminalTiler config directory is unavailable",
            ));
        };

        let mut presets = self.load_presets();
        if let Some(existing) = presets.iter_mut().find(|item| item.id == preset.id) {
            *existing = preset;
        } else {
            presets.push(preset);
        }

        self.write_presets_to_path(path, &presets)
    }

    pub fn delete_preset(&self, preset_id: &str) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Err(io::Error::other(
                "TerminalTiler config directory is unavailable",
            ));
        };

        let mut presets = self.load_presets();
        let before_len = presets.len();
        presets.retain(|p| p.id != preset_id);

        if presets.len() == before_len {
            return Err(io::Error::other("Preset not found"));
        }

        self.write_presets_to_path(path, &presets)
    }

    pub fn reset_builtin_presets(&self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Err(io::Error::other(
                "TerminalTiler config directory is unavailable",
            ));
        };

        let user_presets = self
            .load_presets()
            .into_iter()
            .filter(|preset| !is_builtin_preset_id(&preset.id))
            .collect::<Vec<_>>();

        let mut presets = builtin_presets();
        presets.extend(user_presets);

        self.write_presets_to_path(path, &presets)
    }

    fn write_presets_to_path(
        &self,
        path: &std::path::Path,
        presets: &[WorkspacePreset],
    ) -> io::Result<()> {
        let document = PresetDocument {
            version: STORE_VERSION,
            presets: presets.to_vec(),
        };
        let serialized = toml::to_string_pretty(&document)
            .map_err(|error| io::Error::other(error.to_string()))?;

        atomic_write_private(path, &serialized)
    }

    fn recover_invalid_preset_document(
        &self,
        path: &std::path::Path,
        message: &str,
    ) -> PresetLoadOutcome {
        let warning = match preserve_corrupt_file(path) {
            Ok(Some(preserved)) => format!("{message} Recovery copy: {}.", preserved.display()),
            Ok(None) => message.to_string(),
            Err(error) => format!(
                "{message} TerminalTiler could not preserve the original file: {}.",
                error
            ),
        };
        logging::error(&warning);
        PresetLoadOutcome {
            presets: builtin_presets(),
            warning: Some(warning),
        }
    }
}

#[cfg(test)]
impl PresetStore {
    fn from_path(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }
}

#[cfg(test)]
mod tests {
    use super::PresetStore;
    use crate::model::layout::{WorkingDirectory, tile};
    use crate::model::preset::{
        ApplicationDensity, ThemeMode, WorkspacePreset, builtin_presets, is_builtin_preset_id,
    };
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn temp_dir(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("terminaltiler-{prefix}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn custom_preset(id: &str, name: &str) -> WorkspacePreset {
        WorkspacePreset {
            id: id.into(),
            name: name.into(),
            description: format!("{name} description"),
            tags: vec!["custom".into()],
            root_label: "Workspace root".into(),
            theme: ThemeMode::Light,
            density: ApplicationDensity::Comfortable,
            layout: tile(
                "custom-tile",
                "Custom Tile",
                "Custom Terminal",
                "accent-cyan",
                WorkingDirectory::WorkspaceRoot,
                Some("bash"),
            ),
        }
    }

    #[test]
    fn deletes_builtin_presets() {
        let dir = temp_dir("preset-delete-builtin");
        let path = dir.join("presets.toml");
        let store = PresetStore::from_path(path);

        store.ensure_seeded();
        store.delete_preset("solo-operator").unwrap();

        let preset_ids = store
            .load_presets()
            .into_iter()
            .map(|preset| preset.id)
            .collect::<Vec<_>>();

        assert!(!preset_ids.iter().any(|id| id == "solo-operator"));
        assert!(preset_ids.iter().any(|id| id == "review-pair"));
        assert!(preset_ids.iter().any(|id| id == "delivery-fleet"));
    }

    #[test]
    fn reset_builtin_presets_restores_factory_versions_and_preserves_user_presets() {
        let dir = temp_dir("preset-reset-builtin");
        let path = dir.join("presets.toml");
        let store = PresetStore::from_path(path);

        store.ensure_seeded();
        let mut modified_builtin = builtin_presets()
            .into_iter()
            .find(|preset| preset.id == "review-pair")
            .unwrap();
        modified_builtin.name = "Customized Review Pair".into();
        modified_builtin.tags = vec!["customized".into()];

        store.upsert_preset(modified_builtin).unwrap();
        store.delete_preset("solo-operator").unwrap();
        store
            .upsert_preset(custom_preset("my-preset", "My Preset"))
            .unwrap();
        store
            .upsert_preset(custom_preset("ops-preset", "Ops Preset"))
            .unwrap();

        store.reset_builtin_presets().unwrap();

        let presets = store.load_presets();
        let preset_ids = presets
            .iter()
            .map(|preset| preset.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            preset_ids,
            vec![
                "solo-operator",
                "review-pair",
                "delivery-fleet",
                "my-preset",
                "ops-preset",
            ]
        );

        let builtin = builtin_presets();
        for expected in &builtin {
            let restored = presets
                .iter()
                .find(|preset| preset.id == expected.id)
                .unwrap();
            assert_eq!(restored.name, expected.name);
            assert_eq!(restored.description, expected.description);
            assert_eq!(restored.tags, expected.tags);
            assert_eq!(restored.root_label, expected.root_label);
        }

        let user_presets = presets
            .iter()
            .filter(|preset| !is_builtin_preset_id(&preset.id))
            .collect::<Vec<_>>();
        assert_eq!(user_presets.len(), 2);
        assert_eq!(user_presets[0].id, "my-preset");
        assert_eq!(user_presets[0].name, "My Preset");
        assert_eq!(user_presets[1].id, "ops-preset");
        assert_eq!(user_presets[1].name, "Ops Preset");
    }
}
