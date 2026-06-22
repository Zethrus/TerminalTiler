use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app_paths;
use crate::model::board_workspace::{BoardLaunchRequest, BoardWorkspace};
use crate::storage::document::{
    preserve_corrupt_warning, read_optional_string, write_toml_private,
};

const STORE_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub struct BoardWorkspaceStore {
    path: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BoardWorkspaceDocument {
    version: u32,
    boards: Vec<BoardWorkspace>,
}

#[derive(Debug)]
pub struct BoardWorkspaceLoadOutcome {
    pub boards: Vec<BoardWorkspace>,
    pub warning: Option<String>,
}

impl Default for BoardWorkspaceStore {
    fn default() -> Self {
        Self::new()
    }
}

impl BoardWorkspaceStore {
    pub fn new() -> Self {
        let path = app_paths::config_dir().map(|dir| dir.join("board-workspaces.toml"));
        Self { path }
    }

    pub fn load(&self) -> Vec<BoardWorkspace> {
        self.load_with_status().boards
    }

    pub fn load_with_status(&self) -> BoardWorkspaceLoadOutcome {
        let Some(path) = &self.path else {
            return BoardWorkspaceLoadOutcome {
                boards: Vec::new(),
                warning: Some(
                    "TerminalTiler could not resolve a config directory for Kanban board shortcuts."
                        .into(),
                ),
            };
        };

        let raw = match read_optional_string(path) {
            Ok(Some(raw)) => raw,
            Ok(None) => {
                return BoardWorkspaceLoadOutcome {
                    boards: Vec::new(),
                    warning: None,
                };
            }
            Err(error) => {
                return BoardWorkspaceLoadOutcome {
                    boards: Vec::new(),
                    warning: Some(format!(
                        "TerminalTiler could not read your Kanban shortcut file '{}': {}",
                        path.display(),
                        error
                    )),
                };
            }
        };

        match toml::from_str::<BoardWorkspaceDocument>(&raw) {
            Ok(document) if document.version == STORE_VERSION => BoardWorkspaceLoadOutcome {
                boards: document.boards,
                warning: None,
            },
            Ok(_) => self.recover_invalid_document(
                path,
                "TerminalTiler moved an invalid Kanban shortcut file aside and loaded without board shortcuts.",
            ),
            Err(error) => self.recover_invalid_document(
                path,
                &format!(
                    "TerminalTiler found a corrupt Kanban shortcut file ({}) and moved it aside before loading without board shortcuts.",
                    error
                ),
            ),
        }
    }

    pub fn upsert_from_launch_request(
        &self,
        request: BoardLaunchRequest,
    ) -> io::Result<BoardWorkspace> {
        let id = request.id.clone().unwrap_or_else(unique_board_workspace_id);
        let workspace = request.into_workspace(id);
        self.upsert(workspace.clone())?;
        Ok(workspace)
    }

    pub fn upsert(&self, workspace: BoardWorkspace) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Err(io::Error::other(
                "TerminalTiler config directory is unavailable",
            ));
        };

        let mut boards = self.load();
        if let Some(existing) = boards.iter_mut().find(|item| item.id == workspace.id) {
            *existing = workspace;
        } else {
            boards.push(workspace);
        }
        self.write_to_path(path, &boards)
    }

    pub fn delete(&self, id: &str) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Err(io::Error::other(
                "TerminalTiler config directory is unavailable",
            ));
        };

        let mut boards = self.load();
        let before_len = boards.len();
        boards.retain(|board| board.id != id);
        if boards.len() == before_len {
            return Err(io::Error::other("Kanban board shortcut not found"));
        }
        self.write_to_path(path, &boards)
    }

    fn write_to_path(&self, path: &std::path::Path, boards: &[BoardWorkspace]) -> io::Result<()> {
        let document = BoardWorkspaceDocument {
            version: STORE_VERSION,
            boards: boards.to_vec(),
        };
        write_toml_private(path, &document)
    }

    fn recover_invalid_document(
        &self,
        path: &std::path::Path,
        message: &str,
    ) -> BoardWorkspaceLoadOutcome {
        let warning = preserve_corrupt_warning(path, message);
        BoardWorkspaceLoadOutcome {
            boards: Vec::new(),
            warning: Some(warning),
        }
    }
}

fn unique_board_workspace_id() -> String {
    format!("board-{}", Uuid::new_v4().simple())
}

#[cfg(test)]
impl BoardWorkspaceStore {
    fn from_path(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::preset::{ApplicationDensity, ThemeMode};
    use std::fs;

    fn temp_dir(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("terminaltiler-{prefix}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn request(id: Option<&str>, name: &str, root: PathBuf) -> BoardLaunchRequest {
        BoardLaunchRequest {
            id: id.map(str::to_string),
            name: name.into(),
            project_root: root,
            theme: ThemeMode::Dark,
            density: ApplicationDensity::Compact,
        }
    }

    #[test]
    fn missing_store_loads_empty() {
        let dir = temp_dir("board-workspaces-missing");
        let store = BoardWorkspaceStore::from_path(dir.join("boards.toml"));
        assert!(store.load().is_empty());
    }

    #[test]
    fn save_load_and_delete_round_trips_without_touching_board_file() {
        let dir = temp_dir("board-workspaces-roundtrip");
        let project = dir.join("project");
        fs::create_dir_all(&project).unwrap();
        let path = dir.join("boards.toml");
        let store = BoardWorkspaceStore::from_path(path.clone());

        let saved = store
            .upsert_from_launch_request(request(None, "Project Board", project.clone()))
            .unwrap();
        assert!(saved.id.starts_with("board-"));
        assert_eq!(store.load(), vec![saved.clone()]);
        assert!(!project.join(".terminaltiler").join("board.json").exists());

        store.delete(&saved.id).unwrap();
        assert!(store.load().is_empty());
        assert!(path.exists());
    }

    #[test]
    fn upsert_existing_shortcut_updates_in_place() {
        let dir = temp_dir("board-workspaces-upsert");
        let store = BoardWorkspaceStore::from_path(dir.join("boards.toml"));
        let first = store
            .upsert_from_launch_request(request(Some("board-fixed"), "Old", dir.join("old")))
            .unwrap();
        assert_eq!(first.id, "board-fixed");

        store
            .upsert_from_launch_request(request(Some("board-fixed"), "New", dir.join("new")))
            .unwrap();

        let boards = store.load();
        assert_eq!(boards.len(), 1);
        assert_eq!(boards[0].name, "New");
        assert_eq!(boards[0].project_root, dir.join("new"));
    }

    #[test]
    fn corrupt_store_is_preserved_and_recovers_empty() {
        let dir = temp_dir("board-workspaces-corrupt");
        let path = dir.join("boards.toml");
        fs::write(&path, "version = [").unwrap();
        let store = BoardWorkspaceStore::from_path(path.clone());

        let outcome = store.load_with_status();

        assert!(outcome.boards.is_empty());
        assert!(!path.exists());
        assert!(outcome.warning.as_deref().is_some_and(|warning| {
            warning.contains("corrupt Kanban shortcut file") && warning.contains("Recovery copy:")
        }));
        let preserved = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains("boards.toml.corrupt-")
            })
            .count();
        assert_eq!(preserved, 1);
    }
}
