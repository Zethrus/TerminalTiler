//! Launch-deck shortcuts for per-project Kanban boards.
//!
//! Board data itself stays in `<project_root>/.terminaltiler/board.json`; these
//! records are only bookmarks shown on the launch deck so a user can reopen a
//! board without launching a terminal workspace first.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::preset::{ApplicationDensity, ThemeMode};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardWorkspace {
    pub id: String,
    pub name: String,
    pub project_root: PathBuf,
    pub theme: ThemeMode,
    pub density: ApplicationDensity,
}

impl BoardWorkspace {
    pub fn project_label(&self) -> String {
        self.project_root.display().to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardLaunchRequest {
    pub id: Option<String>,
    pub name: String,
    pub project_root: PathBuf,
    pub theme: ThemeMode,
    pub density: ApplicationDensity,
}

impl BoardLaunchRequest {
    pub fn into_workspace(self, id: String) -> BoardWorkspace {
        BoardWorkspace {
            id,
            name: self.name,
            project_root: self.project_root,
            theme: self.theme,
            density: self.density,
        }
    }
}
