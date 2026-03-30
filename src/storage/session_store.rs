use std::fs;
use std::io;
use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::logging;
use crate::model::preset::WorkspacePreset;
use crate::platform::resolve_workspace_root;
use crate::storage::fs_utils::{atomic_write_private, preserve_corrupt_file};

const SESSION_VERSION: u32 = 1;

fn default_terminal_zoom_steps() -> i32 {
    0
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionDocument {
    version: u32,
    active_tab_index: usize,
    tabs: Vec<SavedTab>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedTab {
    pub preset: WorkspacePreset,
    pub workspace_root: PathBuf,
    pub custom_title: Option<String>,
    #[serde(default = "default_terminal_zoom_steps")]
    pub terminal_zoom_steps: i32,
}

#[derive(Clone, Debug)]
pub struct SavedSession {
    pub tabs: Vec<SavedTab>,
    pub active_tab_index: usize,
}

#[derive(Clone, Debug)]
pub struct SessionStore {
    path: Option<PathBuf>,
}

#[derive(Debug)]
pub struct SessionLoadOutcome {
    pub session: Option<SavedSession>,
    pub warning: Option<String>,
}

impl SessionStore {
    pub fn new() -> Self {
        let path = ProjectDirs::from("dev", "Zethrus", "TerminalTiler")
            .map(|dirs| dirs.data_dir().join("session.toml"));
        Self { path }
    }

    pub fn load_with_status(&self) -> SessionLoadOutcome {
        let Some(path) = self.path.as_ref() else {
            return SessionLoadOutcome {
                session: None,
                warning: Some(
                    "TerminalTiler could not resolve a data directory for session restore.".into(),
                ),
            };
        };

        let raw = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return SessionLoadOutcome {
                    session: None,
                    warning: None,
                };
            }
            Err(error) => {
                return SessionLoadOutcome {
                    session: None,
                    warning: Some(format!(
                        "TerminalTiler could not read the saved session file '{}': {}",
                        path.display(),
                        error
                    )),
                };
            }
        };

        let document = match toml::from_str::<SessionDocument>(&raw) {
            Ok(document) if document.version == SESSION_VERSION && !document.tabs.is_empty() => {
                document
            }
            Ok(_) => {
                return self.recover_invalid_session_document(
                    path,
                    "TerminalTiler moved an invalid saved session aside and started fresh.",
                );
            }
            Err(error) => {
                return self.recover_invalid_session_document(
                    path,
                    &format!(
                        "TerminalTiler found a corrupt saved session ({}) and moved it aside before startup.",
                        error
                    ),
                );
            }
        };

        let mut warnings = Vec::new();
        let tabs = document
            .tabs
            .into_iter()
            .filter_map(
                |mut tab| match resolve_workspace_root(&tab.workspace_root) {
                    Ok(path) => {
                        tab.workspace_root = path;
                        Some(tab)
                    }
                    Err(error) => {
                        warnings.push(format!(
                            "Skipped saved workspace '{}' because '{}' could not be restored: {}",
                            tab.preset.name,
                            tab.workspace_root.display(),
                            error
                        ));
                        None
                    }
                },
            )
            .collect::<Vec<_>>();

        if tabs.is_empty() {
            let warning = warnings
                .into_iter()
                .next()
                .unwrap_or_else(|| "TerminalTiler could not restore any saved workspaces.".into());
            return SessionLoadOutcome {
                session: None,
                warning: Some(warning),
            };
        }

        let active_tab_index = if document.active_tab_index < tabs.len() {
            document.active_tab_index
        } else {
            0
        };

        SessionLoadOutcome {
            session: Some(SavedSession {
                tabs,
                active_tab_index,
            }),
            warning: if warnings.is_empty() {
                None
            } else {
                Some(warnings.join("\n"))
            },
        }
    }

    pub fn save(&self, session: &SavedSession) {
        let Some(path) = &self.path else {
            return;
        };

        let document = SessionDocument {
            version: SESSION_VERSION,
            active_tab_index: session.active_tab_index,
            tabs: session.tabs.clone(),
        };

        let serialized = match toml::to_string_pretty(&document) {
            Ok(s) => s,
            Err(error) => {
                logging::info(format!("failed to serialize session: {}", error));
                return;
            }
        };

        if let Err(error) = atomic_write_private(path, &serialized) {
            logging::info(format!("failed to write session file: {}", error));
        }
    }

    pub fn clear(&self) {
        if let Some(path) = &self.path {
            let _ = fs::remove_file(path);
        }
    }

    fn recover_invalid_session_document(
        &self,
        path: &std::path::Path,
        message: &str,
    ) -> SessionLoadOutcome {
        let warning = match preserve_corrupt_file(path) {
            Ok(Some(preserved)) => format!("{message} Recovery copy: {}.", preserved.display()),
            Ok(None) => message.to_string(),
            Err(error) => format!(
                "{message} TerminalTiler could not preserve the original file: {}.",
                error
            ),
        };
        logging::error(&warning);
        SessionLoadOutcome {
            session: None,
            warning: Some(warning),
        }
    }
}

#[cfg(test)]
impl SessionStore {
    fn from_path(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }
}

#[cfg(test)]
mod tests {
    use super::{SESSION_VERSION, SavedTab, SessionDocument, SessionStore};
    use crate::model::layout::{WorkingDirectory, tile};
    use crate::model::preset::{ApplicationDensity, ThemeMode, WorkspacePreset};
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn temp_dir(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("terminaltiler-{prefix}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn sample_preset() -> WorkspacePreset {
        WorkspacePreset {
            id: "preset-1".into(),
            name: "Sample".into(),
            description: String::new(),
            tags: Vec::new(),
            root_label: "Workspace root".into(),
            theme: ThemeMode::System,
            density: ApplicationDensity::Compact,
            layout: tile(
                "tile-1",
                "Primary",
                "Shell",
                "accent-cyan",
                WorkingDirectory::WorkspaceRoot,
                None,
            ),
        }
    }

    #[test]
    fn moves_corrupt_session_file_aside() {
        let dir = temp_dir("corrupt-session");
        let path = dir.join("session.toml");
        fs::write(&path, "this is not toml").unwrap();
        let store = SessionStore::from_path(path.clone());

        let outcome = store.load_with_status();

        assert!(outcome.session.is_none());
        assert!(outcome.warning.is_some());
        assert!(!path.exists());
        let corrupt_copy_count = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains("session.toml.corrupt-")
            })
            .count();
        assert_eq!(corrupt_copy_count, 1);
    }

    #[test]
    fn skips_saved_tabs_that_can_no_longer_be_restored() {
        let dir = temp_dir("restore-filter");
        let valid_root = dir.join("workspace");
        fs::create_dir_all(&valid_root).unwrap();

        let path = dir.join("session.toml");
        let document = SessionDocument {
            version: SESSION_VERSION,
            active_tab_index: 1,
            tabs: vec![
                SavedTab {
                    preset: sample_preset(),
                    workspace_root: valid_root.clone(),
                    custom_title: Some("valid".into()),
                    terminal_zoom_steps: 2,
                },
                SavedTab {
                    preset: sample_preset(),
                    workspace_root: dir.join("missing"),
                    custom_title: Some("missing".into()),
                    terminal_zoom_steps: -1,
                },
            ],
        };
        fs::write(&path, toml::to_string_pretty(&document).unwrap()).unwrap();
        let store = SessionStore::from_path(path);

        let outcome = store.load_with_status();

        let session = outcome.session.expect("one valid tab should remain");
        assert_eq!(session.tabs.len(), 1);
        assert_eq!(session.active_tab_index, 0);
        assert!(session.tabs[0].workspace_root.is_absolute());
        assert_eq!(session.tabs[0].terminal_zoom_steps, 2);
        assert!(outcome.warning.is_some());
    }

    #[test]
    fn loads_legacy_sessions_without_zoom_steps() {
        let dir = temp_dir("legacy-session-zoom");
        let valid_root = dir.join("workspace");
        fs::create_dir_all(&valid_root).unwrap();

        let path = dir.join("session.toml");
        let legacy_document = SessionDocument {
            version: SESSION_VERSION,
            active_tab_index: 0,
            tabs: vec![SavedTab {
                preset: sample_preset(),
                workspace_root: valid_root,
                custom_title: Some("legacy".into()),
                terminal_zoom_steps: 3,
            }],
        };
        let legacy_session = toml::to_string_pretty(&legacy_document)
            .unwrap()
            .lines()
            .filter(|line| !line.trim_start().starts_with("terminal_zoom_steps = "))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, legacy_session).unwrap();
        let store = SessionStore::from_path(path);

        let outcome = store.load_with_status();

        let session = outcome.session.expect("legacy session should load");
        assert_eq!(session.tabs.len(), 1);
        assert_eq!(session.tabs[0].terminal_zoom_steps, 0);
    }
}
