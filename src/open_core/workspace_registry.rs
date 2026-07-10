use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app_paths;

const WORKSPACE_REGISTRY_VERSION: u32 = 1;
const WORKSPACE_REGISTRY_FILE: &str = "active-workspaces.toml";

/// Machine-local workspace identity. Only `id` belongs on the wire; `root`
/// and aliases are local routing authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceDescriptor {
    pub id: String,
    pub root: PathBuf,
    #[serde(default)]
    pub legacy_aliases: Vec<String>,
}

impl WorkspaceDescriptor {
    pub fn wire_id(&self) -> &str {
        &self.id
    }

    pub fn matches_wire_id(&self, id: &str) -> bool {
        self.id == id || self.legacy_aliases.iter().any(|alias| alias == id)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceRegistrySnapshot {
    #[serde(default)]
    pub workspaces: Vec<WorkspaceDescriptor>,
}

pub type WorkspaceRegistrySnapshotCallback =
    Arc<dyn Fn() -> io::Result<WorkspaceRegistrySnapshot> + Send + Sync>;

#[derive(Clone, Debug)]
pub struct ActiveWorkspaceRegistry {
    path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct WorkspaceRegistryDocument {
    version: u32,
    #[serde(default)]
    workspaces: Vec<WorkspaceDescriptor>,
}

impl ActiveWorkspaceRegistry {
    pub fn open_default() -> io::Result<Self> {
        let path = app_paths::state_dir()
            .ok_or_else(|| io::Error::other("TerminalTiler state directory is unavailable"))?
            .join(WORKSPACE_REGISTRY_FILE);
        Ok(Self { path })
    }

    pub fn snapshot(&self) -> io::Result<WorkspaceRegistrySnapshot> {
        read_registry(&self.path)
    }

    pub fn snapshot_callback(&self) -> WorkspaceRegistrySnapshotCallback {
        let registry = self.clone();
        Arc::new(move || registry.snapshot())
    }

    /// Register an existing local root under a stable opaque id. The legacy
    /// path-shaped id is retained as a local alias for migration only.
    pub fn register(&self, root: &Path) -> io::Result<WorkspaceDescriptor> {
        let legacy_id = format!("workspace:{}", root.display());
        self.register_with_legacy_alias(root, Some(legacy_id))
    }

    pub fn register_with_legacy_alias(
        &self,
        root: &Path,
        legacy_id: Option<String>,
    ) -> io::Result<WorkspaceDescriptor> {
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("workspace root '{}' is not a directory", root.display()),
            ));
        }

        crate::storage::fs_utils::with_persistence_lock(|| {
            let mut snapshot = read_registry(&self.path)?;
            if let Some(existing_index) = snapshot
                .workspaces
                .iter()
                .position(|workspace| workspace.root == root)
            {
                let existing = &mut snapshot.workspaces[existing_index];
                if let Some(alias) = normalized_legacy_alias(legacy_id)
                    && alias != existing.id
                    && !existing.legacy_aliases.contains(&alias)
                {
                    existing.legacy_aliases.push(alias);
                    existing.legacy_aliases.sort();
                }
                let existing = existing.clone();
                write_registry_unlocked(&self.path, &snapshot)?;
                return Ok(existing);
            }

            let descriptor = WorkspaceDescriptor {
                id: format!("workspace:{}", Uuid::new_v4()),
                root,
                legacy_aliases: normalized_legacy_alias(legacy_id).into_iter().collect(),
            };
            snapshot.workspaces.push(descriptor.clone());
            snapshot
                .workspaces
                .sort_by(|left, right| left.id.cmp(&right.id));
            validate_snapshot(&snapshot)?;
            write_registry_unlocked(&self.path, &snapshot)?;
            Ok(descriptor)
        })
    }

    pub fn resolve(&self, wire_id: &str) -> io::Result<Option<WorkspaceDescriptor>> {
        Ok(self
            .snapshot()?
            .workspaces
            .into_iter()
            .find(|workspace| workspace.matches_wire_id(wire_id)))
    }
}

fn normalized_legacy_alias(alias: Option<String>) -> Option<String> {
    alias
        .map(|alias| alias.trim().to_string())
        .filter(|alias| !alias.is_empty())
        .map(|alias| {
            if alias.starts_with("workspace:") {
                alias
            } else {
                format!("workspace:{alias}")
            }
        })
}

fn read_registry(path: &Path) -> io::Result<WorkspaceRegistrySnapshot> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(WorkspaceRegistrySnapshot::default());
        }
        Err(error) => return Err(error),
    };
    let document: WorkspaceRegistryDocument = toml::from_str(&raw).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid workspace registry: {error}"),
        )
    })?;
    if document.version != WORKSPACE_REGISTRY_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported workspace registry version {}",
                document.version
            ),
        ));
    }
    let snapshot = WorkspaceRegistrySnapshot {
        workspaces: document.workspaces,
    };
    validate_snapshot(&snapshot)?;
    Ok(snapshot)
}

fn write_registry_unlocked(path: &Path, snapshot: &WorkspaceRegistrySnapshot) -> io::Result<()> {
    validate_snapshot(snapshot)?;
    let raw = toml::to_string_pretty(&WorkspaceRegistryDocument {
        version: WORKSPACE_REGISTRY_VERSION,
        workspaces: snapshot.workspaces.clone(),
    })
    .map_err(|error| io::Error::other(format!("serialize workspace registry: {error}")))?;
    crate::storage::fs_utils::atomic_write_private_unlocked(path, &raw)
}

fn validate_snapshot(snapshot: &WorkspaceRegistrySnapshot) -> io::Result<()> {
    let mut ids = BTreeSet::new();
    let mut roots = BTreeSet::new();
    for workspace in &snapshot.workspaces {
        let raw_id = workspace.id.strip_prefix("workspace:").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "workspace id must use the workspace:<uuid> form",
            )
        })?;
        Uuid::parse_str(raw_id).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "workspace id must contain a valid UUID",
            )
        })?;
        if !ids.insert(workspace.id.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "workspace registry contains duplicate ids",
            ));
        }
        if !roots.insert(workspace.root.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "workspace registry contains duplicate roots",
            ));
        }
        for alias in &workspace.legacy_aliases {
            if !ids.insert(alias.clone()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "workspace registry contains duplicate aliases",
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ActiveWorkspaceRegistry;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn opaque_workspace_identity_is_stable_and_preserves_legacy_alias() {
        let dir = std::env::temp_dir().join(format!(
            "terminaltiler-workspace-registry-{}",
            Uuid::new_v4()
        ));
        let root = dir.join("workspace");
        fs::create_dir_all(&root).unwrap();
        let registry = ActiveWorkspaceRegistry {
            path: dir.join("state").join("active-workspaces.toml"),
        };

        let first = registry.register(&root).unwrap();
        let second = registry.register(&root).unwrap();

        assert_eq!(first, second);
        assert!(first.id.starts_with("workspace:"));
        assert!(Uuid::parse_str(first.id.trim_start_matches("workspace:")).is_ok());
        assert_eq!(first.legacy_aliases.len(), 1);
        assert_eq!(
            registry.resolve(&first.legacy_aliases[0]).unwrap(),
            Some(first)
        );
    }
}
