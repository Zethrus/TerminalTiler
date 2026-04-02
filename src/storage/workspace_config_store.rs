use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::logging;
use crate::model::workspace_config::WorkspaceConfig;
use crate::storage::fs_utils::{atomic_write_private, preserve_corrupt_file};

const STORE_VERSION: u32 = 1;
const WORKSPACE_CONFIG_DIR: &str = ".terminaltiler";
const WORKSPACE_CONFIG_FILE: &str = "workspace.toml";

#[derive(Clone, Debug, Default)]
pub struct WorkspaceConfigStore;

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct WorkspaceConfigLoadOutcome {
    pub config: WorkspaceConfig,
    pub warning: Option<String>,
    pub path: PathBuf,
    pub exists: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct WorkspaceConfigDocument {
    version: u32,
    #[serde(default)]
    assets: crate::model::assets::WorkspaceAssets,
    #[serde(default)]
    introspection: crate::model::workspace_config::RepoIntrospectionConfig,
}

impl WorkspaceConfigStore {
    pub fn new() -> Self {
        Self
    }

    pub fn path_for_root(&self, workspace_root: &Path) -> PathBuf {
        workspace_root
            .join(WORKSPACE_CONFIG_DIR)
            .join(WORKSPACE_CONFIG_FILE)
    }

    pub fn load_for_root(&self, workspace_root: &Path) -> WorkspaceConfigLoadOutcome {
        let path = self.path_for_root(workspace_root);
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return WorkspaceConfigLoadOutcome {
                    config: WorkspaceConfig::default(),
                    warning: None,
                    path,
                    exists: false,
                };
            }
            Err(error) => {
                return WorkspaceConfigLoadOutcome {
                    config: WorkspaceConfig::default(),
                    warning: Some(format!(
                        "TerminalTiler could not read workspace config '{}': {}",
                        path.display(),
                        error
                    )),
                    path,
                    exists: true,
                };
            }
        };

        match toml::from_str::<WorkspaceConfigDocument>(&raw) {
            Ok(document) if document.version == STORE_VERSION => WorkspaceConfigLoadOutcome {
                config: WorkspaceConfig {
                    assets: document.assets,
                    introspection: document.introspection,
                },
                warning: None,
                path,
                exists: true,
            },
            Ok(_) => self.recover_invalid_workspace_config(
                &path,
                "TerminalTiler moved an invalid workspace config aside and loaded defaults.",
            ),
            Err(error) => self.recover_invalid_workspace_config(
                &path,
                &format!(
                    "TerminalTiler found a corrupt workspace config ({error}) and moved it aside before loading defaults."
                ),
            ),
        }
    }

    pub fn save_for_root(&self, workspace_root: &Path, config: &WorkspaceConfig) -> io::Result<()> {
        let path = self.path_for_root(workspace_root);
        let document = WorkspaceConfigDocument {
            version: STORE_VERSION,
            assets: config.assets.clone(),
            introspection: config.introspection.clone(),
        };
        let serialized = toml::to_string_pretty(&document)
            .map_err(|error| io::Error::other(error.to_string()))?;
        atomic_write_private(&path, &serialized)
    }

    fn recover_invalid_workspace_config(
        &self,
        path: &Path,
        message: &str,
    ) -> WorkspaceConfigLoadOutcome {
        let warning = match preserve_corrupt_file(path) {
            Ok(Some(preserved)) => format!("{message} Recovery copy: {}.", preserved.display()),
            Ok(None) => message.to_string(),
            Err(error) => format!(
                "{message} TerminalTiler could not preserve the original file: {}.",
                error
            ),
        };
        logging::error(&warning);
        WorkspaceConfigLoadOutcome {
            config: WorkspaceConfig::default(),
            warning: Some(warning),
            path: path.to_path_buf(),
            exists: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::WorkspaceConfigStore;
    use crate::model::assets::Runbook;
    use crate::model::workspace_config::WorkspaceConfig;
    use uuid::Uuid;

    fn temp_root(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("terminaltiler-{prefix}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn loads_defaults_when_workspace_config_is_missing() {
        let root = temp_root("workspace-config-missing");
        let store = WorkspaceConfigStore::new();

        let outcome = store.load_for_root(&root);

        assert!(!outcome.exists);
        assert_eq!(outcome.config, WorkspaceConfig::default());
    }

    #[test]
    fn saves_and_loads_workspace_config() {
        let root = temp_root("workspace-config-roundtrip");
        let store = WorkspaceConfigStore::new();
        let mut config = WorkspaceConfig::default();
        config.assets.runbooks.push(Runbook {
            id: "rb-1".into(),
            name: "Deploy".into(),
            description: String::new(),
            tags: vec!["ops".into()],
            target: Default::default(),
            variables: Vec::new(),
            steps: Vec::new(),
            confirm_policy: Default::default(),
        });

        store.save_for_root(&root, &config).unwrap();
        let loaded = store.load_for_root(&root);

        assert!(loaded.exists);
        assert_eq!(loaded.config, config);
    }
}
