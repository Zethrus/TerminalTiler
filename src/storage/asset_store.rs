use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::app_paths;
use crate::extension::{
    CatalogContributionProvider, CatalogItemKind, CatalogPersistedIds, CatalogViewEntry,
};
use crate::logging;
use crate::model::assets::{WorkspaceAssets, builtin_role_templates};
use crate::model::workspace_config::{ConfigScope, WorkspaceConfig};
use crate::storage::document::{
    preserve_corrupt_warning, read_optional_string, write_toml_private,
};
use crate::storage::workspace_config_store::WorkspaceConfigStore;

const STORE_VERSION: u32 = 1;

#[derive(Clone)]
pub struct AssetStore {
    path: Option<PathBuf>,
    workspace_config_store: WorkspaceConfigStore,
    catalog: Option<Arc<dyn CatalogContributionProvider>>,
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
    #[serde(default)]
    runbooks: Vec<crate::model::assets::Runbook>,
    #[serde(default)]
    snippets: Vec<crate::model::assets::CliSnippet>,
}

#[derive(Debug)]
pub struct AssetLoadOutcome {
    pub assets: WorkspaceAssets,
    pub warning: Option<String>,
}

impl Default for AssetStore {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetStore {
    pub fn new() -> Self {
        let path = app_paths::config_dir().map(|dir| dir.join("workspace-assets.toml"));
        Self {
            path,
            workspace_config_store: WorkspaceConfigStore::new(),
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

    #[cfg_attr(target_os = "windows", allow(dead_code))]
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

    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub fn load_assets(&self) -> WorkspaceAssets {
        self.load_assets_with_status().assets
    }

    pub fn load_assets_for_workspace_root(&self, workspace_root: &Path) -> AssetLoadOutcome {
        let global = self.load_assets_with_status();
        let workspace = self.workspace_config_store.load_for_root(workspace_root);
        AssetLoadOutcome {
            assets: merge_assets_with_builtins(merge_workspace_assets_for_view(
                &global.assets,
                &workspace.config.assets,
            )),
            warning: combine_warnings(global.warning, workspace.warning),
        }
    }

    pub fn load_assets_with_status(&self) -> AssetLoadOutcome {
        let mut outcome = self.load_persisted_assets_with_status();
        if let Some(contributions) = self
            .catalog
            .as_ref()
            .and_then(|provider| provider.contributions())
        {
            extend_unique_by_id(
                &mut outcome.assets.role_templates,
                contributions.role_templates,
                |item| item.id.as_str(),
            );
            extend_unique_by_id(
                &mut outcome.assets.runbooks,
                contributions.runbooks,
                |item| item.id.as_str(),
            );
            extend_unique_by_id(
                &mut outcome.assets.snippets,
                contributions.snippets,
                |item| item.id.as_str(),
            );
        }
        outcome
    }

    fn load_persisted_assets_with_status(&self) -> AssetLoadOutcome {
        let Some(path) = self.path.as_ref() else {
            return AssetLoadOutcome {
                assets: default_assets(),
                warning: Some(
                    "TerminalTiler could not resolve a config directory for workspace assets."
                        .into(),
                ),
            };
        };

        let raw = match read_optional_string(path) {
            Ok(Some(raw)) => raw,
            Ok(None) => {
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
                assets: merge_assets_with_builtins(WorkspaceAssets {
                    connection_profiles: document.connection_profiles,
                    inventory_hosts: document.inventory_hosts,
                    inventory_groups: document.inventory_groups,
                    role_templates: document.role_templates,
                    runbooks: document.runbooks,
                    snippets: document.snippets,
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

    pub fn load_workspace_config(&self, workspace_root: &Path) -> WorkspaceConfig {
        self.workspace_config_store
            .load_for_root(workspace_root)
            .config
    }

    pub fn save_assets_for_scope(
        &self,
        assets: &WorkspaceAssets,
        scope: ConfigScope,
        workspace_root: Option<&Path>,
    ) -> io::Result<()> {
        match scope {
            ConfigScope::Global => self.save_assets(assets),
            ConfigScope::Workspace => {
                let workspace_root = workspace_root.ok_or_else(|| {
                    io::Error::other("workspace root is required for workspace scoped assets")
                })?;
                let mut config = self
                    .workspace_config_store
                    .load_for_root(workspace_root)
                    .config;
                config.assets = self.without_provider_only_assets(assets);
                self.workspace_config_store
                    .save_for_root(workspace_root, &config)
            }
        }
    }

    pub fn catalog_view_metadata(&self) -> Vec<CatalogViewEntry> {
        let assets = self.load_persisted_assets_with_status().assets;
        let persisted = CatalogPersistedIds {
            role_templates: assets
                .role_templates
                .into_iter()
                .map(|item| item.id)
                .collect(),
            runbooks: assets.runbooks.into_iter().map(|item| item.id).collect(),
            snippets: assets.snippets.into_iter().map(|item| item.id).collect(),
            ..CatalogPersistedIds::default()
        };
        self.catalog
            .as_ref()
            .map(|provider| provider.view_metadata(&persisted))
            .unwrap_or_default()
    }

    fn write_assets_to_path(
        &self,
        path: &std::path::Path,
        assets: &WorkspaceAssets,
    ) -> io::Result<()> {
        let assets = self.without_provider_only_assets(assets);
        let document = AssetDocument {
            version: STORE_VERSION,
            connection_profiles: assets.connection_profiles.clone(),
            inventory_hosts: assets.inventory_hosts.clone(),
            inventory_groups: assets.inventory_groups.clone(),
            role_templates: merge_assets_with_builtins(WorkspaceAssets {
                connection_profiles: Vec::new(),
                inventory_hosts: Vec::new(),
                inventory_groups: Vec::new(),
                role_templates: assets.role_templates.clone(),
                runbooks: Vec::new(),
                snippets: Vec::new(),
            })
            .role_templates,
            runbooks: assets.runbooks.clone(),
            snippets: assets.snippets.clone(),
        };
        write_toml_private(path, &document)
    }

    fn without_provider_only_assets(&self, assets: &WorkspaceAssets) -> WorkspaceAssets {
        let persisted = self.load_persisted_assets_with_status().assets;
        let persisted_role_ids = persisted
            .role_templates
            .into_iter()
            .map(|item| item.id)
            .collect::<HashSet<_>>();
        let persisted_runbook_ids = persisted
            .runbooks
            .into_iter()
            .map(|item| item.id)
            .collect::<HashSet<_>>();
        let persisted_snippet_ids = persisted
            .snippets
            .into_iter()
            .map(|item| item.id)
            .collect::<HashSet<_>>();
        let metadata = self.catalog_view_metadata();
        let provider_ids = |kind| {
            metadata
                .iter()
                .filter(|entry| entry.kind == kind && entry.origin.read_only())
                .map(|entry| entry.id.clone())
                .collect::<HashSet<_>>()
        };
        let provider_role_ids = provider_ids(CatalogItemKind::RoleTemplate);
        let provider_runbook_ids = provider_ids(CatalogItemKind::Runbook);
        let provider_snippet_ids = provider_ids(CatalogItemKind::Snippet);
        let mut filtered = assets.clone();
        filtered.role_templates.retain(|item| {
            !provider_role_ids.contains(&item.id) || persisted_role_ids.contains(&item.id)
        });
        filtered.runbooks.retain(|item| {
            !provider_runbook_ids.contains(&item.id) || persisted_runbook_ids.contains(&item.id)
        });
        filtered.snippets.retain(|item| {
            !provider_snippet_ids.contains(&item.id) || persisted_snippet_ids.contains(&item.id)
        });
        filtered
    }

    fn recover_invalid_asset_document(
        &self,
        path: &std::path::Path,
        message: &str,
    ) -> AssetLoadOutcome {
        let warning = preserve_corrupt_warning(path, message);
        AssetLoadOutcome {
            assets: default_assets(),
            warning: Some(warning),
        }
    }
}

#[cfg(test)]
impl AssetStore {
    fn from_path(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            workspace_config_store: WorkspaceConfigStore::new(),
            catalog: None,
        }
    }
}

fn extend_unique_by_id<T, F>(current: &mut Vec<T>, additions: Vec<T>, id_of: F)
where
    F: Fn(&T) -> &str,
{
    let mut ids = current
        .iter()
        .map(|item| id_of(item).to_string())
        .collect::<HashSet<_>>();
    current.extend(
        additions
            .into_iter()
            .filter(|item| ids.insert(id_of(item).to_string())),
    );
}

fn default_assets() -> WorkspaceAssets {
    WorkspaceAssets {
        role_templates: builtin_role_templates(),
        ..WorkspaceAssets::default()
    }
}

pub fn merge_assets_with_builtins(mut assets: WorkspaceAssets) -> WorkspaceAssets {
    for builtin in builtin_role_templates() {
        if assets
            .role_templates
            .iter()
            .all(|role| role.id != builtin.id)
        {
            assets.role_templates.push(builtin);
        }
    }
    assets
}

fn combine_warnings(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(first), Some(second)) if !second.trim().is_empty() => {
            Some(format!("{first}\n{second}"))
        }
        (Some(first), _) => Some(first),
        (_, Some(second)) => Some(second),
        (None, None) => None,
    }
}

pub fn merge_workspace_assets_for_view(
    global: &WorkspaceAssets,
    workspace: &WorkspaceAssets,
) -> WorkspaceAssets {
    WorkspaceAssets {
        connection_profiles: merge_by_id(
            &global.connection_profiles,
            &workspace.connection_profiles,
            |item| item.id.as_str(),
        ),
        inventory_hosts: merge_by_id(
            &global.inventory_hosts,
            &workspace.inventory_hosts,
            |item| item.id.as_str(),
        ),
        inventory_groups: merge_by_id(
            &global.inventory_groups,
            &workspace.inventory_groups,
            |item| item.id.as_str(),
        ),
        role_templates: merge_by_id(&global.role_templates, &workspace.role_templates, |item| {
            item.id.as_str()
        }),
        runbooks: merge_by_id(&global.runbooks, &workspace.runbooks, |item| {
            item.id.as_str()
        }),
        snippets: merge_by_id(&global.snippets, &workspace.snippets, |item| {
            item.id.as_str()
        }),
    }
}

fn merge_by_id<T, F>(global: &[T], workspace: &[T], id_of: F) -> Vec<T>
where
    T: Clone,
    F: Fn(&T) -> &str,
{
    let workspace_ids = workspace
        .iter()
        .map(|item| id_of(item).to_string())
        .collect::<HashSet<_>>();
    let mut merged = global
        .iter()
        .filter(|item| !workspace_ids.contains(id_of(item)))
        .cloned()
        .collect::<Vec<_>>();
    merged.extend(workspace.iter().cloned());
    merged
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    use uuid::Uuid;

    use super::AssetStore;
    use crate::extension::{
        CatalogContributionProvider, CatalogContributions, CatalogItemKind, CatalogTrustMetadata,
    };
    use crate::model::assets::CliSnippet;

    struct TestCatalog(CatalogContributions);

    impl CatalogContributionProvider for TestCatalog {
        fn contributions(&self) -> Option<CatalogContributions> {
            Some(self.0.clone())
        }
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("terminaltiler-{prefix}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn moves_corrupt_asset_file_aside_and_loads_defaults() {
        let dir = temp_dir("corrupt-assets");
        let path = dir.join("workspace-assets.toml");
        fs::write(&path, "not = [valid").unwrap();
        let store = AssetStore::from_path(path.clone());

        let outcome = store.load_assets_with_status();

        assert!(!path.exists());
        assert!(outcome.warning.as_deref().is_some_and(|warning| {
            warning.contains("corrupt workspace assets file") && warning.contains("Recovery copy:")
        }));
        assert!(!outcome.assets.role_templates.is_empty());
        let preserved = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains("workspace-assets.toml.corrupt-")
            })
            .count();
        assert_eq!(preserved, 1);
    }

    #[test]
    fn saving_effective_assets_never_persists_provider_items() {
        let dir = temp_dir("provider-assets");
        let path = dir.join("workspace-assets.toml");
        let provider = Arc::new(TestCatalog(CatalogContributions {
            namespace: "test.catalog".into(),
            revision: "1".into(),
            trust: CatalogTrustMetadata {
                read_only: true,
                executable_content: true,
                trusted: true,
            },
            snippets: vec![CliSnippet {
                id: "provider-command".into(),
                name: "Provider command".into(),
                description: String::new(),
                command: "echo managed".into(),
                variables: Vec::new(),
                tags: Vec::new(),
            }],
            ..CatalogContributions::default()
        }));
        let store = AssetStore::from_path(path.clone()).with_catalog_provider(Some(provider));

        let effective = store.load_assets();
        assert!(
            effective
                .snippets
                .iter()
                .any(|snippet| snippet.id == "provider-command")
        );
        store.save_assets(&effective).unwrap();

        assert!(
            !fs::read_to_string(path)
                .unwrap()
                .contains("provider-command")
        );
        let metadata = store.catalog_view_metadata();
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].kind, CatalogItemKind::Snippet);
        assert!(metadata[0].origin.read_only());
    }
}
