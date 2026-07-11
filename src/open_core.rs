//! Public open-core integration APIs for external applications.
//!
//! This module intentionally contains no external-specific behavior. Private
//! external applications can use it to install packs and synchronize the Core-owned
//! configuration files without depending on Core internals.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::app_paths;
use crate::extension::CompanionRefreshScope;

mod workspace_registry;
pub use workspace_registry::{
    ActiveWorkspaceRegistry, WorkspaceDescriptor, WorkspaceRegistrySnapshot,
    WorkspaceRegistrySnapshotCallback,
};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncExportScope {
    Personal,
    Team,
}

/// Explicit selection of portable global assets.
///
/// The empty default is deliberate: global commands never become syncable as
/// a side effect of adding a new asset class or enabling Sync.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SyncAssetSelection {
    #[serde(default)]
    pub role_ids: BTreeSet<String>,
    #[serde(default)]
    pub runbook_ids: BTreeSet<String>,
    #[serde(default)]
    pub snippet_ids: BTreeSet<String>,
}

/// Additive v2 export selection using opaque workspace descriptors and
/// explicit asset ids. `SyncExportSelection` remains supported for extension
/// API v1 consumers.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SyncExportSelectionV2 {
    pub scope: SyncExportScope,
    pub preferences: bool,
    pub presets: bool,
    #[serde(default)]
    pub assets: SyncAssetSelection,
    #[serde(default)]
    pub workspaces: Vec<WorkspaceDescriptor>,
}

impl SyncExportSelectionV2 {
    pub fn personal(
        preferences: bool,
        presets: bool,
        assets: SyncAssetSelection,
        workspaces: Vec<WorkspaceDescriptor>,
    ) -> Self {
        Self {
            scope: SyncExportScope::Personal,
            preferences,
            presets,
            assets,
            workspaces,
        }
    }
}

/// Typed, caller-owned export selection. It intentionally has no `Default` so
/// adding a new data class cannot silently expand a sync scope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncExportSelection {
    pub scope: SyncExportScope,
    pub preferences: bool,
    pub presets: bool,
    pub portable_global_assets: bool,
    pub workspace_roots: Vec<PathBuf>,
}

impl SyncExportSelection {
    pub fn personal(
        preferences: bool,
        presets: bool,
        portable_global_assets: bool,
        workspace_roots: Vec<PathBuf>,
    ) -> Self {
        Self {
            scope: SyncExportScope::Personal,
            preferences,
            presets,
            portable_global_assets,
            workspace_roots,
        }
    }

    pub fn team_portable(workspace_roots: Vec<PathBuf>) -> Self {
        Self {
            scope: SyncExportScope::Team,
            preferences: false,
            presets: true,
            portable_global_assets: false,
            workspace_roots,
        }
    }
}

/// Caller-approved local destinations for applying a sync snapshot.
///
/// Workspace document ids are opaque remote identifiers. They are skipped unless
/// the caller explicitly maps the id to an existing local workspace directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncApplyOptions {
    pub conflict_policy: ConflictPolicy,
    pub workspace_destinations: BTreeMap<String, PathBuf>,
}

impl Default for SyncApplyOptions {
    fn default() -> Self {
        Self {
            conflict_policy: ConflictPolicy::PreferRemote,
            workspace_destinations: BTreeMap::new(),
        }
    }
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

/// A versioned remote deletion. The id has the same opaque, stable meaning as
/// `SyncDocument::id`; callers must still provide an approved workspace map.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SyncTombstone {
    pub kind: SyncDocumentKind,
    pub id: String,
    pub version: i32,
    pub device_id: String,
    pub deleted_at: String,
}

/// One preflighted filesystem transaction containing remote documents and
/// deletions. Callers should prefer this over applying snapshots and
/// tombstones separately.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SyncApplyBatch {
    #[serde(default)]
    pub documents: Vec<SyncDocument>,
    #[serde(default)]
    pub tombstones: Vec<SyncTombstone>,
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
    pub errors: usize,
    pub documents: Vec<SyncDocumentApplyResult>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyncDocumentApplyOutcome {
    Applied,
    Skipped,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncDocumentApplyResult {
    pub id: String,
    pub outcome: SyncDocumentApplyOutcome,
    pub message: Option<String>,
}

/// Structured result suitable for background controllers and platform UI
/// dispatch. `report` retains the extension API v1 counters.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SyncApplyResult {
    pub report: ApplyReport,
    pub affected_scopes: Vec<CompanionRefreshScope>,
    pub errors: Vec<String>,
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
    load_sync_snapshot_selected(SyncExportSelection::personal(
        options.include_preferences,
        options.include_presets,
        options.include_global_assets,
        if options.include_workspace_configs {
            sync_workspace_roots(&options)?
        } else {
            Vec::new()
        },
    ))
}

pub fn load_sync_snapshot_selected(selection: SyncExportSelection) -> io::Result<SyncSnapshot> {
    let mut documents = Vec::new();
    if selection.preferences
        && let Some(path) = preferences_path()
    {
        push_preference_document_if_exists(&mut documents, &path)?;
    }
    if selection.presets
        && let Some(path) = presets_path()
    {
        push_portable_preset_document_if_exists(&mut documents, &path)?;
    }
    if selection.portable_global_assets
        && let Some(path) = assets_path()
    {
        push_portable_asset_document_if_exists(&mut documents, &path)?;
    }
    for root in selection.workspace_roots {
        let path = workspace_config_path(&root);
        let id = format!("workspace:{}", root.display());
        push_portable_workspace_document_if_exists(&mut documents, &id, &path)?;
    }
    Ok(SyncSnapshot { documents })
}

/// Builds a snapshot using explicit global-asset ids and opaque workspace
/// identities. Unknown ids are ignored rather than broadening the export.
pub fn load_sync_snapshot_selected_v2(
    selection: SyncExportSelectionV2,
) -> io::Result<SyncSnapshot> {
    let mut documents = Vec::new();
    if selection.preferences
        && let Some(path) = preferences_path()
    {
        push_preference_document_if_exists(&mut documents, &path)?;
    }
    if selection.presets
        && let Some(path) = presets_path()
    {
        push_portable_preset_document_if_exists(&mut documents, &path)?;
    }
    if (!selection.assets.role_ids.is_empty()
        || !selection.assets.runbook_ids.is_empty()
        || !selection.assets.snippet_ids.is_empty())
        && let Some(path) = assets_path()
    {
        push_selected_asset_document_if_exists(&mut documents, &path, &selection.assets)?;
    }

    let mut workspace_ids = BTreeSet::new();
    for workspace in selection.workspaces {
        if !workspace_ids.insert(workspace.id.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("duplicate workspace descriptor id '{}'", workspace.id),
            ));
        }
        validate_opaque_workspace_id(&workspace.id)?;
        let path = workspace_config_path(&workspace.root);
        push_selected_workspace_document_if_exists(
            &mut documents,
            &workspace.id,
            &path,
            &selection.assets,
        )?;
    }
    Ok(SyncSnapshot { documents })
}

pub fn apply_sync_snapshot(
    snapshot: &SyncSnapshot,
    conflict_policy: ConflictPolicy,
) -> io::Result<ApplyReport> {
    apply_sync_snapshot_with_options(
        snapshot,
        &SyncApplyOptions {
            conflict_policy,
            ..SyncApplyOptions::default()
        },
    )
}

pub fn apply_sync_snapshot_with_options(
    snapshot: &SyncSnapshot,
    options: &SyncApplyOptions,
) -> io::Result<ApplyReport> {
    apply_sync_batch_with_options(
        &SyncApplyBatch {
            documents: snapshot.documents.clone(),
            tombstones: Vec::new(),
        },
        options,
    )
    .map(|result| result.report)
}

/// Applies documents and tombstones under one preflight and one shared
/// persistence lock. No target changes when any later operation is invalid.
pub fn apply_sync_batch_with_options(
    batch: &SyncApplyBatch,
    options: &SyncApplyOptions,
) -> io::Result<SyncApplyResult> {
    let mut prepared_documents = Vec::with_capacity(batch.documents.len());
    let mut prepared_tombstones = Vec::with_capacity(batch.tombstones.len());
    let mut seen_ids = BTreeMap::new();
    let mut seen_paths = BTreeSet::new();
    let mut result = SyncApplyResult::default();

    for document in &batch.documents {
        if let Some(existing_kind) = seen_ids.insert(document.id.clone(), document.kind.clone()) {
            push_preflight_error(
                &mut result,
                &document.id,
                format!("duplicate sync route id (already used by {existing_kind:?})"),
            );
            continue;
        }
        if let Err(error) = validate_logical_path(document)
            .and_then(|_| validate_sync_document_identity(document))
            .and_then(|_| validate_sync_document(document))
        {
            push_preflight_error(&mut result, &document.id, error.to_string());
            continue;
        }
        let path = match path_for_sync_document(document, &options.workspace_destinations) {
            Ok(path) => path,
            Err(error) => {
                push_preflight_error(&mut result, &document.id, error.to_string());
                continue;
            }
        };
        if let Some(path) = &path
            && !seen_paths.insert(path.clone())
        {
            push_preflight_error(
                &mut result,
                &document.id,
                "multiple sync routes resolve to the same destination",
            );
            continue;
        }
        prepared_documents.push((document.clone(), path));
    }

    for tombstone in &batch.tombstones {
        if let Some(existing_kind) = seen_ids.insert(tombstone.id.clone(), tombstone.kind.clone()) {
            push_preflight_error(
                &mut result,
                &tombstone.id,
                format!("duplicate sync route id (already used by {existing_kind:?})"),
            );
            continue;
        }
        if let Err(error) = validate_sync_tombstone(tombstone) {
            push_preflight_error(&mut result, &tombstone.id, error.to_string());
            continue;
        }
        let route = SyncDocument {
            kind: tombstone.kind.clone(),
            id: tombstone.id.clone(),
            logical_path: logical_path_for_kind(&tombstone.kind).into(),
            contents: String::new(),
        };
        let path = match path_for_sync_document(&route, &options.workspace_destinations) {
            Ok(path) => path,
            Err(error) => {
                push_preflight_error(&mut result, &tombstone.id, error.to_string());
                continue;
            }
        };
        if let Some(path) = &path
            && !seen_paths.insert(path.clone())
        {
            push_preflight_error(
                &mut result,
                &tombstone.id,
                "multiple sync routes resolve to the same destination",
            );
            continue;
        }
        prepared_tombstones.push((tombstone.clone(), path));
    }

    if result.report.errors > 0 {
        return Ok(result);
    }

    crate::storage::fs_utils::with_persistence_lock(|| {
        let mut writes = Vec::new();
        let mut removals = Vec::new();
        for (document, path) in prepared_documents {
            let Some(path) = path else {
                push_skipped(
                    &mut result.report,
                    document.id,
                    "no approved local destination",
                );
                continue;
            };
            if path.exists() && options.conflict_policy == ConflictPolicy::PreferLocal {
                push_skipped(
                    &mut result.report,
                    document.id,
                    "local destination was preferred",
                );
                continue;
            }
            let contents = if document.kind == SyncDocumentKind::Preferences {
                preference_projection_contents(&path, &document.contents)?
            } else {
                document.contents
            };
            push_applied(&mut result.report, document.id);
            push_refresh_scope(&mut result.affected_scopes, scope_for_kind(&document.kind));
            writes.push((path, contents));
        }
        for (tombstone, path) in prepared_tombstones {
            let Some(path) = path else {
                push_skipped(
                    &mut result.report,
                    tombstone.id,
                    "no approved local destination",
                );
                continue;
            };
            if !path.exists() {
                push_skipped(
                    &mut result.report,
                    tombstone.id,
                    "local destination is already absent",
                );
                continue;
            }
            push_applied(&mut result.report, tombstone.id);
            push_refresh_scope(&mut result.affected_scopes, scope_for_kind(&tombstone.kind));
            removals.push(path);
        }
        crate::storage::fs_utils::transactional_apply_private_unlocked(&writes, &removals)?;
        result.report.applied = writes.len() + removals.len();
        Ok(())
    })?;
    Ok(result)
}

/// Applies remote deletions using the same destination authority and shared
/// persistence lock as snapshot writes. New callers should submit a
/// `SyncApplyBatch` so writes and deletions cannot be split across transactions.
pub fn apply_sync_tombstones_with_options(
    tombstones: &[SyncTombstone],
    options: &SyncApplyOptions,
) -> io::Result<ApplyReport> {
    apply_sync_batch_with_options(
        &SyncApplyBatch {
            documents: Vec::new(),
            tombstones: tombstones.to_vec(),
        },
        options,
    )
    .map(|result| result.report)
}

fn push_preflight_error(result: &mut SyncApplyResult, id: &str, message: impl Into<String>) {
    let message = message.into();
    result.report.errors += 1;
    result.report.documents.push(SyncDocumentApplyResult {
        id: id.to_string(),
        outcome: SyncDocumentApplyOutcome::Error,
        message: Some(message.clone()),
    });
    result.errors.push(format!("{id}: {message}"));
}

fn push_skipped(report: &mut ApplyReport, id: String, message: impl Into<String>) {
    report.skipped += 1;
    report.documents.push(SyncDocumentApplyResult {
        id,
        outcome: SyncDocumentApplyOutcome::Skipped,
        message: Some(message.into()),
    });
}

fn push_applied(report: &mut ApplyReport, id: String) {
    report.documents.push(SyncDocumentApplyResult {
        id,
        outcome: SyncDocumentApplyOutcome::Applied,
        message: None,
    });
}

fn push_refresh_scope(scopes: &mut Vec<CompanionRefreshScope>, scope: CompanionRefreshScope) {
    if !scopes.contains(&scope) {
        scopes.push(scope);
    }
}

fn scope_for_kind(kind: &SyncDocumentKind) -> CompanionRefreshScope {
    match kind {
        SyncDocumentKind::Preferences => CompanionRefreshScope::Preferences,
        SyncDocumentKind::Presets => CompanionRefreshScope::Presets,
        SyncDocumentKind::GlobalAssets => CompanionRefreshScope::Assets,
        SyncDocumentKind::WorkspaceConfig => CompanionRefreshScope::WorkspaceConfigs,
    }
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
        SyncDocumentKind::Preferences => {
            let parsed: PreferenceDocument =
                toml::from_str(&document.contents).map_err(toml_error)?;
            validate_schema_version(parsed.version, PREFERENCE_STORE_VERSION, "preferences")
        }
        SyncDocumentKind::Presets => {
            let parsed: PresetDocument = toml::from_str(&document.contents).map_err(toml_error)?;
            validate_schema_version(parsed.version, PRESET_STORE_VERSION, "presets")?;
            validate_presets(&parsed.presets)
        }
        SyncDocumentKind::GlobalAssets => {
            let parsed: AssetDocument = toml::from_str(&document.contents).map_err(toml_error)?;
            validate_schema_version(parsed.version, ASSET_STORE_VERSION, "global assets")?;
            validate_assets(&parsed.into_assets())
        }
        SyncDocumentKind::WorkspaceConfig => {
            let parsed: WorkspaceConfigDocument =
                toml::from_str(&document.contents).map_err(toml_error)?;
            validate_schema_version(parsed.version, ASSET_STORE_VERSION, "workspace config")?;
            validate_assets(&parsed.assets)
        }
    }
}

fn validate_sync_document_identity(document: &SyncDocument) -> io::Result<()> {
    if document.id.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "sync document id cannot be empty",
        ));
    }
    if document.logical_path != logical_path_for_kind(&document.kind) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "sync document kind {:?} does not match logical path '{}'",
                document.kind, document.logical_path
            ),
        ));
    }
    Ok(())
}

fn validate_sync_tombstone(tombstone: &SyncTombstone) -> io::Result<()> {
    if tombstone.id.trim().is_empty()
        || tombstone.device_id.trim().is_empty()
        || tombstone.deleted_at.trim().is_empty()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "sync tombstone identity, device, and deletion time are required",
        ));
    }
    if tombstone.version <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "sync tombstone version must be positive",
        ));
    }
    Ok(())
}

fn logical_path_for_kind(kind: &SyncDocumentKind) -> &'static str {
    match kind {
        SyncDocumentKind::Preferences => "preferences.toml",
        SyncDocumentKind::Presets => "presets.toml",
        SyncDocumentKind::GlobalAssets => "workspace-assets.toml",
        SyncDocumentKind::WorkspaceConfig => ".terminaltiler/workspace.toml",
    }
}

fn validate_schema_version(actual: u32, expected: u32, label: &str) -> io::Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported {label} schema version {actual}; expected {expected}"),
        ))
    }
}

fn validate_presets(presets: &[WorkspacePreset]) -> io::Result<()> {
    validate_unique_ids(presets.iter().map(|preset| preset.id.as_str()), "preset")?;
    for preset in presets {
        validate_unique_ids(
            preset
                .layout
                .tile_specs()
                .iter()
                .map(|tile| tile.id.as_str()),
            "preset tile",
        )?;
    }
    Ok(())
}

fn validate_assets(assets: &WorkspaceAssets) -> io::Result<()> {
    validate_unique_ids(
        assets
            .connection_profiles
            .iter()
            .map(|item| item.id.as_str()),
        "connection profile",
    )?;
    validate_unique_ids(
        assets.inventory_hosts.iter().map(|item| item.id.as_str()),
        "inventory host",
    )?;
    validate_unique_ids(
        assets.inventory_groups.iter().map(|item| item.id.as_str()),
        "inventory group",
    )?;
    validate_unique_ids(
        assets.role_templates.iter().map(|item| item.id.as_str()),
        "role template",
    )?;
    validate_unique_ids(
        assets.runbooks.iter().map(|item| item.id.as_str()),
        "runbook",
    )?;
    validate_unique_ids(
        assets.snippets.iter().map(|item| item.id.as_str()),
        "snippet",
    )?;

    let host_ids = assets
        .inventory_hosts
        .iter()
        .map(|item| item.id.as_str())
        .collect::<BTreeSet<_>>();
    let group_ids = assets
        .inventory_groups
        .iter()
        .map(|item| item.id.as_str())
        .collect::<BTreeSet<_>>();
    let connection_ids = assets
        .connection_profiles
        .iter()
        .map(|item| item.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut role_ids = assets
        .role_templates
        .iter()
        .map(|item| item.id.clone())
        .collect::<BTreeSet<_>>();
    for builtin in crate::model::assets::builtin_role_templates() {
        role_ids.insert(builtin.id);
    }

    for profile in &assets.connection_profiles {
        if let Some(host_id) = profile.inventory_host_id.as_deref()
            && !host_ids.contains(host_id)
        {
            return invalid_reference("connection profile", &profile.id, "inventory host", host_id);
        }
    }
    for host in &assets.inventory_hosts {
        for group_id in &host.group_ids {
            if !group_ids.contains(group_id.as_str()) {
                return invalid_reference("inventory host", &host.id, "inventory group", group_id);
            }
        }
    }
    for role in &assets.role_templates {
        if let Some(connection_id) = role.default_connection_profile_id.as_deref()
            && !connection_ids.contains(connection_id)
        {
            return invalid_reference(
                "role template",
                &role.id,
                "connection profile",
                connection_id,
            );
        }
    }
    for runbook in &assets.runbooks {
        validate_unique_ids(
            runbook
                .variables
                .iter()
                .map(|variable| variable.id.as_str()),
            "runbook variable",
        )?;
        validate_unique_ids(
            runbook.steps.iter().map(|step| step.id.as_str()),
            "runbook step",
        )?;
        match &runbook.target {
            crate::model::assets::RunbookTarget::Role(role_id) if !role_ids.contains(role_id) => {
                return invalid_reference("runbook", &runbook.id, "role template", role_id);
            }
            crate::model::assets::RunbookTarget::ConnectionProfile(connection_id)
                if !connection_ids.contains(connection_id.as_str()) =>
            {
                return invalid_reference(
                    "runbook",
                    &runbook.id,
                    "connection profile",
                    connection_id,
                );
            }
            _ => {}
        }
    }
    for snippet in &assets.snippets {
        validate_unique_ids(
            snippet
                .variables
                .iter()
                .map(|variable| variable.id.as_str()),
            "snippet variable",
        )?;
    }
    Ok(())
}

fn validate_unique_ids<'a>(ids: impl IntoIterator<Item = &'a str>, label: &str) -> io::Result<()> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if id.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{label} id cannot be empty"),
            ));
        }
        if !seen.insert(id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("duplicate {label} id '{id}'"),
            ));
        }
    }
    Ok(())
}

fn invalid_reference(
    source_kind: &str,
    source_id: &str,
    target_kind: &str,
    target_id: &str,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{source_kind} '{source_id}' references missing {target_kind} '{target_id}'"),
    ))
}

fn validate_logical_path(document: &SyncDocument) -> io::Result<()> {
    let path = Path::new(&document.logical_path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "sync logical path must be relative and cannot traverse parents",
        ));
    }
    Ok(())
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

fn push_portable_preset_document_if_exists(
    documents: &mut Vec<SyncDocument>,
    path: &Path,
) -> io::Result<()> {
    match fs::read_to_string(path) {
        Ok(raw) => {
            let mut document: PresetDocument = toml::from_str(&raw).map_err(toml_error)?;
            document.version = PRESET_STORE_VERSION;
            for preset in &mut document.presets {
                preset.workspace_root = None;
            }
            documents.push(SyncDocument {
                kind: SyncDocumentKind::Presets,
                id: "presets".into(),
                logical_path: "presets.toml".into(),
                contents: toml::to_string_pretty(&document).map_err(toml_ser_error)?,
            });
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

fn portable_assets(mut assets: WorkspaceAssets) -> WorkspaceAssets {
    assets.connection_profiles.clear();
    assets.inventory_hosts.clear();
    for role in &mut assets.role_templates {
        role.default_connection_profile_id = None;
    }
    assets
}

fn selected_portable_assets(
    mut assets: WorkspaceAssets,
    selection: &SyncAssetSelection,
) -> WorkspaceAssets {
    assets.connection_profiles.clear();
    assets.inventory_hosts.clear();
    assets.inventory_groups.clear();
    assets
        .role_templates
        .retain(|role| selection.role_ids.contains(&role.id));
    for role in &mut assets.role_templates {
        role.default_connection_profile_id = None;
    }
    assets
        .runbooks
        .retain(|runbook| selection.runbook_ids.contains(&runbook.id));
    for runbook in &mut assets.runbooks {
        match &runbook.target {
            crate::model::assets::RunbookTarget::ConnectionProfile(_) => {
                runbook.target = crate::model::assets::RunbookTarget::AllPanes;
            }
            crate::model::assets::RunbookTarget::Role(role_id)
                if !selection.role_ids.contains(role_id) =>
            {
                runbook.target = crate::model::assets::RunbookTarget::AllPanes;
            }
            _ => {}
        }
    }
    assets
        .snippets
        .retain(|snippet| selection.snippet_ids.contains(&snippet.id));
    assets
}

fn push_portable_asset_document_if_exists(
    documents: &mut Vec<SyncDocument>,
    path: &Path,
) -> io::Result<()> {
    match fs::read_to_string(path) {
        Ok(raw) => {
            let document: AssetDocument = toml::from_str(&raw).map_err(toml_error)?;
            let document = AssetDocument::from_assets(portable_assets(document.into_assets()));
            documents.push(SyncDocument {
                kind: SyncDocumentKind::GlobalAssets,
                id: "global-assets".into(),
                logical_path: "workspace-assets.toml".into(),
                contents: toml::to_string_pretty(&document).map_err(toml_ser_error)?,
            });
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

fn push_selected_asset_document_if_exists(
    documents: &mut Vec<SyncDocument>,
    path: &Path,
    selection: &SyncAssetSelection,
) -> io::Result<()> {
    match fs::read_to_string(path) {
        Ok(raw) => {
            let document: AssetDocument = toml::from_str(&raw).map_err(toml_error)?;
            let document = AssetDocument::from_assets(selected_portable_assets(
                document.into_assets(),
                selection,
            ));
            documents.push(SyncDocument {
                kind: SyncDocumentKind::GlobalAssets,
                id: "global-assets".into(),
                logical_path: "workspace-assets.toml".into(),
                contents: toml::to_string_pretty(&document).map_err(toml_ser_error)?,
            });
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

fn push_portable_workspace_document_if_exists(
    documents: &mut Vec<SyncDocument>,
    id: &str,
    path: &Path,
) -> io::Result<()> {
    match fs::read_to_string(path) {
        Ok(raw) => {
            let mut document: WorkspaceConfigDocument = toml::from_str(&raw).map_err(toml_error)?;
            document.version = ASSET_STORE_VERSION;
            document.assets = portable_assets(document.assets);
            documents.push(SyncDocument {
                kind: SyncDocumentKind::WorkspaceConfig,
                id: id.into(),
                logical_path: ".terminaltiler/workspace.toml".into(),
                contents: toml::to_string_pretty(&document).map_err(toml_ser_error)?,
            });
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

fn push_selected_workspace_document_if_exists(
    documents: &mut Vec<SyncDocument>,
    id: &str,
    path: &Path,
    selection: &SyncAssetSelection,
) -> io::Result<()> {
    match fs::read_to_string(path) {
        Ok(raw) => {
            let mut document: WorkspaceConfigDocument = toml::from_str(&raw).map_err(toml_error)?;
            document.version = ASSET_STORE_VERSION;
            document.assets = selected_portable_assets(document.assets, selection);
            documents.push(SyncDocument {
                kind: SyncDocumentKind::WorkspaceConfig,
                id: id.into(),
                logical_path: ".terminaltiler/workspace.toml".into(),
                contents: toml::to_string_pretty(&document).map_err(toml_ser_error)?,
            });
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

fn validate_opaque_workspace_id(id: &str) -> io::Result<()> {
    let value = id.strip_prefix("workspace:").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "workspace id must use workspace:<uuid>",
        )
    })?;
    uuid::Uuid::parse_str(value).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "workspace id must contain a valid UUID",
        )
    })?;
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

fn path_for_sync_document(
    document: &SyncDocument,
    workspace_destinations: &BTreeMap<String, PathBuf>,
) -> io::Result<Option<PathBuf>> {
    match document.kind {
        SyncDocumentKind::Preferences => Ok(preferences_path()),
        SyncDocumentKind::Presets => Ok(presets_path()),
        SyncDocumentKind::GlobalAssets => Ok(assets_path()),
        SyncDocumentKind::WorkspaceConfig => workspace_destinations
            .get(&document.id)
            .map(|root| approved_workspace_config_path(root))
            .transpose(),
    }
}

fn approved_workspace_config_path(root: &Path) -> io::Result<PathBuf> {
    let root = root.canonicalize()?;
    if !root.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "approved workspace root '{}' is not a directory",
                root.display()
            ),
        ));
    }

    let config_dir = root.join(".terminaltiler");
    if config_dir.exists() {
        let canonical_config_dir = config_dir.canonicalize()?;
        if !canonical_config_dir.starts_with(&root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "workspace config directory escapes the approved root",
            ));
        }
        return Ok(canonical_config_dir.join("workspace.toml"));
    }
    Ok(config_dir.join("workspace.toml"))
}

fn sync_preference_projection(raw: &str) -> io::Result<PreferenceDocument> {
    let mut document: PreferenceDocument = toml::from_str(raw).map_err(toml_error)?;
    document.version = PREFERENCE_STORE_VERSION;
    Ok(document)
}

fn preference_projection_contents(path: &Path, incoming_raw: &str) -> io::Result<String> {
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
    Ok(merged)
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
    use std::{
        sync::{Mutex, OnceLock, mpsc},
        thread,
        time::Duration,
    };
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
        let previous = std::env::var_os(crate::app_paths::PROFILE_ROOT_ENV);
        unsafe {
            std::env::set_var(crate::app_paths::PROFILE_ROOT_ENV, &dir);
        }
        let result = test();
        unsafe {
            match previous {
                Some(value) => std::env::set_var(crate::app_paths::PROFILE_ROOT_ENV, value),
                None => std::env::remove_var(crate::app_paths::PROFILE_ROOT_ENV),
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
    fn typed_export_selection_strips_machine_and_secret_bearing_fields() {
        with_config_home("portable-selection", || {
            let config_dir = app_paths::config_dir().unwrap();
            fs::create_dir_all(&config_dir).unwrap();
            let machine_root = temp_config_home("machine-root");
            let mut preset = sample_preset("portable-preset");
            preset.workspace_root = Some(machine_root.clone());
            fs::write(
                config_dir.join("presets.toml"),
                toml::to_string_pretty(&PresetDocument {
                    version: 1,
                    presets: vec![preset],
                })
                .unwrap(),
            )
            .unwrap();
            fs::write(
                config_dir.join("workspace-assets.toml"),
                r#"
version = 1

[[connection_profiles]]
id = "production"
name = "Production"
kind = "ssh"
inventory_host_id = "prod-host"

[[inventory_hosts]]
id = "prod-host"
name = "Production host"
host = "10.20.30.40"
user = "deploy"
password_secret_ref = "keyring:production-password"
ssh_key_path = "/home/alice/.ssh/id_ed25519"

[[role_templates]]
id = "operator"
name = "Operator"
default_connection_profile_id = "production"
"#,
            )
            .unwrap();

            let personal = load_sync_snapshot_selected(SyncExportSelection::personal(
                false,
                true,
                true,
                Vec::new(),
            ))
            .unwrap();
            let serialized = personal
                .documents
                .iter()
                .map(|document| document.contents.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(!serialized.contains(machine_root.to_string_lossy().as_ref()));
            assert!(!serialized.contains("10.20.30.40"));
            assert!(!serialized.contains("keyring:production-password"));
            assert!(!serialized.contains("/home/alice/.ssh/id_ed25519"));
            assert!(!serialized.contains("production-password"));

            let team = load_sync_snapshot_selected(SyncExportSelection::team_portable(Vec::new()))
                .unwrap();
            assert!(
                team.documents
                    .iter()
                    .any(|document| document.kind == SyncDocumentKind::Presets)
            );
            assert!(!team.documents.iter().any(|document| matches!(
                document.kind,
                SyncDocumentKind::Preferences | SyncDocumentKind::GlobalAssets
            )));
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
    fn workspace_sync_requires_an_explicit_destination_mapping() {
        with_config_home("workspace-destination", || {
            let workspace_root = temp_config_home("approved-workspace");
            let document = SyncDocument {
                kind: SyncDocumentKind::WorkspaceConfig,
                id: "opaque-workspace-id".into(),
                logical_path: ".terminaltiler/workspace.toml".into(),
                contents: "version = 1\n".into(),
            };
            let snapshot = SyncSnapshot {
                documents: vec![document.clone()],
            };

            let skipped = apply_sync_snapshot(&snapshot, ConflictPolicy::PreferRemote).unwrap();
            assert_eq!(skipped.applied, 0);
            assert_eq!(skipped.skipped, 1);
            assert!(!workspace_config_path(&workspace_root).exists());

            let applied = apply_sync_snapshot_with_options(
                &snapshot,
                &SyncApplyOptions {
                    conflict_policy: ConflictPolicy::PreferRemote,
                    workspace_destinations: BTreeMap::from([(document.id, workspace_root.clone())]),
                },
            )
            .unwrap();
            assert_eq!(applied.applied, 1);
            assert_eq!(
                fs::read_to_string(workspace_config_path(&workspace_root)).unwrap(),
                "version = 1\n"
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn workspace_sync_rejects_config_directory_symlink_escape() {
        use std::os::unix::fs::symlink;

        with_config_home("workspace-symlink", || {
            let workspace_root = temp_config_home("symlink-workspace");
            let outside = temp_config_home("symlink-outside");
            symlink(&outside, workspace_root.join(".terminaltiler")).unwrap();
            let document = SyncDocument {
                kind: SyncDocumentKind::WorkspaceConfig,
                id: "opaque-workspace-id".into(),
                logical_path: ".terminaltiler/workspace.toml".into(),
                contents: "version = 1\n".into(),
            };

            let report = apply_sync_snapshot_with_options(
                &SyncSnapshot {
                    documents: vec![document.clone()],
                },
                &SyncApplyOptions {
                    conflict_policy: ConflictPolicy::PreferRemote,
                    workspace_destinations: BTreeMap::from([(document.id, workspace_root)]),
                },
            )
            .unwrap();

            assert_eq!(report.errors, 1);
            assert_eq!(report.documents[0].outcome, SyncDocumentApplyOutcome::Error);
            assert!(!outside.join("workspace.toml").exists());
        });
    }

    #[test]
    fn workspace_sync_rejects_traversal_even_with_an_approved_root() {
        with_config_home("workspace-traversal", || {
            let workspace_root = temp_config_home("traversal-workspace");
            let outside = workspace_root.parent().unwrap().join("escaped.toml");
            let document = SyncDocument {
                kind: SyncDocumentKind::WorkspaceConfig,
                id: "opaque-workspace-id".into(),
                logical_path: "../../escaped.toml".into(),
                contents: "version = 1\n".into(),
            };
            let report = apply_sync_snapshot_with_options(
                &SyncSnapshot {
                    documents: vec![document.clone()],
                },
                &SyncApplyOptions {
                    conflict_policy: ConflictPolicy::PreferRemote,
                    workspace_destinations: BTreeMap::from([(document.id, workspace_root)]),
                },
            )
            .unwrap();

            assert_eq!(report.errors, 1);
            assert!(!outside.exists());
        });
    }

    #[test]
    fn sync_batch_is_validated_before_any_document_is_written() {
        with_config_home("sync-preflight", || {
            let config_dir = app_paths::config_dir().unwrap();
            let prefs_path = config_dir.join("preferences.toml");
            let snapshot = SyncSnapshot {
                documents: vec![
                    SyncDocument {
                        kind: SyncDocumentKind::Preferences,
                        id: "preferences".into(),
                        logical_path: "preferences.toml".into(),
                        contents: "version = 1\ndefault_density = \"compact\"\n".into(),
                    },
                    SyncDocument {
                        kind: SyncDocumentKind::Presets,
                        id: "presets".into(),
                        logical_path: "presets.toml".into(),
                        contents: "not valid toml = [".into(),
                    },
                ],
            };

            let report = apply_sync_snapshot(&snapshot, ConflictPolicy::PreferRemote).unwrap();
            assert_eq!(report.errors, 1);
            assert!(!prefs_path.exists());
        });
    }

    #[test]
    fn duplicate_document_ids_fail_preflight_without_writes() {
        with_config_home("sync-duplicate", || {
            let config_dir = app_paths::config_dir().unwrap();
            let snapshot = SyncSnapshot {
                documents: vec![
                    SyncDocument {
                        kind: SyncDocumentKind::Preferences,
                        id: "same".into(),
                        logical_path: "preferences.toml".into(),
                        contents: "version = 1\n".into(),
                    },
                    SyncDocument {
                        kind: SyncDocumentKind::Presets,
                        id: "same".into(),
                        logical_path: "presets.toml".into(),
                        contents: "version = 1\npresets = []\n".into(),
                    },
                ],
            };

            let report = apply_sync_snapshot(&snapshot, ConflictPolicy::PreferRemote).unwrap();
            assert_eq!(report.errors, 1);
            assert!(!config_dir.join("preferences.toml").exists());
            assert!(!config_dir.join("presets.toml").exists());
        });
    }

    #[test]
    fn failed_later_commit_rolls_back_earlier_document() {
        with_config_home("sync-rollback", || {
            let config_dir = app_paths::config_dir().unwrap();
            fs::create_dir_all(&config_dir).unwrap();
            let preferences = config_dir.join("preferences.toml");
            let presets = config_dir.join("presets.toml");
            let original = "version = 1\ndefault_density = \"compact\"\n";
            fs::write(&preferences, original).unwrap();
            fs::create_dir(&presets).unwrap();
            let snapshot = SyncSnapshot {
                documents: vec![
                    SyncDocument {
                        kind: SyncDocumentKind::Preferences,
                        id: "preferences".into(),
                        logical_path: "preferences.toml".into(),
                        contents: "version = 1\ndefault_density = \"comfortable\"\n".into(),
                    },
                    SyncDocument {
                        kind: SyncDocumentKind::Presets,
                        id: "presets".into(),
                        logical_path: "presets.toml".into(),
                        contents: "version = 1\npresets = []\n".into(),
                    },
                ],
            };

            assert!(apply_sync_snapshot(&snapshot, ConflictPolicy::PreferRemote).is_err());
            assert_eq!(fs::read_to_string(preferences).unwrap(), original);
            assert!(presets.is_dir());
        });
    }

    #[test]
    fn sync_apply_waits_for_the_shared_persistence_lock() {
        with_config_home("sync-lock", || {
            let (locked_tx, locked_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let holder = thread::spawn(move || {
                crate::storage::fs_utils::with_persistence_lock(|| {
                    locked_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(())
                })
                .unwrap();
            });
            locked_rx.recv().unwrap();

            let (done_tx, done_rx) = mpsc::channel();
            let applier = thread::spawn(move || {
                let result = apply_sync_snapshot(
                    &SyncSnapshot {
                        documents: vec![SyncDocument {
                            kind: SyncDocumentKind::Preferences,
                            id: "preferences".into(),
                            logical_path: "preferences.toml".into(),
                            contents: "version = 1\n".into(),
                        }],
                    },
                    ConflictPolicy::PreferRemote,
                );
                done_tx.send(result).unwrap();
            });
            assert!(done_rx.recv_timeout(Duration::from_millis(50)).is_err());
            release_tx.send(()).unwrap();
            assert_eq!(
                done_rx
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
                    .unwrap()
                    .applied,
                1
            );
            holder.join().unwrap();
            applier.join().unwrap();
        });
    }

    #[test]
    fn tombstone_delete_then_remote_restore_recreates_document() {
        with_config_home("sync-tombstone", || {
            let config_dir = app_paths::config_dir().unwrap();
            fs::create_dir_all(&config_dir).unwrap();
            let presets = config_dir.join("presets.toml");
            fs::write(&presets, "version = 1\npresets = []\n").unwrap();
            let tombstone = SyncTombstone {
                kind: SyncDocumentKind::Presets,
                id: "presets".into(),
                version: 2,
                device_id: "device".into(),
                deleted_at: "2026-07-09T00:00:00Z".into(),
            };

            let deleted =
                apply_sync_tombstones_with_options(&[tombstone], &SyncApplyOptions::default())
                    .unwrap();
            assert_eq!(deleted.applied, 1);
            assert!(!presets.exists());

            let restored = apply_sync_snapshot(
                &SyncSnapshot {
                    documents: vec![SyncDocument {
                        kind: SyncDocumentKind::Presets,
                        id: "presets".into(),
                        logical_path: "presets.toml".into(),
                        contents: "version = 1\npresets = []\n".into(),
                    }],
                },
                ConflictPolicy::PreferRemote,
            )
            .unwrap();
            assert_eq!(restored.applied, 1);
            assert!(presets.exists());
        });
    }

    #[test]
    fn mixed_batch_preflight_failure_prevents_all_writes_and_deletions() {
        with_config_home("sync-mixed-preflight", || {
            let config_dir = app_paths::config_dir().unwrap();
            fs::create_dir_all(&config_dir).unwrap();
            let preferences = config_dir.join("preferences.toml");
            let presets = config_dir.join("presets.toml");
            fs::write(&presets, "version = 1\npresets = []\n").unwrap();

            let result = apply_sync_batch_with_options(
                &SyncApplyBatch {
                    documents: vec![SyncDocument {
                        kind: SyncDocumentKind::Preferences,
                        id: "preferences".into(),
                        logical_path: "preferences.toml".into(),
                        contents: "version = 1\ndefault_density = \"comfortable\"\n".into(),
                    }],
                    tombstones: vec![SyncTombstone {
                        kind: SyncDocumentKind::Presets,
                        id: "presets".into(),
                        version: 0,
                        device_id: "device".into(),
                        deleted_at: "2026-07-10T00:00:00Z".into(),
                    }],
                },
                &SyncApplyOptions::default(),
            )
            .unwrap();

            assert_eq!(result.report.errors, 1);
            assert!(!preferences.exists());
            assert!(presets.exists());
        });
    }

    #[test]
    fn mixed_batch_commits_documents_and_tombstones_with_refresh_scopes() {
        with_config_home("sync-mixed-commit", || {
            let config_dir = app_paths::config_dir().unwrap();
            fs::create_dir_all(&config_dir).unwrap();
            let preferences = config_dir.join("preferences.toml");
            let presets = config_dir.join("presets.toml");
            fs::write(&presets, "version = 1\npresets = []\n").unwrap();

            let result = apply_sync_batch_with_options(
                &SyncApplyBatch {
                    documents: vec![SyncDocument {
                        kind: SyncDocumentKind::Preferences,
                        id: "preferences".into(),
                        logical_path: "preferences.toml".into(),
                        contents: "version = 1\ndefault_density = \"comfortable\"\n".into(),
                    }],
                    tombstones: vec![SyncTombstone {
                        kind: SyncDocumentKind::Presets,
                        id: "presets".into(),
                        version: 2,
                        device_id: "device".into(),
                        deleted_at: "2026-07-10T00:00:00Z".into(),
                    }],
                },
                &SyncApplyOptions::default(),
            )
            .unwrap();

            assert_eq!(result.report.applied, 2);
            assert!(preferences.exists());
            assert!(!presets.exists());
            assert_eq!(
                result.affected_scopes,
                vec![
                    CompanionRefreshScope::Preferences,
                    CompanionRefreshScope::Presets
                ]
            );
        });
    }

    #[test]
    fn v2_asset_selection_defaults_closed_and_never_exports_unselected_commands() {
        with_config_home("sync-v2-selection", || {
            let config_dir = app_paths::config_dir().unwrap();
            fs::create_dir_all(&config_dir).unwrap();
            let selected = CliSnippet {
                id: "selected".into(),
                name: "Selected".into(),
                description: String::new(),
                command: "echo selected-command".into(),
                variables: Vec::new(),
                tags: Vec::new(),
            };
            let unselected = CliSnippet {
                id: "unselected".into(),
                name: "Unselected".into(),
                description: String::new(),
                command: "echo must-not-export".into(),
                variables: Vec::new(),
                tags: Vec::new(),
            };
            fs::write(
                config_dir.join("workspace-assets.toml"),
                toml::to_string_pretty(&AssetDocument::from_assets(WorkspaceAssets {
                    snippets: vec![selected, unselected],
                    ..WorkspaceAssets::default()
                }))
                .unwrap(),
            )
            .unwrap();

            let closed = load_sync_snapshot_selected_v2(SyncExportSelectionV2::personal(
                false,
                false,
                SyncAssetSelection::default(),
                Vec::new(),
            ))
            .unwrap();
            assert!(closed.documents.is_empty());

            let selected = load_sync_snapshot_selected_v2(SyncExportSelectionV2::personal(
                false,
                false,
                SyncAssetSelection {
                    snippet_ids: BTreeSet::from(["selected".into()]),
                    ..SyncAssetSelection::default()
                },
                Vec::new(),
            ))
            .unwrap();
            assert_eq!(selected.documents.len(), 1);
            assert!(selected.documents[0].contents.contains("selected-command"));
            assert!(!selected.documents[0].contents.contains("must-not-export"));
        });
    }

    #[test]
    fn unsupported_schema_and_duplicate_inner_ids_fail_before_write() {
        with_config_home("sync-schema-validation", || {
            let config_dir = app_paths::config_dir().unwrap();
            let result = apply_sync_batch_with_options(
                &SyncApplyBatch {
                    documents: vec![
                        SyncDocument {
                            kind: SyncDocumentKind::Preferences,
                            id: "preferences".into(),
                            logical_path: "preferences.toml".into(),
                            contents: "version = 99\n".into(),
                        },
                        SyncDocument {
                            kind: SyncDocumentKind::Presets,
                            id: "presets".into(),
                            logical_path: "presets.toml".into(),
                            contents: r#"
version = 1
[[presets]]
id = "duplicate"
name = "First"
description = ""
tags = []
root_label = "Root"
theme = "dark"
density = "compact"
[presets.layout]
kind = "tile"
[presets.layout.value]
id = "first"
title = "First"
agent_label = "Shell"
accent_class = "accent-cyan"
working_directory = { kind = "workspace-root" }
[[presets]]
id = "duplicate"
name = "Second"
description = ""
tags = []
root_label = "Root"
theme = "dark"
density = "compact"
[presets.layout]
kind = "tile"
[presets.layout.value]
id = "second"
title = "Second"
agent_label = "Shell"
accent_class = "accent-cyan"
working_directory = { kind = "workspace-root" }
"#
                            .into(),
                        },
                    ],
                    tombstones: Vec::new(),
                },
                &SyncApplyOptions::default(),
            )
            .unwrap();

            assert_eq!(result.report.errors, 2);
            assert!(!config_dir.join("preferences.toml").exists());
            assert!(!config_dir.join("presets.toml").exists());
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
