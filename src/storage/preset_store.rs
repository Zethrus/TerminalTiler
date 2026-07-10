use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::app_paths;
use crate::extension::CatalogContributionProvider;
use crate::logging;
use crate::model::preset::{WorkspacePreset, builtin_presets, is_builtin_preset_id};
use crate::storage::document::{
    preserve_corrupt_warning, read_optional_string, write_toml_private,
};

const STORE_VERSION: u32 = 1;

#[derive(Clone)]
pub struct PresetStore {
    path: Option<PathBuf>,
    catalog: Option<Arc<dyn CatalogContributionProvider>>,
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

impl Default for PresetStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PresetStore {
    pub fn new() -> Self {
        let path = app_paths::config_dir().map(|dir| dir.join("presets.toml"));
        Self {
            path,
            catalog: None,
        }
    }

    pub fn with_catalog_provider(
        mut self,
        catalog: Option<Arc<dyn CatalogContributionProvider>>,
    ) -> Self {
        self.catalog = catalog;
        self
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
        let mut outcome = self.load_persisted_presets_with_status();
        if let Some(contributions) = self
            .catalog
            .as_ref()
            .and_then(|provider| provider.contributions())
        {
            let mut ids = outcome
                .presets
                .iter()
                .map(|preset| preset.id.clone())
                .collect::<std::collections::HashSet<_>>();
            outcome.presets.extend(
                contributions
                    .presets
                    .into_iter()
                    .filter(|preset| ids.insert(preset.id.clone())),
            );
        }
        outcome
    }

    fn load_persisted_presets_with_status(&self) -> PresetLoadOutcome {
        let Some(path) = &self.path else {
            return PresetLoadOutcome {
                presets: builtin_presets(),
                warning: Some(
                    "TerminalTiler could not resolve a config directory for presets.".into(),
                ),
            };
        };

        let raw = match read_optional_string(path) {
            Ok(Some(raw)) => raw,
            Ok(None) => {
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
        write_toml_private(path, &document)
    }

    fn recover_invalid_preset_document(
        &self,
        path: &std::path::Path,
        message: &str,
    ) -> PresetLoadOutcome {
        let warning = preserve_corrupt_warning(path, message);
        PresetLoadOutcome {
            presets: builtin_presets(),
            warning: Some(warning),
        }
    }
}

#[cfg(test)]
impl PresetStore {
    fn from_path(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            catalog: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{PresetDocument, PresetStore, STORE_VERSION};
    use crate::extension::{
        CatalogContributionProvider, CatalogContributions, CatalogTrustMetadata,
    };
    use crate::model::layout::{WorkingDirectory, tile};
    use crate::model::preset::{
        ApplicationDensity, ThemeMode, WorkspacePreset, builtin_presets, is_builtin_preset_id,
    };
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct TestCatalog(CatalogContributions);

    impl CatalogContributionProvider for TestCatalog {
        fn contributions(&self) -> Option<CatalogContributions> {
            Some(self.0.clone())
        }
    }

    #[test]
    fn runtime_catalog_adds_non_colliding_presets_without_persisting_them() {
        let dir = temp_dir("runtime-catalog-presets");
        let path = dir.join("presets.toml");
        let mut collision = builtin_presets()[0].clone();
        collision.name = "Must not replace user view".to_string();
        let mut contributed = collision.clone();
        contributed.id = "runtime-only-preset".to_string();
        contributed.name = "Runtime only".to_string();
        let provider = Arc::new(TestCatalog(CatalogContributions {
            namespace: "test.catalog".to_string(),
            revision: "1".to_string(),
            trust: CatalogTrustMetadata {
                read_only: true,
                ..Default::default()
            },
            presets: vec![collision, contributed],
            ..Default::default()
        }));
        let store = PresetStore::from_path(path.clone()).with_catalog_provider(Some(provider));

        let loaded = store.load_presets();
        assert!(
            loaded
                .iter()
                .any(|preset| preset.id == "runtime-only-preset")
        );
        assert!(
            !loaded
                .iter()
                .any(|preset| preset.name == "Must not replace user view")
        );
        assert!(
            !path.exists(),
            "runtime contributions must not seed user files"
        );
    }

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
            workspace_root: None,
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
    fn saves_and_loads_workspace_root_with_preset() {
        let dir = temp_dir("preset-workspace-root");
        let path = dir.join("presets.toml");
        let workspace_root = dir.join("workspace");
        fs::create_dir_all(&workspace_root).unwrap();
        let store = PresetStore::from_path(path.clone());
        let mut preset = custom_preset("rooted-preset", "Rooted Preset");
        preset.workspace_root = Some(workspace_root.clone());

        store.upsert_preset(preset).unwrap();

        let loaded = store.load_presets();
        let rooted = loaded
            .iter()
            .find(|preset| preset.id == "rooted-preset")
            .expect("saved preset should load");
        assert_eq!(
            rooted.workspace_root.as_deref(),
            Some(workspace_root.as_path())
        );
        assert!(fs::read_to_string(path).unwrap().contains("workspace_root"));
    }

    #[test]
    fn loads_legacy_presets_without_workspace_root() {
        let dir = temp_dir("preset-legacy-rootless");
        let path = dir.join("presets.toml");
        let store = PresetStore::from_path(path.clone());
        let document = PresetDocument {
            version: STORE_VERSION,
            presets: vec![custom_preset("legacy-preset", "Legacy Preset")],
        };
        fs::write(&path, toml::to_string_pretty(&document).unwrap()).unwrap();

        let loaded = store.load_presets();

        let legacy = loaded
            .iter()
            .find(|preset| preset.id == "legacy-preset")
            .expect("legacy preset should load");
        assert_eq!(legacy.workspace_root, None);
    }

    #[test]
    fn moves_corrupt_preset_file_aside_and_loads_builtins() {
        let dir = temp_dir("preset-corrupt");
        let path = dir.join("presets.toml");
        fs::write(&path, "version = [").unwrap();
        let store = PresetStore::from_path(path.clone());

        let outcome = store.load_presets_with_status();

        assert!(!path.exists());
        assert!(outcome.warning.as_deref().is_some_and(|warning| {
            warning.contains("corrupt preset file") && warning.contains("Recovery copy:")
        }));
        assert!(
            outcome
                .presets
                .iter()
                .any(|preset| preset.id == "solo-operator")
        );
        let preserved = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains("presets.toml.corrupt-")
            })
            .count();
        assert_eq!(preserved, 1);
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
    fn deletes_custom_preset_and_preserves_other_presets() {
        let dir = temp_dir("preset-delete-custom");
        let path = dir.join("presets.toml");
        let store = PresetStore::from_path(path);

        store.ensure_seeded();
        store
            .upsert_preset(custom_preset("my-preset", "My Preset"))
            .unwrap();
        store
            .upsert_preset(custom_preset("ops-preset", "Ops Preset"))
            .unwrap();

        store.delete_preset("my-preset").unwrap();

        let preset_ids = store
            .load_presets()
            .into_iter()
            .map(|preset| preset.id)
            .collect::<Vec<_>>();

        assert!(!preset_ids.iter().any(|id| id == "my-preset"));
        assert!(preset_ids.iter().any(|id| id == "ops-preset"));
        assert!(preset_ids.iter().any(|id| id == "solo-operator"));
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
