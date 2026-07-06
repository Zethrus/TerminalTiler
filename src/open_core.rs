//! Public open-core integration APIs for external applications.
//!
//! This module intentionally contains no external-specific behavior. Private
//! external applications can use it to install packs and synchronize the Core-owned
//! configuration files without depending on Core internals.

use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::app_paths;

pub use crate::model::{
    assets::{
        AgentRoleTemplate, CliSnippet, ConnectionProfile, InventoryGroup, InventoryHost, Runbook,
        RunbookConfirmPolicy, RunbookStep, WorkspaceAssets,
    },
    layout::{LayoutNode, ReconnectPolicy, WorkingDirectory, tile},
    preset::{ApplicationDensity, ThemeMode, WorkspacePreset},
    workspace_config::WorkspaceConfig,
};
pub use crate::{
    model::assets::RestoreLaunchMode,
    voice::{VoiceActivationMode, VoiceEngineMode},
};

const PRESET_STORE_VERSION: u32 = 1;
const ASSET_STORE_VERSION: u32 = 1;
const PREFERENCE_STORE_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigBlobKind {
    Presets,
    GlobalAssets,
    WorkspaceConfig,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncDocumentKind {
    Preferences,
    Presets,
    GlobalAssets,
    WorkspaceConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncSnapshotOptions {
    pub include_preferences: bool,
    pub include_presets: bool,
    pub include_global_assets: bool,
    pub include_workspace_configs: bool,
    pub workspace_roots: Vec<PathBuf>,
}

impl Default for SyncSnapshotOptions {
    fn default() -> Self {
        Self {
            include_preferences: true,
            include_presets: true,
            include_global_assets: true,
            include_workspace_configs: true,
            workspace_roots: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SyncDocument {
    pub kind: SyncDocumentKind,
    /// Stable logical document id. Pro encrypts this before using it as a server object id.
    pub id: String,
    /// Human-readable logical path for local routing only. Pro must not send this plaintext to v2 APIs.
    pub logical_path: String,
    pub contents: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SyncSnapshot {
    pub documents: Vec<SyncDocument>,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PreferenceDocument {
    version: u32,
    #[serde(default = "default_density", alias = "last_density")]
    default_density: ApplicationDensity,
    #[serde(default = "default_theme")]
    default_theme: ThemeMode,
    #[serde(default = "default_close_to_background")]
    close_to_background: bool,
    #[serde(default)]
    default_restore_mode: RestoreLaunchMode,
    #[serde(default = "default_fullscreen_shortcut")]
    workspace_fullscreen_shortcut: String,
    #[serde(default = "default_density_shortcut")]
    workspace_density_shortcut: String,
    #[serde(default = "default_zoom_in_shortcut")]
    workspace_zoom_in_shortcut: String,
    #[serde(default = "default_zoom_out_shortcut")]
    workspace_zoom_out_shortcut: String,
    #[serde(default = "default_tile_selection_prefix_shortcut")]
    workspace_tile_selection_prefix_shortcut: String,
    #[serde(default = "default_command_palette_shortcut")]
    command_palette_shortcut: String,
    #[serde(default = "default_max_reconnect_attempts")]
    max_reconnect_attempts: u32,
    #[serde(default)]
    voice: SyncVoicePreferences,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SyncVoicePreferences {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    activation_mode: VoiceActivationMode,
    #[serde(default = "default_voice_hotkey")]
    hotkey: String,
    #[serde(default)]
    prefer_global_hotkey: bool,
    #[serde(default)]
    engine_mode: VoiceEngineMode,
}

impl Default for SyncVoicePreferences {
    fn default() -> Self {
        Self {
            enabled: false,
            activation_mode: VoiceActivationMode::PushToTalk,
            hotkey: default_voice_hotkey(),
            prefer_global_hotkey: false,
            engine_mode: VoiceEngineMode::Auto,
        }
    }
}

fn default_density() -> ApplicationDensity {
    ApplicationDensity::Compact
}

fn default_theme() -> ThemeMode {
    ThemeMode::System
}

fn default_close_to_background() -> bool {
    false
}

fn default_fullscreen_shortcut() -> String {
    "F11".into()
}

fn default_density_shortcut() -> String {
    "<Ctrl><Shift>D".into()
}

fn default_zoom_in_shortcut() -> String {
    "<Ctrl>plus".into()
}

fn default_zoom_out_shortcut() -> String {
    "<Ctrl>minus".into()
}

fn default_tile_selection_prefix_shortcut() -> String {
    "<Alt>T".into()
}

fn default_command_palette_shortcut() -> String {
    "<Ctrl><Shift>P".into()
}

fn default_max_reconnect_attempts() -> u32 {
    5
}

fn default_voice_hotkey() -> String {
    crate::voice::preferences::DEFAULT_VOICE_HOTKEY.into()
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

pub fn load_sync_snapshot(options: SyncSnapshotOptions) -> io::Result<SyncSnapshot> {
    let mut documents = Vec::new();
    if options.include_preferences
        && let Some(path) = preferences_path()
    {
        push_preference_document_if_exists(&mut documents, &path)?;
    }
    if options.include_presets
        && let Some(path) = presets_path()
    {
        push_sync_document_if_exists(
            &mut documents,
            SyncDocumentKind::Presets,
            "presets",
            "presets.toml",
            &path,
        )?;
    }
    if options.include_global_assets
        && let Some(path) = assets_path()
    {
        push_sync_document_if_exists(
            &mut documents,
            SyncDocumentKind::GlobalAssets,
            "global-assets",
            "workspace-assets.toml",
            &path,
        )?;
    }
    if options.include_workspace_configs {
        for root in sync_workspace_roots(&options)? {
            let path = workspace_config_path(&root);
            let id = format!("workspace:{}", root.display());
            push_sync_document_if_exists(
                &mut documents,
                SyncDocumentKind::WorkspaceConfig,
                &id,
                ".terminaltiler/workspace.toml",
                &path,
            )?;
        }
    }
    Ok(SyncSnapshot { documents })
}

pub fn apply_sync_snapshot(
    snapshot: &SyncSnapshot,
    conflict_policy: ConflictPolicy,
) -> io::Result<ApplyReport> {
    let mut report = ApplyReport::default();
    for document in &snapshot.documents {
        let Some(path) = path_for_sync_document(document) else {
            report.skipped += 1;
            continue;
        };
        if path.exists() && conflict_policy == ConflictPolicy::PreferLocal {
            report.skipped += 1;
            continue;
        }
        validate_sync_document(document)?;
        if document.kind == SyncDocumentKind::Preferences {
            apply_preference_projection(&path, &document.contents)?;
        } else {
            write_private(&path, &document.contents)?;
        }
        report.applied += 1;
    }
    Ok(report)
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

fn validate_sync_document(document: &SyncDocument) -> io::Result<()> {
    match document.kind {
        SyncDocumentKind::Preferences => toml::from_str::<PreferenceDocument>(&document.contents)
            .map(|_| ())
            .map_err(toml_error),
        SyncDocumentKind::Presets => toml::from_str::<PresetDocument>(&document.contents)
            .map(|_| ())
            .map_err(toml_error),
        SyncDocumentKind::GlobalAssets => toml::from_str::<AssetDocument>(&document.contents)
            .map(|_| ())
            .map_err(toml_error),
        SyncDocumentKind::WorkspaceConfig => {
            toml::from_str::<WorkspaceConfigDocument>(&document.contents)
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

fn push_sync_document_if_exists(
    documents: &mut Vec<SyncDocument>,
    kind: SyncDocumentKind,
    id: &str,
    logical_path: &str,
    path: &Path,
) -> io::Result<()> {
    match fs::read_to_string(path) {
        Ok(contents) => documents.push(SyncDocument {
            kind,
            id: id.to_string(),
            logical_path: logical_path.to_string(),
            contents,
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

fn push_preference_document_if_exists(
    documents: &mut Vec<SyncDocument>,
    path: &Path,
) -> io::Result<()> {
    match fs::read_to_string(path) {
        Ok(contents) => {
            let document = sync_preference_projection(&contents)?;
            documents.push(SyncDocument {
                kind: SyncDocumentKind::Preferences,
                id: "preferences".into(),
                logical_path: "preferences.toml".into(),
                contents: toml::to_string_pretty(&document).map_err(toml_ser_error)?,
            });
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

fn sync_workspace_roots(options: &SyncSnapshotOptions) -> io::Result<Vec<PathBuf>> {
    let mut roots = BTreeSet::new();
    for root in &options.workspace_roots {
        roots.insert(root.clone());
    }
    if let Some(path) = presets_path()
        && let Ok(document) = read_preset_document(&path)
    {
        for preset in document.presets {
            if let Some(root) = preset.workspace_root {
                roots.insert(root);
            }
        }
    }
    Ok(roots.into_iter().collect())
}

fn path_for_sync_document(document: &SyncDocument) -> Option<PathBuf> {
    match document.kind {
        SyncDocumentKind::Preferences => preferences_path(),
        SyncDocumentKind::Presets => presets_path(),
        SyncDocumentKind::GlobalAssets => assets_path(),
        SyncDocumentKind::WorkspaceConfig => document
            .id
            .strip_prefix("workspace:")
            .map(PathBuf::from)
            .map(|root| workspace_config_path(&root)),
    }
}

fn sync_preference_projection(raw: &str) -> io::Result<PreferenceDocument> {
    let mut document: PreferenceDocument = toml::from_str(raw).map_err(toml_error)?;
    document.version = PREFERENCE_STORE_VERSION;
    Ok(document)
}

fn apply_preference_projection(path: &Path, incoming_raw: &str) -> io::Result<()> {
    let incoming: PreferenceDocument = toml::from_str(incoming_raw).map_err(toml_error)?;
    let merged = match fs::read_to_string(path) {
        Ok(local_raw) => {
            let mut local: toml::Table = local_raw.parse().map_err(toml_error)?;
            let incoming_table: toml::Table = incoming_raw.parse().map_err(toml_error)?;
            for key in [
                "version",
                "default_density",
                "default_theme",
                "close_to_background",
                "default_restore_mode",
                "workspace_fullscreen_shortcut",
                "workspace_density_shortcut",
                "workspace_zoom_in_shortcut",
                "workspace_zoom_out_shortcut",
                "workspace_tile_selection_prefix_shortcut",
                "command_palette_shortcut",
                "max_reconnect_attempts",
            ] {
                if let Some(value) = incoming_table.get(key).cloned() {
                    local.insert(key.to_string(), value);
                }
            }
            if let Some(incoming_voice) = incoming_table
                .get("voice")
                .and_then(|value| value.as_table())
            {
                let mut merged_voice = local
                    .get("voice")
                    .and_then(|value| value.as_table())
                    .cloned()
                    .unwrap_or_default();
                for (key, value) in incoming_voice {
                    merged_voice.insert(key.clone(), value.clone());
                }
                local.insert("voice".into(), toml::Value::Table(merged_voice));
            }
            toml::to_string_pretty(&local).map_err(toml_ser_error)?
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            toml::to_string_pretty(&incoming).map_err(toml_ser_error)?
        }
        Err(error) => return Err(error),
    };
    write_private(path, &merged)
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
    app_paths::config_dir().map(|dir| dir.join("presets.toml"))
}

fn assets_path() -> Option<PathBuf> {
    app_paths::config_dir().map(|dir| dir.join("workspace-assets.toml"))
}

fn preferences_path() -> Option<PathBuf> {
    app_paths::config_dir().map(|dir| dir.join("preferences.toml"))
}

fn workspace_config_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".terminaltiler").join("workspace.toml")
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
    fn sync_snapshot_projects_preferences_without_local_only_fields() {
        with_config_home("sync-prefs", || {
            let config_dir = app_paths::config_dir().unwrap();
            fs::create_dir_all(&config_dir).unwrap();
            fs::write(
                config_dir.join("preferences.toml"),
                r#"
version = 1
default_density = "comfortable"
default_theme = "dark"
settings_dialog_width = 1200
settings_dialog_height = 900
max_reconnect_attempts = 9

[voice]
enabled = true
microphone_id = "local-mic"
hotkey = "<Ctrl><Shift>space"
pack_status = { state = "installed", version = "1" }
engine_mode = "cpu"
"#,
            )
            .unwrap();

            let snapshot = load_sync_snapshot(SyncSnapshotOptions::default()).unwrap();
            let prefs = snapshot
                .documents
                .iter()
                .find(|document| document.kind == SyncDocumentKind::Preferences)
                .unwrap();

            assert!(prefs.contents.contains("default_density"));
            assert!(prefs.contents.contains("max_reconnect_attempts"));
            assert!(!prefs.contents.contains("settings_dialog_width"));
            assert!(!prefs.contents.contains("microphone_id"));
            assert!(!prefs.contents.contains("pack_status"));
        });
    }

    #[test]
    fn applying_preference_projection_preserves_local_only_fields() {
        with_config_home("apply-sync-prefs", || {
            let config_dir = app_paths::config_dir().unwrap();
            fs::create_dir_all(&config_dir).unwrap();
            let prefs_path = config_dir.join("preferences.toml");
            fs::write(
                &prefs_path,
                r#"
version = 1
default_density = "compact"
settings_dialog_width = 777

[voice]
microphone_id = "local-mic"
pack_status = { state = "installed", version = "1" }
"#,
            )
            .unwrap();

            apply_sync_snapshot(
                &SyncSnapshot {
                    documents: vec![SyncDocument {
                        kind: SyncDocumentKind::Preferences,
                        id: "preferences".into(),
                        logical_path: "preferences.toml".into(),
                        contents: r#"
version = 1
default_density = "comfortable"
default_theme = "dark"

[voice]
enabled = true
hotkey = "<Ctrl><Shift>space"
engine_mode = "cpu"
"#
                        .into(),
                    }],
                },
                ConflictPolicy::PreferRemote,
            )
            .unwrap();

            let merged = fs::read_to_string(prefs_path).unwrap();
            assert!(merged.contains("default_density = \"comfortable\""));
            assert!(merged.contains("settings_dialog_width = 777"));
            assert!(merged.contains("microphone_id = \"local-mic\""));
            assert!(merged.contains("pack_status"));
        });
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
