use std::fs;
use std::io;
use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::logging;
use crate::model::assets::{WorkspaceAssets, builtin_role_templates};
use crate::storage::fs_utils::{atomic_write_private, preserve_corrupt_file};

const STORE_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub struct AssetStore {
    path: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AssetDocument {
    version: u32,
    #[serde(default)]
    connection_profiles: Vec<crate::model::assets::ConnectionProfile>,
    #[serde(default)]
    inventory_hosts: Vec<crate::model::assets::InventoryHost>,
    #[serde(default)]
    inventory_groups: Vec<crate::model::assets::InventoryGroup>,
    #[serde(default = "builtin_role_templates")]
    role_templates: Vec<crate::model::assets::AgentRoleTemplate>,
}

#[derive(Debug)]
pub struct AssetLoadOutcome {
    pub assets: WorkspaceAssets,
    pub warning: Option<String>,
}

impl AssetStore {
    pub fn new() -> Self {
        let path = ProjectDirs::from("dev", "Zethrus", "TerminalTiler")
            .map(|dirs| dirs.config_dir().join("workspace-assets.toml"));
        Self { path }
    }

    pub fn ensure_seeded(&self) {
        let Some(path) = &self.path else {
            return;
        };
        if path.exists() {
            return;
        }
        if let Err(error) = self.write_assets_to_path(path, &WorkspaceAssets::default()) {
            logging::error(format!("failed to seed workspace assets: {}", error));
        }
    }

    pub fn load_assets(&self) -> WorkspaceAssets {
        self.load_assets_with_status().assets
    }

    pub fn load_assets_with_status(&self) -> AssetLoadOutcome {
        let Some(path) = self.path.as_ref() else {
            return AssetLoadOutcome {
                assets: default_assets(),
                warning: Some(
                    "TerminalTiler could not resolve a config directory for workspace assets."
                        .into(),
                ),
            };
        };

        let raw = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return AssetLoadOutcome {
                    assets: default_assets(),
                    warning: None,
                };
            }
            Err(error) => {
                return AssetLoadOutcome {
                    assets: default_assets(),
                    warning: Some(format!(
                        "TerminalTiler could not read workspace assets '{}': {}",
                        path.display(),
                        error
                    )),
                };
            }
        };

        match toml::from_str::<AssetDocument>(&raw) {
            Ok(document) if document.version == STORE_VERSION => AssetLoadOutcome {
                assets: merge_builtins(WorkspaceAssets {
                    connection_profiles: document.connection_profiles,
                    inventory_hosts: document.inventory_hosts,
                    inventory_groups: document.inventory_groups,
                    role_templates: document.role_templates,
                }),
                warning: None,
            },
            Ok(_) => self.recover_invalid_asset_document(
                path,
                "TerminalTiler moved an invalid workspace assets file aside and loaded defaults.",
            ),
            Err(error) => self.recover_invalid_asset_document(
                path,
                &format!(
                    "TerminalTiler found a corrupt workspace assets file ({error}) and moved it aside before loading defaults."
                ),
            ),
        }
    }

    pub fn save_assets(&self, assets: &WorkspaceAssets) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Err(io::Error::other(
                "TerminalTiler config directory is unavailable",
            ));
        };
        self.write_assets_to_path(path, assets)
    }

    fn write_assets_to_path(&self, path: &std::path::Path, assets: &WorkspaceAssets) -> io::Result<()> {
        let document = AssetDocument {
            version: STORE_VERSION,
            connection_profiles: assets.connection_profiles.clone(),
            inventory_hosts: assets.inventory_hosts.clone(),
            inventory_groups: assets.inventory_groups.clone(),
            role_templates: merge_builtins(WorkspaceAssets {
                connection_profiles: Vec::new(),
                inventory_hosts: Vec::new(),
                inventory_groups: Vec::new(),
                role_templates: assets.role_templates.clone(),
            })
            .role_templates,
        };
        let serialized = toml::to_string_pretty(&document)
            .map_err(|error| io::Error::other(error.to_string()))?;
        atomic_write_private(path, &serialized)
    }

    fn recover_invalid_asset_document(
        &self,
        path: &std::path::Path,
        message: &str,
    ) -> AssetLoadOutcome {
        let warning = match preserve_corrupt_file(path) {
            Ok(Some(preserved)) => format!("{message} Recovery copy: {}.", preserved.display()),
            Ok(None) => message.to_string(),
            Err(error) => format!(
                "{message} TerminalTiler could not preserve the original file: {}.",
                error
            ),
        };
        logging::error(&warning);
        AssetLoadOutcome {
            assets: default_assets(),
            warning: Some(warning),
        }
    }
}

fn default_assets() -> WorkspaceAssets {
    WorkspaceAssets {
        role_templates: builtin_role_templates(),
        ..WorkspaceAssets::default()
    }
}

fn merge_builtins(mut assets: WorkspaceAssets) -> WorkspaceAssets {
    for builtin in builtin_role_templates() {
        if assets.role_templates.iter().all(|role| role.id != builtin.id) {
            assets.role_templates.push(builtin);
        }
    }
    assets
}
