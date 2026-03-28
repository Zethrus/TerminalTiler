use std::fs;
use std::io;
use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::app::logging;
use crate::model::preset::{WorkspacePreset, builtin_presets};
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
