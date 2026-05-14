//! Public open-core integration APIs for external applications.
//!
//! This module intentionally contains no external-specific behavior. Private
//! external applications can use it to install packs and synchronize the Core-owned
//! configuration files without depending on Core internals.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

pub use crate::model::{
    assets::{
        AgentRoleTemplate, CliSnippet, ConnectionProfile, InventoryGroup, InventoryHost, Runbook,
        RunbookConfirmPolicy, RunbookStep, WorkspaceAssets,
    },
    layout::{LayoutNode, ReconnectPolicy, WorkingDirectory, tile},
    preset::{ApplicationDensity, ThemeMode, WorkspacePreset},
    workspace_config::WorkspaceConfig,
};

const PRESET_STORE_VERSION: u32 = 1;
const ASSET_STORE_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigBlobKind {
    Presets,
    GlobalAssets,
    WorkspaceConfig,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfigBlob {
    pub kind: ConfigBlobKind,
    pub path: String,
    pub contents: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfigSnapshot {
    pub blobs: Vec<ConfigBlob>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictPolicy {
    PreferLocal,
    PreferRemote,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ApplyReport {
    pub applied: usize,
    pub skipped: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PackInstallReport {
    pub installed: usize,
    pub updated: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PresetPack {
    pub presets: Vec<WorkspacePreset>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetPack {
    #[serde(default)]
    pub assets: WorkspaceAssets,
}

#[derive(Debug, Serialize, Deserialize)]
struct PresetDocument {
    version: u32,
    presets: Vec<WorkspacePreset>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AssetDocument {
    version: u32,
    #[serde(default)]
    connection_profiles: Vec<ConnectionProfile>,
    #[serde(default)]
    inventory_hosts: Vec<InventoryHost>,
    #[serde(default)]
    inventory_groups: Vec<InventoryGroup>,
    #[serde(default)]
    role_templates: Vec<AgentRoleTemplate>,
    #[serde(default)]
    runbooks: Vec<Runbook>,
    #[serde(default)]
    snippets: Vec<CliSnippet>,
}

#[derive(Debug, Serialize, Deserialize)]
struct WorkspaceConfigDocument {
    version: u32,
    #[serde(default)]
    assets: WorkspaceAssets,
    #[serde(default)]
    introspection: crate::model::workspace_config::RepoIntrospectionConfig,
}

pub fn load_config_snapshot(workspace_root: Option<&Path>) -> io::Result<ConfigSnapshot> {
    let mut blobs = Vec::new();
    if let Some(path) = presets_path() {
        push_blob_if_exists(&mut blobs, ConfigBlobKind::Presets, "presets.toml", &path)?;
    }
    if let Some(path) = assets_path() {
        push_blob_if_exists(
            &mut blobs,
            ConfigBlobKind::GlobalAssets,
            "workspace-assets.toml",
            &path,
        )?;
    }
    if let Some(root) = workspace_root {
        let path = workspace_config_path(root);
        push_blob_if_exists(
            &mut blobs,
            ConfigBlobKind::WorkspaceConfig,
            ".terminaltiler/workspace.toml",
            &path,
        )?;
    }
    Ok(ConfigSnapshot { blobs })
}

pub fn apply_config_snapshot(
    snapshot: &ConfigSnapshot,
    workspace_root: Option<&Path>,
    conflict_policy: ConflictPolicy,
) -> io::Result<ApplyReport> {
    let mut report = ApplyReport::default();
    for blob in &snapshot.blobs {
        let Some(path) = path_for_blob(blob, workspace_root) else {
            report.skipped += 1;
            continue;
        };
        if path.exists() && conflict_policy == ConflictPolicy::PreferLocal {
            report.skipped += 1;
            continue;
        }
        validate_blob(blob)?;
        write_private(&path, &blob.contents)?;
        report.applied += 1;
    }
    Ok(report)
}

pub fn install_preset_pack(pack: &PresetPack) -> io::Result<PackInstallReport> {
    let Some(path) = presets_path() else {
        return Err(io::Error::other(
            "TerminalTiler config directory is unavailable",
        ));
    };
    let mut document = read_preset_document(&path)?;
    let mut report = PackInstallReport::default();
    for preset in &pack.presets {
        if let Some(existing) = document
            .presets
            .iter_mut()
            .find(|existing| existing.id == preset.id)
        {
            *existing = preset.clone();
            report.updated += 1;
        } else {
            document.presets.push(preset.clone());
            report.installed += 1;
        }
    }
    write_toml(&path, &document)?;
    Ok(report)
}

pub fn install_asset_pack(pack: &AssetPack) -> io::Result<PackInstallReport> {
    let Some(path) = assets_path() else {
        return Err(io::Error::other(
            "TerminalTiler config directory is unavailable",
        ));
    };
    let mut assets = read_asset_document(&path)?.into_assets();
    let mut report = PackInstallReport::default();
    merge_by_id(
        &mut assets.connection_profiles,
        &pack.assets.connection_profiles,
        |item| &item.id,
        &mut report,
    );
    merge_by_id(
        &mut assets.inventory_hosts,
        &pack.assets.inventory_hosts,
        |item| &item.id,
        &mut report,
    );
    merge_by_id(
        &mut assets.inventory_groups,
        &pack.assets.inventory_groups,
        |item| &item.id,
        &mut report,
    );
    merge_by_id(
        &mut assets.role_templates,
        &pack.assets.role_templates,
        |item| &item.id,
        &mut report,
    );
    merge_by_id(
        &mut assets.runbooks,
        &pack.assets.runbooks,
        |item| &item.id,
        &mut report,
    );
    merge_by_id(
        &mut assets.snippets,
        &pack.assets.snippets,
        |item| &item.id,
        &mut report,
    );
    write_toml(&path, &AssetDocument::from_assets(assets))?;
    Ok(report)
}

fn path_for_blob(blob: &ConfigBlob, workspace_root: Option<&Path>) -> Option<PathBuf> {
    match blob.kind {
        ConfigBlobKind::Presets => presets_path(),
        ConfigBlobKind::GlobalAssets => assets_path(),
        ConfigBlobKind::WorkspaceConfig => workspace_root.map(workspace_config_path),
    }
}

fn validate_blob(blob: &ConfigBlob) -> io::Result<()> {
    match blob.kind {
        ConfigBlobKind::Presets => toml::from_str::<PresetDocument>(&blob.contents)
            .map(|_| ())
            .map_err(toml_error),
        ConfigBlobKind::GlobalAssets => toml::from_str::<AssetDocument>(&blob.contents)
            .map(|_| ())
            .map_err(toml_error),
        ConfigBlobKind::WorkspaceConfig => {
            toml::from_str::<WorkspaceConfigDocument>(&blob.contents)
                .map(|_| ())
                .map_err(toml_error)
        }
    }
}

fn push_blob_if_exists(
    blobs: &mut Vec<ConfigBlob>,
    kind: ConfigBlobKind,
    label: &str,
    path: &Path,
) -> io::Result<()> {
    match fs::read_to_string(path) {
        Ok(contents) => blobs.push(ConfigBlob {
            kind,
            path: label.to_string(),
            contents,
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

fn read_preset_document(path: &Path) -> io::Result<PresetDocument> {
    match fs::read_to_string(path) {
        Ok(raw) => toml::from_str(&raw).map_err(toml_error),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(PresetDocument {
            version: PRESET_STORE_VERSION,
            presets: crate::model::preset::builtin_presets(),
        }),
        Err(error) => Err(error),
    }
}

fn read_asset_document(path: &Path) -> io::Result<AssetDocument> {
    match fs::read_to_string(path) {
        Ok(raw) => toml::from_str(&raw).map_err(toml_error),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(AssetDocument::from_assets(WorkspaceAssets {
                role_templates: crate::model::assets::builtin_role_templates(),
                ..WorkspaceAssets::default()
            }))
        }
        Err(error) => Err(error),
    }
}

impl AssetDocument {
    fn from_assets(assets: WorkspaceAssets) -> Self {
        Self {
            version: ASSET_STORE_VERSION,
            connection_profiles: assets.connection_profiles,
            inventory_hosts: assets.inventory_hosts,
            inventory_groups: assets.inventory_groups,
            role_templates: assets.role_templates,
            runbooks: assets.runbooks,
            snippets: assets.snippets,
        }
    }

    fn into_assets(self) -> WorkspaceAssets {
        WorkspaceAssets {
            connection_profiles: self.connection_profiles,
            inventory_hosts: self.inventory_hosts,
            inventory_groups: self.inventory_groups,
            role_templates: self.role_templates,
            runbooks: self.runbooks,
            snippets: self.snippets,
        }
    }
}

fn merge_by_id<T, F>(target: &mut Vec<T>, incoming: &[T], id: F, report: &mut PackInstallReport)
where
    T: Clone,
    F: Fn(&T) -> &str,
{
    for item in incoming {
        if let Some(existing) = target.iter_mut().find(|existing| id(existing) == id(item)) {
            *existing = item.clone();
            report.updated += 1;
        } else {
            target.push(item.clone());
            report.installed += 1;
        }
    }
}

fn presets_path() -> Option<PathBuf> {
    project_dirs().map(|dirs| dirs.config_dir().join("presets.toml"))
}

fn assets_path() -> Option<PathBuf> {
    project_dirs().map(|dirs| dirs.config_dir().join("workspace-assets.toml"))
}

fn workspace_config_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".terminaltiler").join("workspace.toml")
}

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("dev", "Zethrus", "TerminalTiler")
}

fn write_toml<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let raw = toml::to_string_pretty(value).map_err(toml_ser_error)?;
    write_private(path, &raw)
}

fn write_private(path: &Path, contents: &str) -> io::Result<()> {
    crate::storage::fs_utils::atomic_write_private(path, contents)
}

fn toml_error(error: toml::de::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn toml_ser_error(error: toml::ser::Error) -> io::Error {
    io::Error::other(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        assets::CliSnippet,
        layout::{WorkingDirectory, tile},
        preset::{ApplicationDensity, ThemeMode},
    };
    use std::sync::{Mutex, OnceLock};
    use uuid::Uuid;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn temp_config_home(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "terminaltiler-open-core-{prefix}-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn with_config_home<T>(prefix: &str, test: impl FnOnce() -> T) -> T {
        let _guard = env_lock().lock().unwrap();
        let dir = temp_config_home(prefix);
        let previous = std::env::var_os("XDG_CONFIG_HOME");
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", &dir);
        }
        let result = test();
        unsafe {
            match previous {
                Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }
        result
    }

    fn sample_preset(id: &str) -> WorkspacePreset {
        WorkspacePreset {
            id: id.into(),
            name: "Sample Delivery".into(),
            description: "Sample pack preset".into(),
            tags: vec!["sample".into()],
            root_label: "Workspace".into(),
            workspace_root: None,
            theme: ThemeMode::Dark,
            density: ApplicationDensity::Compact,
            layout: tile(
                "primary",
                "Primary",
                "Shell",
                "accent-cyan",
                WorkingDirectory::WorkspaceRoot,
                Some("bash"),
            ),
        }
    }

    #[test]
    fn installs_preset_pack_and_exposes_snapshot_blob() {
        with_config_home("preset-pack", || {
            let report = install_preset_pack(&PresetPack {
                presets: vec![sample_preset("sample-delivery")],
            })
            .unwrap();
            assert_eq!(report.installed, 1);
            assert_eq!(report.updated, 0);

            let snapshot = load_config_snapshot(None).unwrap();
            let presets = snapshot
                .blobs
                .iter()
                .find(|blob| blob.kind == ConfigBlobKind::Presets)
                .expect("preset blob should be present");
            assert!(presets.contents.contains("sample-delivery"));
        });
    }

    #[test]
    fn installs_asset_pack_by_stable_ids() {
        with_config_home("asset-pack", || {
            let report = install_asset_pack(&AssetPack {
                assets: WorkspaceAssets {
                    snippets: vec![CliSnippet {
                        id: "sample-status".into(),
                        name: "Sync status".into(),
                        description: "Check sample status".into(),
                        command: "terminaltiler sample status".into(),
                        variables: Vec::new(),
                        tags: vec!["sample".into()],
                    }],
                    ..WorkspaceAssets::default()
                },
            })
            .unwrap();
            assert_eq!(report.installed, 1);

            let updated = install_asset_pack(&AssetPack {
                assets: WorkspaceAssets {
                    snippets: vec![CliSnippet {
                        id: "sample-status".into(),
                        name: "Sync status updated".into(),
                        description: "Check sample status".into(),
                        command: "terminaltiler sample status".into(),
                        variables: Vec::new(),
                        tags: vec!["sample".into()],
                    }],
                    ..WorkspaceAssets::default()
                },
            })
            .unwrap();
            assert_eq!(updated.updated, 1);
        });
    }
}
