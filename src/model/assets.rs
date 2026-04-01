use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectionKind {
    Local,
    Ssh,
    Wsl,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "value")]
pub enum TileConnectionTarget {
    Local,
    Profile(String),
}

impl Default for TileConnectionTarget {
    fn default() -> Self {
        Self::Local
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub id: String,
    pub name: String,
    pub kind: ConnectionKind,
    #[serde(default)]
    pub inventory_host_id: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub remote_working_directory: Option<String>,
    #[serde(default)]
    pub shell_program: Option<String>,
    #[serde(default)]
    pub startup_prefix: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryGroup {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryHost {
    pub id: String,
    pub name: String,
    pub host: String,
    #[serde(default)]
    pub group_ids: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub main_ip: String,
    #[serde(default)]
    pub user: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    #[serde(default, alias = "pricepermonth")]
    pub price_per_month_usd_cents: u64,
    #[serde(default, alias = "password")]
    pub password_secret_ref: Option<String>,
    #[serde(default, alias = "sshkey")]
    pub ssh_key_path: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputHelperRule {
    pub id: String,
    pub label: String,
    pub regex: String,
    #[serde(default = "default_output_severity")]
    pub severity: OutputSeverity,
    #[serde(default)]
    pub toast_on_match: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRoleTemplate {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_accent_class")]
    pub accent_class: String,
    #[serde(default)]
    pub default_title: Option<String>,
    #[serde(default)]
    pub default_agent_label: Option<String>,
    #[serde(default)]
    pub default_startup_command: Option<String>,
    #[serde(default)]
    pub default_connection_profile_id: Option<String>,
    #[serde(default)]
    pub default_pane_groups: Vec<String>,
    #[serde(default)]
    pub default_output_helpers: Vec<OutputHelperRule>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestoreLaunchMode {
    Prompt,
    RerunStartupCommands,
    ShellOnly,
}

impl RestoreLaunchMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Prompt => "Prompt",
            Self::RerunStartupCommands => "Resume And Rerun",
            Self::ShellOnly => "Resume As Shells",
        }
    }
}

impl Default for RestoreLaunchMode {
    fn default() -> Self {
        Self::Prompt
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceAssets {
    #[serde(default)]
    pub connection_profiles: Vec<ConnectionProfile>,
    #[serde(default)]
    pub inventory_hosts: Vec<InventoryHost>,
    #[serde(default)]
    pub inventory_groups: Vec<InventoryGroup>,
    #[serde(default)]
    pub role_templates: Vec<AgentRoleTemplate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectSuggestion {
    pub id: String,
    pub title: String,
    pub description: String,
    pub role_ids: Vec<String>,
    pub tile_count: usize,
    pub startup_commands: Vec<Option<String>>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaneStatusSnapshot {
    pub connection_label: String,
    pub location_label: String,
    pub shell_label: String,
    pub helper_label: String,
    pub helper_severity: Option<OutputSeverity>,
}

impl PaneStatusSnapshot {
    pub fn to_line(&self) -> String {
        [
            self.connection_label.trim(),
            self.location_label.trim(),
            self.shell_label.trim(),
            self.helper_label.trim(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("  •  ")
    }
}

fn default_ssh_port() -> u16 {
    22
}

fn default_output_severity() -> OutputSeverity {
    OutputSeverity::Warning
}

fn default_accent_class() -> String {
    "accent-cyan".into()
}

pub fn builtin_role_templates() -> Vec<AgentRoleTemplate> {
    vec![
        AgentRoleTemplate {
            id: "planner".into(),
            name: "Planner".into(),
            description: "Plan-first role for coordinating work and writing specs.".into(),
            accent_class: "accent-violet".into(),
            default_title: Some("Planner".into()),
            default_agent_label: Some("Planner".into()),
            default_startup_command: Some("codex".into()),
            default_connection_profile_id: None,
            default_pane_groups: vec!["planning".into()],
            default_output_helpers: vec![OutputHelperRule {
                id: "planner-error".into(),
                label: "Planning error".into(),
                regex: "(?i)(error|failed|panic)".into(),
                severity: OutputSeverity::Error,
                toast_on_match: true,
            }],
        },
        AgentRoleTemplate {
            id: "implementer".into(),
            name: "Implementer".into(),
            description: "Primary build-and-edit workspace role.".into(),
            accent_class: "accent-cyan".into(),
            default_title: Some("Implementer".into()),
            default_agent_label: Some("Build".into()),
            default_startup_command: Some("bash".into()),
            default_connection_profile_id: None,
            default_pane_groups: vec!["delivery".into()],
            default_output_helpers: vec![
                OutputHelperRule {
                    id: "compile-error".into(),
                    label: "Compile error".into(),
                    regex: "(?i)(error\\[|compilation failed|build failed)".into(),
                    severity: OutputSeverity::Error,
                    toast_on_match: true,
                },
                OutputHelperRule {
                    id: "test-failed".into(),
                    label: "Test failure".into(),
                    regex: "(?i)(test result: FAILED|assertion failed|failures:)".into(),
                    severity: OutputSeverity::Warning,
                    toast_on_match: true,
                },
            ],
        },
        AgentRoleTemplate {
            id: "reviewer".into(),
            name: "Reviewer".into(),
            description: "Review and validation role.".into(),
            accent_class: "accent-rose".into(),
            default_title: Some("Reviewer".into()),
            default_agent_label: Some("QA".into()),
            default_startup_command: Some("bash".into()),
            default_connection_profile_id: None,
            default_pane_groups: vec!["review".into()],
            default_output_helpers: vec![OutputHelperRule {
                id: "qa-warning".into(),
                label: "QA warning".into(),
                regex: "(?i)(warning|deprecated)".into(),
                severity: OutputSeverity::Info,
                toast_on_match: false,
            }],
        },
        AgentRoleTemplate {
            id: "ops".into(),
            name: "Ops".into(),
            description: "Operations role for remote hosts and service work.".into(),
            accent_class: "accent-amber".into(),
            default_title: Some("Ops".into()),
            default_agent_label: Some("Ops".into()),
            default_startup_command: Some("bash".into()),
            default_connection_profile_id: None,
            default_pane_groups: vec!["ops".into()],
            default_output_helpers: vec![
                OutputHelperRule {
                    id: "ssh-failure".into(),
                    label: "SSH failure".into(),
                    regex: "(?i)(permission denied|connection refused|timed out|host key verification failed)".into(),
                    severity: OutputSeverity::Error,
                    toast_on_match: true,
                },
                OutputHelperRule {
                    id: "service-failure".into(),
                    label: "Service failure".into(),
                    regex: "(?i)(failed|unhealthy|degraded|critical)".into(),
                    severity: OutputSeverity::Warning,
                    toast_on_match: true,
                },
            ],
        },
        AgentRoleTemplate {
            id: "release".into(),
            name: "Release".into(),
            description: "Packaging and release automation role.".into(),
            accent_class: "accent-violet".into(),
            default_title: Some("Release".into()),
            default_agent_label: Some("Release".into()),
            default_startup_command: Some("bash".into()),
            default_connection_profile_id: None,
            default_pane_groups: vec!["delivery".into()],
            default_output_helpers: vec![OutputHelperRule {
                id: "release-error".into(),
                label: "Release error".into(),
                regex: "(?i)(release failed|publish failed|artifact missing)".into(),
                severity: OutputSeverity::Error,
                toast_on_match: true,
            }],
        },
    ]
}
