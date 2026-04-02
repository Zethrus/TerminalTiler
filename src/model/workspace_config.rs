use serde::{Deserialize, Serialize};

use crate::model::assets::WorkspaceAssets;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigScope {
    Global,
    Workspace,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    #[serde(default)]
    pub assets: WorkspaceAssets,
    #[serde(default)]
    pub introspection: RepoIntrospectionConfig,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoIntrospectionConfig {
    #[serde(default)]
    pub suggestion_overrides: Vec<SuggestionOverride>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestionOverride {
    pub id: String,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub startup_commands: Option<Vec<Option<String>>>,
}
