use std::collections::{HashMap, HashSet};

use regex::Regex;

use crate::model::assets::{
    AgentRoleTemplate, CliSnippet, ConnectionProfile, InventoryGroup, InventoryHost,
    OutputHelperRule, Runbook, RunbookStep, RunbookTarget, RunbookVariable, SnippetVariable,
    WorkspaceAssets, builtin_role_templates,
};
use crate::model::workspace_config::ConfigScope;
use crate::storage::asset_store::{merge_assets_with_builtins, merge_workspace_assets_for_view};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssetSection {
    Overview,
    Connections,
    Hosts,
    Groups,
    Roles,
    Runbooks,
    Snippets,
    RawToml,
}

impl AssetSection {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Connections => "Connections",
            Self::Hosts => "Hosts",
            Self::Groups => "Groups",
            Self::Roles => "Roles",
            Self::Runbooks => "Runbooks",
            Self::Snippets => "Snippets",
            Self::RawToml => "Raw TOML",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Overview => "Scope guidance, effective asset counts, and quick-start notes.",
            Self::Connections => "Connection profiles used by tiles and roles.",
            Self::Hosts => "Inventory hosts that can back SSH connection profiles.",
            Self::Groups => "Shared host groups for inventory organization.",
            Self::Roles => "Role templates with startup defaults and output helpers.",
            Self::Runbooks => "Reusable commands that target panes, roles, or connections.",
            Self::Snippets => "Single-pane CLI macros with optional variable prompts.",
            Self::RawToml => "Advanced editing for the current scope document.",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetItemSource {
    BuiltIn,
    Global,
    Workspace,
    WorkspaceOverride,
}

impl AssetItemSource {
    pub fn label(&self) -> &'static str {
        match self {
            Self::BuiltIn => "Built-in",
            Self::Global => "Global",
            Self::Workspace => "Workspace",
            Self::WorkspaceOverride => "Workspace override",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetValidationIssue {
    pub section: AssetSection,
    pub item_id: Option<String>,
    pub message: String,
}

pub fn effective_assets_for_scope(
    scope: ConfigScope,
    current_assets: &WorkspaceAssets,
    global_assets: &WorkspaceAssets,
) -> WorkspaceAssets {
    match scope {
        ConfigScope::Global => merge_assets_with_builtins(current_assets.clone()),
        ConfigScope::Workspace => merge_assets_with_builtins(merge_workspace_assets_for_view(
            global_assets,
            current_assets,
        )),
    }
}

pub fn validate_assets(
    scope: ConfigScope,
    current_assets: &WorkspaceAssets,
    global_assets: &WorkspaceAssets,
) -> Vec<AssetValidationIssue> {
    let sanitized_assets = prune_blank_drafts(current_assets.clone());
    let effective_assets = effective_assets_for_scope(scope, &sanitized_assets, global_assets);
    let mut issues = Vec::new();

    validate_connection_profiles(
        &mut issues,
        &sanitized_assets.connection_profiles,
        &effective_assets,
    );
    validate_inventory_hosts(
        &mut issues,
        &sanitized_assets.inventory_hosts,
        &effective_assets,
    );
    validate_inventory_groups(&mut issues, &sanitized_assets.inventory_groups);
    validate_roles(
        &mut issues,
        &sanitized_assets.role_templates,
        &effective_assets,
    );
    validate_runbooks(&mut issues, &sanitized_assets.runbooks, &effective_assets);
    validate_snippets(&mut issues, &sanitized_assets.snippets);

    issues
}

pub fn prune_blank_drafts(mut assets: WorkspaceAssets) -> WorkspaceAssets {
    assets
        .connection_profiles
        .retain(|item| !is_blank_connection_profile(item));
    assets.inventory_hosts.retain(|item| !is_blank_host(item));
    assets.inventory_groups.retain(|item| !is_blank_group(item));
    assets.role_templates.retain(|item| !is_blank_role(item));
    for role in &mut assets.role_templates {
        role.default_output_helpers
            .retain(|item| !is_blank_output_helper(item));
    }
    assets.runbooks.retain(|item| !is_blank_runbook(item));
    for runbook in &mut assets.runbooks {
        runbook
            .variables
            .retain(|item| !is_blank_runbook_variable(item));
        runbook.steps.retain(|item| !is_blank_runbook_step(item));
    }
    assets.snippets.retain(|item| !is_blank_snippet(item));
    for snippet in &mut assets.snippets {
        snippet
            .variables
            .retain(|item| !is_blank_snippet_variable(item));
    }
    assets
}

pub fn connection_source(
    scope: ConfigScope,
    id: &str,
    current_assets: &WorkspaceAssets,
    global_assets: &WorkspaceAssets,
) -> AssetItemSource {
    simple_source(
        scope,
        id,
        &current_assets
            .connection_profiles
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>(),
        &global_assets
            .connection_profiles
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>(),
        &HashSet::new(),
    )
}

pub fn host_source(
    scope: ConfigScope,
    id: &str,
    current_assets: &WorkspaceAssets,
    global_assets: &WorkspaceAssets,
) -> AssetItemSource {
    simple_source(
        scope,
        id,
        &current_assets
            .inventory_hosts
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>(),
        &global_assets
            .inventory_hosts
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>(),
        &HashSet::new(),
    )
}

pub fn group_source(
    scope: ConfigScope,
    id: &str,
    current_assets: &WorkspaceAssets,
    global_assets: &WorkspaceAssets,
) -> AssetItemSource {
    simple_source(
        scope,
        id,
        &current_assets
            .inventory_groups
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>(),
        &global_assets
            .inventory_groups
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>(),
        &HashSet::new(),
    )
}

pub fn role_source(
    scope: ConfigScope,
    id: &str,
    current_assets: &WorkspaceAssets,
    global_assets: &WorkspaceAssets,
) -> AssetItemSource {
    let builtins = builtin_role_templates()
        .into_iter()
        .map(|item| item.id)
        .collect::<HashSet<_>>();
    let builtin_refs = builtins.iter().map(String::as_str).collect::<HashSet<_>>();
    simple_source(
        scope,
        id,
        &current_assets
            .role_templates
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>(),
        &global_assets
            .role_templates
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>(),
        &builtin_refs,
    )
}

pub fn runbook_source(
    scope: ConfigScope,
    id: &str,
    current_assets: &WorkspaceAssets,
    global_assets: &WorkspaceAssets,
) -> AssetItemSource {
    simple_source(
        scope,
        id,
        &current_assets
            .runbooks
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>(),
        &global_assets
            .runbooks
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>(),
        &HashSet::new(),
    )
}

pub fn snippet_source(
    scope: ConfigScope,
    id: &str,
    current_assets: &WorkspaceAssets,
    global_assets: &WorkspaceAssets,
) -> AssetItemSource {
    simple_source(
        scope,
        id,
        &current_assets
            .snippets
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>(),
        &global_assets
            .snippets
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>(),
        &HashSet::new(),
    )
}

fn simple_source(
    scope: ConfigScope,
    id: &str,
    current_ids: &HashSet<&str>,
    global_ids: &HashSet<&str>,
    builtin_ids: &HashSet<&str>,
) -> AssetItemSource {
    match scope {
        ConfigScope::Global => {
            if builtin_ids.contains(id) {
                AssetItemSource::BuiltIn
            } else {
                AssetItemSource::Global
            }
        }
        ConfigScope::Workspace => {
            if current_ids.contains(id) {
                if global_ids.contains(id) || builtin_ids.contains(id) {
                    AssetItemSource::WorkspaceOverride
                } else {
                    AssetItemSource::Workspace
                }
            } else if builtin_ids.contains(id) {
                AssetItemSource::BuiltIn
            } else {
                AssetItemSource::Global
            }
        }
    }
}

fn validate_connection_profiles(
    issues: &mut Vec<AssetValidationIssue>,
    profiles: &[ConnectionProfile],
    effective_assets: &WorkspaceAssets,
) {
    validate_duplicate_ids(
        issues,
        AssetSection::Connections,
        profiles.iter().map(|item| item.id.as_str()),
    );

    let host_ids = effective_assets
        .inventory_hosts
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    for profile in profiles {
        if is_blank_connection_profile(profile) {
            continue;
        }
        require_field(
            issues,
            AssetSection::Connections,
            &profile.id,
            "Connection profiles need an ID.",
            !profile.id.trim().is_empty(),
        );
        require_field(
            issues,
            AssetSection::Connections,
            &profile.id,
            "Connection profiles need a name.",
            !profile.name.trim().is_empty(),
        );
        if let Some(host_id) = profile.inventory_host_id.as_deref()
            && !host_id.trim().is_empty()
            && !host_ids.contains(host_id)
        {
            issues.push(AssetValidationIssue {
                section: AssetSection::Connections,
                item_id: Some(profile.id.clone()),
                message: format!("Connection references missing host \"{host_id}\"."),
            });
        }
    }
}

fn validate_inventory_hosts(
    issues: &mut Vec<AssetValidationIssue>,
    hosts: &[InventoryHost],
    effective_assets: &WorkspaceAssets,
) {
    validate_duplicate_ids(
        issues,
        AssetSection::Hosts,
        hosts.iter().map(|item| item.id.as_str()),
    );

    let group_ids = effective_assets
        .inventory_groups
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    for host in hosts {
        if is_blank_host(host) {
            continue;
        }
        require_field(
            issues,
            AssetSection::Hosts,
            &host.id,
            "Hosts need an ID.",
            !host.id.trim().is_empty(),
        );
        require_field(
            issues,
            AssetSection::Hosts,
            &host.id,
            "Hosts need a name.",
            !host.name.trim().is_empty(),
        );
        require_field(
            issues,
            AssetSection::Hosts,
            &host.id,
            "Hosts need a hostname or address.",
            !host.host.trim().is_empty(),
        );
        for group_id in &host.group_ids {
            if !group_id.trim().is_empty() && !group_ids.contains(group_id.as_str()) {
                issues.push(AssetValidationIssue {
                    section: AssetSection::Hosts,
                    item_id: Some(host.id.clone()),
                    message: format!("Host references missing group \"{group_id}\"."),
                });
            }
        }
    }
}

fn validate_inventory_groups(issues: &mut Vec<AssetValidationIssue>, groups: &[InventoryGroup]) {
    validate_duplicate_ids(
        issues,
        AssetSection::Groups,
        groups.iter().map(|item| item.id.as_str()),
    );

    for group in groups {
        if is_blank_group(group) {
            continue;
        }
        require_field(
            issues,
            AssetSection::Groups,
            &group.id,
            "Groups need an ID.",
            !group.id.trim().is_empty(),
        );
        require_field(
            issues,
            AssetSection::Groups,
            &group.id,
            "Groups need a name.",
            !group.name.trim().is_empty(),
        );
    }
}

fn validate_roles(
    issues: &mut Vec<AssetValidationIssue>,
    roles: &[AgentRoleTemplate],
    effective_assets: &WorkspaceAssets,
) {
    validate_duplicate_ids(
        issues,
        AssetSection::Roles,
        roles.iter().map(|item| item.id.as_str()),
    );

    let connection_ids = effective_assets
        .connection_profiles
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();

    for role in roles {
        if is_blank_role(role) {
            continue;
        }
        require_field(
            issues,
            AssetSection::Roles,
            &role.id,
            "Roles need an ID.",
            !role.id.trim().is_empty(),
        );
        require_field(
            issues,
            AssetSection::Roles,
            &role.id,
            "Roles need a name.",
            !role.name.trim().is_empty(),
        );
        if let Some(connection_id) = role.default_connection_profile_id.as_deref()
            && !connection_id.trim().is_empty()
            && !connection_ids.contains(connection_id)
        {
            issues.push(AssetValidationIssue {
                section: AssetSection::Roles,
                item_id: Some(role.id.clone()),
                message: format!("Role references missing connection \"{connection_id}\"."),
            });
        }
        for helper in &role.default_output_helpers {
            if is_blank_output_helper(helper) {
                continue;
            }
            require_field(
                issues,
                AssetSection::Roles,
                &role.id,
                "Output helper rules need an ID.",
                !helper.id.trim().is_empty(),
            );
            require_field(
                issues,
                AssetSection::Roles,
                &role.id,
                "Output helper rules need a label.",
                !helper.label.trim().is_empty(),
            );
            require_field(
                issues,
                AssetSection::Roles,
                &role.id,
                "Output helper rules need a regex.",
                !helper.regex.trim().is_empty(),
            );
            if !helper.regex.trim().is_empty() && Regex::new(&helper.regex).is_err() {
                issues.push(AssetValidationIssue {
                    section: AssetSection::Roles,
                    item_id: Some(role.id.clone()),
                    message: format!("Role helper \"{}\" has an invalid regex.", helper.label),
                });
            }
        }
    }
}

fn validate_runbooks(
    issues: &mut Vec<AssetValidationIssue>,
    runbooks: &[Runbook],
    effective_assets: &WorkspaceAssets,
) {
    validate_duplicate_ids(
        issues,
        AssetSection::Runbooks,
        runbooks.iter().map(|item| item.id.as_str()),
    );

    let role_ids = effective_assets
        .role_templates
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let connection_ids = effective_assets
        .connection_profiles
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();

    for runbook in runbooks {
        if is_blank_runbook(runbook) {
            continue;
        }
        require_field(
            issues,
            AssetSection::Runbooks,
            &runbook.id,
            "Runbooks need an ID.",
            !runbook.id.trim().is_empty(),
        );
        require_field(
            issues,
            AssetSection::Runbooks,
            &runbook.id,
            "Runbooks need a name.",
            !runbook.name.trim().is_empty(),
        );
        match &runbook.target {
            RunbookTarget::Role(role_id)
                if !role_id.trim().is_empty() && !role_ids.contains(role_id.as_str()) =>
            {
                issues.push(AssetValidationIssue {
                    section: AssetSection::Runbooks,
                    item_id: Some(runbook.id.clone()),
                    message: format!("Runbook references missing role \"{role_id}\"."),
                });
            }
            RunbookTarget::ConnectionProfile(connection_id)
                if !connection_id.trim().is_empty()
                    && !connection_ids.contains(connection_id.as_str()) =>
            {
                issues.push(AssetValidationIssue {
                    section: AssetSection::Runbooks,
                    item_id: Some(runbook.id.clone()),
                    message: format!("Runbook references missing connection \"{connection_id}\"."),
                });
            }
            _ => {}
        }

        let mut variable_ids = HashMap::<&str, usize>::new();
        for variable in &runbook.variables {
            if is_blank_runbook_variable(variable) {
                continue;
            }
            *variable_ids.entry(variable.id.as_str()).or_insert(0) += 1;
            require_field(
                issues,
                AssetSection::Runbooks,
                &runbook.id,
                "Runbook variables need an ID.",
                !variable.id.trim().is_empty(),
            );
            require_field(
                issues,
                AssetSection::Runbooks,
                &runbook.id,
                "Runbook variables need a label.",
                !variable.label.trim().is_empty(),
            );
        }
        for (variable_id, count) in variable_ids {
            if !variable_id.trim().is_empty() && count > 1 {
                issues.push(AssetValidationIssue {
                    section: AssetSection::Runbooks,
                    item_id: Some(runbook.id.clone()),
                    message: format!("Runbook variable ID \"{variable_id}\" is duplicated."),
                });
            }
        }

        let mut step_ids = HashMap::<&str, usize>::new();
        for step in &runbook.steps {
            if is_blank_runbook_step(step) {
                continue;
            }
            *step_ids.entry(step.id.as_str()).or_insert(0) += 1;
            require_field(
                issues,
                AssetSection::Runbooks,
                &runbook.id,
                "Runbook steps need an ID.",
                !step.id.trim().is_empty(),
            );
            require_field(
                issues,
                AssetSection::Runbooks,
                &runbook.id,
                "Runbook steps need a label.",
                !step.label.trim().is_empty(),
            );
            require_field(
                issues,
                AssetSection::Runbooks,
                &runbook.id,
                "Runbook steps need a command.",
                !step.command.trim().is_empty(),
            );
        }
        for (step_id, count) in step_ids {
            if !step_id.trim().is_empty() && count > 1 {
                issues.push(AssetValidationIssue {
                    section: AssetSection::Runbooks,
                    item_id: Some(runbook.id.clone()),
                    message: format!("Runbook step ID \"{step_id}\" is duplicated."),
                });
            }
        }
    }
}

fn validate_snippets(issues: &mut Vec<AssetValidationIssue>, snippets: &[CliSnippet]) {
    validate_duplicate_ids(
        issues,
        AssetSection::Snippets,
        snippets.iter().map(|item| item.id.as_str()),
    );

    for snippet in snippets {
        if is_blank_snippet(snippet) {
            continue;
        }
        require_field(
            issues,
            AssetSection::Snippets,
            &snippet.id,
            "Snippets need an ID.",
            !snippet.id.trim().is_empty(),
        );
        require_field(
            issues,
            AssetSection::Snippets,
            &snippet.id,
            "Snippets need a name.",
            !snippet.name.trim().is_empty(),
        );
        require_field(
            issues,
            AssetSection::Snippets,
            &snippet.id,
            "Snippets need a command.",
            !snippet.command.trim().is_empty(),
        );

        let mut variable_ids = HashMap::<&str, usize>::new();
        for variable in &snippet.variables {
            if is_blank_snippet_variable(variable) {
                continue;
            }
            *variable_ids.entry(variable.id.as_str()).or_insert(0) += 1;
            require_field(
                issues,
                AssetSection::Snippets,
                &snippet.id,
                "Snippet variables need an ID.",
                !variable.id.trim().is_empty(),
            );
            require_field(
                issues,
                AssetSection::Snippets,
                &snippet.id,
                "Snippet variables need a label.",
                !variable.label.trim().is_empty(),
            );
        }
        for (variable_id, count) in variable_ids {
            if !variable_id.trim().is_empty() && count > 1 {
                issues.push(AssetValidationIssue {
                    section: AssetSection::Snippets,
                    item_id: Some(snippet.id.clone()),
                    message: format!("Snippet variable ID \"{variable_id}\" is duplicated."),
                });
            }
        }
    }
}

fn is_blank_connection_profile(item: &ConnectionProfile) -> bool {
    item.id.trim().is_empty()
        && item.name.trim().is_empty()
        && item
            .inventory_host_id
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        && item
            .remote_working_directory
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        && item
            .shell_program
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        && item
            .startup_prefix
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        && item.tags.is_empty()
}

fn is_blank_host(item: &InventoryHost) -> bool {
    item.id.trim().is_empty()
        && item.name.trim().is_empty()
        && item.host.trim().is_empty()
        && item.group_ids.is_empty()
        && item.tags.is_empty()
        && item.provider.trim().is_empty()
        && item.main_ip.trim().is_empty()
        && item.user.trim().is_empty()
        && item.port == 22
        && item.price_per_month_usd_cents == 0
        && item
            .password_secret_ref
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        && item.ssh_key_path.as_deref().unwrap_or("").trim().is_empty()
}

fn is_blank_group(item: &InventoryGroup) -> bool {
    item.id.trim().is_empty() && item.name.trim().is_empty() && item.tags.is_empty()
}

fn is_blank_role(item: &AgentRoleTemplate) -> bool {
    item.id.trim().is_empty()
        && item.name.trim().is_empty()
        && item.description.trim().is_empty()
        && item.accent_class.trim() == "accent-cyan"
        && item
            .default_title
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        && item
            .default_agent_label
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        && item
            .default_startup_command
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        && item
            .default_connection_profile_id
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        && item.default_pane_groups.is_empty()
        && item.default_reconnect_policy == crate::model::layout::ReconnectPolicy::Manual
        && item
            .default_output_helpers
            .iter()
            .all(is_blank_output_helper)
}

fn is_blank_output_helper(item: &OutputHelperRule) -> bool {
    item.id.trim().is_empty()
        && item.label.trim().is_empty()
        && item.regex.trim().is_empty()
        && item.severity == crate::model::assets::OutputSeverity::Warning
        && item.toast_on_match
}

fn is_blank_runbook(item: &Runbook) -> bool {
    item.id.trim().is_empty()
        && item.name.trim().is_empty()
        && item.description.trim().is_empty()
        && item.tags.is_empty()
        && matches!(item.target, RunbookTarget::AllPanes)
        && item.variables.iter().all(is_blank_runbook_variable)
        && item.steps.iter().all(is_blank_runbook_step)
        && item.confirm_policy == crate::model::assets::RunbookConfirmPolicy::MultiPaneOrRemote
}

fn is_blank_runbook_variable(item: &RunbookVariable) -> bool {
    item.id.trim().is_empty()
        && item.label.trim().is_empty()
        && item.description.trim().is_empty()
        && item
            .default_value
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        && item.required
}

fn is_blank_runbook_step(item: &RunbookStep) -> bool {
    item.id.trim().is_empty()
        && item.label.trim().is_empty()
        && item.command.trim().is_empty()
        && item.append_newline
}

fn is_blank_snippet(item: &CliSnippet) -> bool {
    item.id.trim().is_empty()
        && item.name.trim().is_empty()
        && item.description.trim().is_empty()
        && item.command.trim().is_empty()
        && item.tags.is_empty()
        && item.variables.iter().all(is_blank_snippet_variable)
}

fn is_blank_snippet_variable(item: &SnippetVariable) -> bool {
    item.id.trim().is_empty()
        && item.label.trim().is_empty()
        && item.description.trim().is_empty()
        && item.default_value.trim().is_empty()
}

fn validate_duplicate_ids<'a, I>(
    issues: &mut Vec<AssetValidationIssue>,
    section: AssetSection,
    ids: I,
) where
    I: Iterator<Item = &'a str>,
{
    let mut counts = HashMap::<&str, usize>::new();
    for id in ids {
        *counts.entry(id).or_insert(0) += 1;
    }
    for (id, count) in counts {
        if !id.trim().is_empty() && count > 1 {
            issues.push(AssetValidationIssue {
                section,
                item_id: Some(id.to_string()),
                message: format!("ID \"{id}\" is duplicated."),
            });
        }
    }
}

fn require_field(
    issues: &mut Vec<AssetValidationIssue>,
    section: AssetSection,
    item_id: &str,
    message: &str,
    valid: bool,
) {
    if !valid {
        issues.push(AssetValidationIssue {
            section,
            item_id: if item_id.trim().is_empty() {
                None
            } else {
                Some(item_id.to_string())
            },
            message: message.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AssetItemSource, AssetSection, effective_assets_for_scope, prune_blank_drafts, role_source,
        validate_assets,
    };
    use crate::model::assets::{
        AgentRoleTemplate, ConnectionKind, ConnectionProfile, InventoryGroup, InventoryHost,
        Runbook, RunbookConfirmPolicy, RunbookStep, RunbookTarget, WorkspaceAssets,
    };
    use crate::model::layout::ReconnectPolicy;
    use crate::model::workspace_config::ConfigScope;

    #[test]
    fn workspace_scope_merges_current_assets_over_global() {
        let global = WorkspaceAssets {
            connection_profiles: vec![ConnectionProfile {
                id: "ssh-prod".into(),
                name: "Prod".into(),
                kind: ConnectionKind::Ssh,
                inventory_host_id: Some("prod".into()),
                tags: Vec::new(),
                remote_working_directory: None,
                shell_program: None,
                startup_prefix: None,
            }],
            inventory_hosts: vec![InventoryHost {
                id: "prod".into(),
                name: "Prod".into(),
                host: "10.0.0.1".into(),
                group_ids: Vec::new(),
                tags: Vec::new(),
                provider: String::new(),
                main_ip: String::new(),
                user: String::new(),
                port: 22,
                price_per_month_usd_cents: 0,
                password_secret_ref: None,
                ssh_key_path: None,
            }],
            ..WorkspaceAssets::default()
        };
        let workspace = WorkspaceAssets {
            connection_profiles: vec![ConnectionProfile {
                id: "ssh-prod".into(),
                name: "Prod override".into(),
                kind: ConnectionKind::Ssh,
                inventory_host_id: Some("prod".into()),
                tags: Vec::new(),
                remote_working_directory: Some("/srv/app".into()),
                shell_program: None,
                startup_prefix: None,
            }],
            ..WorkspaceAssets::default()
        };

        let merged = effective_assets_for_scope(ConfigScope::Workspace, &workspace, &global);
        assert_eq!(merged.connection_profiles.len(), 1);
        assert_eq!(merged.connection_profiles[0].name, "Prod override");
    }

    #[test]
    fn validator_reports_missing_references_and_regex_errors() {
        let current = WorkspaceAssets {
            inventory_hosts: vec![InventoryHost {
                id: "host-1".into(),
                name: "Host".into(),
                host: "10.0.0.1".into(),
                group_ids: vec!["missing-group".into()],
                tags: Vec::new(),
                provider: String::new(),
                main_ip: String::new(),
                user: String::new(),
                port: 22,
                price_per_month_usd_cents: 0,
                password_secret_ref: None,
                ssh_key_path: None,
            }],
            role_templates: vec![AgentRoleTemplate {
                id: "custom-role".into(),
                name: "Custom".into(),
                description: String::new(),
                accent_class: "accent-cyan".into(),
                default_title: None,
                default_agent_label: None,
                default_startup_command: None,
                default_connection_profile_id: Some("missing-connection".into()),
                default_pane_groups: Vec::new(),
                default_reconnect_policy: ReconnectPolicy::Manual,
                default_output_helpers: vec![crate::model::assets::OutputHelperRule {
                    id: "bad".into(),
                    label: "Bad".into(),
                    regex: "(".into(),
                    severity: crate::model::assets::OutputSeverity::Error,
                    toast_on_match: true,
                }],
            }],
            runbooks: vec![Runbook {
                id: "deploy".into(),
                name: "Deploy".into(),
                description: String::new(),
                tags: Vec::new(),
                target: RunbookTarget::ConnectionProfile("missing-connection".into()),
                variables: Vec::new(),
                steps: vec![RunbookStep {
                    id: "step-1".into(),
                    label: String::new(),
                    command: String::new(),
                    append_newline: true,
                }],
                confirm_policy: RunbookConfirmPolicy::Never,
            }],
            ..WorkspaceAssets::default()
        };

        let issues = validate_assets(ConfigScope::Global, &current, &WorkspaceAssets::default());
        assert!(
            issues
                .iter()
                .any(|issue| issue.section == AssetSection::Hosts)
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.section == AssetSection::Roles)
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.section == AssetSection::Runbooks)
        );
    }

    #[test]
    fn role_source_marks_workspace_override() {
        let global = WorkspaceAssets {
            role_templates: vec![AgentRoleTemplate {
                id: "ops".into(),
                name: "Ops".into(),
                description: String::new(),
                accent_class: "accent-amber".into(),
                default_title: None,
                default_agent_label: None,
                default_startup_command: None,
                default_connection_profile_id: None,
                default_pane_groups: Vec::new(),
                default_reconnect_policy: ReconnectPolicy::Manual,
                default_output_helpers: Vec::new(),
            }],
            ..WorkspaceAssets::default()
        };
        let workspace = WorkspaceAssets {
            role_templates: vec![AgentRoleTemplate {
                id: "ops".into(),
                name: "Ops local".into(),
                description: String::new(),
                accent_class: "accent-amber".into(),
                default_title: None,
                default_agent_label: None,
                default_startup_command: None,
                default_connection_profile_id: None,
                default_pane_groups: Vec::new(),
                default_reconnect_policy: ReconnectPolicy::Manual,
                default_output_helpers: Vec::new(),
            }],
            ..WorkspaceAssets::default()
        };

        let source = role_source(ConfigScope::Workspace, "ops", &workspace, &global);
        assert_eq!(source, AssetItemSource::WorkspaceOverride);
    }

    #[test]
    fn partially_filled_group_still_reports_missing_fields() {
        let current = WorkspaceAssets {
            inventory_groups: vec![InventoryGroup {
                id: "ops".into(),
                name: String::new(),
                tags: Vec::new(),
            }],
            ..WorkspaceAssets::default()
        };

        let issues = validate_assets(ConfigScope::Global, &current, &WorkspaceAssets::default());
        assert!(
            issues
                .iter()
                .any(|issue| issue.section == AssetSection::Groups)
        );
    }

    #[test]
    fn blank_draft_host_is_ignored_until_user_fills_it() {
        let current = WorkspaceAssets {
            inventory_hosts: vec![InventoryHost {
                id: String::new(),
                name: String::new(),
                host: String::new(),
                group_ids: Vec::new(),
                tags: Vec::new(),
                provider: String::new(),
                main_ip: String::new(),
                user: String::new(),
                port: 22,
                price_per_month_usd_cents: 0,
                password_secret_ref: None,
                ssh_key_path: None,
            }],
            ..WorkspaceAssets::default()
        };

        let issues = validate_assets(ConfigScope::Global, &current, &WorkspaceAssets::default());
        assert!(issues.is_empty());

        let pruned = prune_blank_drafts(current);
        assert!(pruned.inventory_hosts.is_empty());
    }
}
