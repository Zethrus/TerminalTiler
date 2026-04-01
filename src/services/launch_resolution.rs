use std::path::Path;

use crate::model::assets::{
    ConnectionKind, ConnectionProfile, InventoryHost, TileConnectionTarget, WorkspaceAssets,
};
use crate::model::layout::{TileSpec, WorkingDirectory};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolvedLaunchTransport {
    DefaultLocal,
    LocalProfile {
        profile_id: String,
        profile_name: String,
        shell_program: Option<String>,
        startup_prefix: Option<String>,
    },
    SshProfile {
        profile_id: String,
        profile_name: String,
        host_id: String,
        host_name: String,
        host: String,
        user: String,
        port: u16,
        provider: String,
        ssh_key_path: Option<String>,
        remote_working_directory: String,
        shell_program: String,
        startup_prefix: Option<String>,
    },
    WslProfile {
        profile_id: String,
        profile_name: String,
        startup_prefix: Option<String>,
    },
}

#[derive(Clone, Debug)]
pub struct ResolvedTileLaunch {
    pub connection_label: String,
    pub command: Option<String>,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub startup_command: Option<String>,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub transport: ResolvedLaunchTransport,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub remote: bool,
}

pub fn resolve_tile_launch(
    tile: &TileSpec,
    workspace_root: &Path,
    assets: &WorkspaceAssets,
) -> Result<ResolvedTileLaunch, String> {
    match &tile.connection_target {
        TileConnectionTarget::Local => Ok(ResolvedTileLaunch {
            connection_label: "local".into(),
            command: tile.startup_command.clone(),
            startup_command: tile.startup_command.clone(),
            transport: ResolvedLaunchTransport::DefaultLocal,
            remote: false,
        }),
        TileConnectionTarget::Profile(profile_id) => {
            let Some(profile) = assets
                .connection_profiles
                .iter()
                .find(|profile| profile.id == *profile_id)
            else {
                return Err(format!("Connection profile '{profile_id}' is missing."));
            };

            match profile.kind {
                ConnectionKind::Local => {
                    let startup_command = tile
                        .startup_command
                        .clone()
                        .or_else(|| profile.startup_prefix.clone());
                    Ok(ResolvedTileLaunch {
                        connection_label: profile.name.clone(),
                        command: startup_command.clone(),
                        startup_command,
                        transport: ResolvedLaunchTransport::LocalProfile {
                            profile_id: profile.id.clone(),
                            profile_name: profile.name.clone(),
                            shell_program: profile.shell_program.clone(),
                            startup_prefix: profile.startup_prefix.clone(),
                        },
                        remote: false,
                    })
                }
                ConnectionKind::Ssh => resolve_ssh_launch(tile, workspace_root, assets, profile),
                ConnectionKind::Wsl => {
                    let startup_command = tile
                        .startup_command
                        .clone()
                        .or_else(|| profile.startup_prefix.clone());
                    Ok(ResolvedTileLaunch {
                        connection_label: profile.name.clone(),
                        command: Some(build_wsl_command(tile, workspace_root, profile)),
                        startup_command,
                        transport: ResolvedLaunchTransport::WslProfile {
                            profile_id: profile.id.clone(),
                            profile_name: profile.name.clone(),
                            startup_prefix: profile.startup_prefix.clone(),
                        },
                        remote: true,
                    })
                }
            }
        }
    }
}

fn resolve_ssh_launch(
    tile: &TileSpec,
    workspace_root: &Path,
    assets: &WorkspaceAssets,
    profile: &crate::model::assets::ConnectionProfile,
) -> Result<ResolvedTileLaunch, String> {
    let Some(host_id) = profile.inventory_host_id.as_deref() else {
        return Err(format!(
            "SSH profile '{}' does not reference an inventory host.",
            profile.name
        ));
    };
    let Some(host) = assets
        .inventory_hosts
        .iter()
        .find(|host| host.id == host_id)
    else {
        return Err(format!(
            "SSH profile '{}' references missing host '{}'.",
            profile.name, host_id
        ));
    };

    let login = ssh_login(host);
    let mut parts = vec!["ssh".to_string()];
    if host.port != 22 {
        parts.push("-p".into());
        parts.push(host.port.to_string());
    }
    if let Some(key_path) = host
        .ssh_key_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
    {
        parts.push("-i".into());
        parts.push(shell_quote(key_path));
    }
    parts.push(shell_quote(&login));

    let remote_cwd = resolve_remote_working_directory(tile, profile);
    let remote_shell = profile
        .shell_program
        .clone()
        .unwrap_or_else(|| "bash".into());
    let remote_command = tile
        .startup_command
        .clone()
        .or_else(|| profile.startup_prefix.clone());
    let remote_script = match remote_command.as_deref() {
        Some(command) if !command.trim().is_empty() => format!(
            "cd {} && {} ; exec {} -l",
            shell_quote(&remote_cwd),
            command,
            shell_quote(&remote_shell)
        ),
        _ => format!(
            "cd {} && exec {} -l",
            shell_quote(&remote_cwd),
            shell_quote(&remote_shell)
        ),
    };
    parts.push("-t".into());
    parts.push(shell_quote(&remote_script));

    let connection_label = if host.provider.trim().is_empty() {
        profile.name.clone()
    } else {
        format!("{} ({})", profile.name, host.provider.trim())
    };

    let _ = workspace_root;
    Ok(ResolvedTileLaunch {
        connection_label,
        command: Some(parts.join(" ")),
        startup_command: remote_command,
        transport: ResolvedLaunchTransport::SshProfile {
            profile_id: profile.id.clone(),
            profile_name: profile.name.clone(),
            host_id: host.id.clone(),
            host_name: host.name.clone(),
            host: host.host.clone(),
            user: host.user.clone(),
            port: host.port,
            provider: host.provider.clone(),
            ssh_key_path: host.ssh_key_path.clone(),
            remote_working_directory: remote_cwd,
            shell_program: remote_shell,
            startup_prefix: profile.startup_prefix.clone(),
        },
        remote: true,
    })
}

fn resolve_remote_working_directory(tile: &TileSpec, profile: &ConnectionProfile) -> String {
    profile
        .remote_working_directory
        .clone()
        .unwrap_or_else(|| match &tile.working_directory {
            WorkingDirectory::WorkspaceRoot => ".".into(),
            WorkingDirectory::Relative(path) => path.clone(),
            WorkingDirectory::Absolute(path) => path.display().to_string(),
            WorkingDirectory::Home => "~".into(),
        })
}

fn ssh_login(host: &InventoryHost) -> String {
    if host.user.trim().is_empty() {
        host.host.clone()
    } else {
        format!("{}@{}", host.user.trim(), host.host.trim())
    }
}

fn build_wsl_command(
    tile: &TileSpec,
    workspace_root: &Path,
    profile: &crate::model::assets::ConnectionProfile,
) -> String {
    let distro = profile.name.clone();
    let command = tile
        .startup_command
        .clone()
        .or_else(|| profile.startup_prefix.clone())
        .unwrap_or_else(|| "bash".into());
    let working_directory = match &tile.working_directory {
        WorkingDirectory::WorkspaceRoot => workspace_root.display().to_string(),
        WorkingDirectory::Relative(path) => workspace_root.join(path).display().to_string(),
        WorkingDirectory::Absolute(path) => path.display().to_string(),
        WorkingDirectory::Home => "~".into(),
    };
    format!(
        "wsl.exe -d {} --cd {} -- {}",
        shell_quote(&distro),
        shell_quote(&working_directory),
        command
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{ResolvedLaunchTransport, resolve_tile_launch};
    use crate::model::assets::{
        ConnectionKind, ConnectionProfile, InventoryHost, TileConnectionTarget, WorkspaceAssets,
    };
    use crate::model::layout::{ReconnectPolicy, TileSpec, WorkingDirectory};

    fn tile() -> TileSpec {
        TileSpec {
            id: "tile-1".into(),
            title: "Tile 1".into(),
            agent_label: "Shell".into(),
            accent_class: "accent-cyan".into(),
            working_directory: WorkingDirectory::WorkspaceRoot,
            startup_command: Some("echo hello".into()),
            connection_target: TileConnectionTarget::Local,
            pane_groups: Vec::new(),
            reconnect_policy: ReconnectPolicy::Manual,
            applied_role_id: None,
            output_helpers: Vec::new(),
        }
    }

    #[test]
    fn resolves_local_launch() {
        let resolved = resolve_tile_launch(&tile(), Path::new("/tmp"), &WorkspaceAssets::default())
            .expect("local launch should resolve");
        assert_eq!(resolved.connection_label, "local");
        assert_eq!(resolved.command.as_deref(), Some("echo hello"));
        assert_eq!(resolved.startup_command.as_deref(), Some("echo hello"));
        assert_eq!(resolved.transport, ResolvedLaunchTransport::DefaultLocal);
        assert!(!resolved.remote);
    }

    #[test]
    fn resolves_ssh_launch_from_profile_and_inventory() {
        let mut tile = tile();
        tile.connection_target = TileConnectionTarget::Profile("prod".into());
        let assets = WorkspaceAssets {
            connection_profiles: vec![ConnectionProfile {
                id: "prod".into(),
                name: "Prod".into(),
                kind: ConnectionKind::Ssh,
                inventory_host_id: Some("host-1".into()),
                tags: Vec::new(),
                remote_working_directory: Some("/srv/app".into()),
                shell_program: Some("bash".into()),
                startup_prefix: None,
            }],
            inventory_hosts: vec![InventoryHost {
                id: "host-1".into(),
                name: "Prod Box".into(),
                host: "prod.example.com".into(),
                group_ids: Vec::new(),
                tags: Vec::new(),
                provider: "hetzner".into(),
                main_ip: "192.0.2.10".into(),
                user: "deploy".into(),
                port: 22,
                price_per_month_usd_cents: 1500,
                password_secret_ref: None,
                ssh_key_path: Some("~/.ssh/id_ed25519".into()),
            }],
            inventory_groups: Vec::new(),
            role_templates: Vec::new(),
            runbooks: Vec::new(),
        };
        let resolved = resolve_tile_launch(&tile, Path::new("/workspace"), &assets)
            .expect("ssh launch should resolve");
        assert!(resolved.connection_label.contains("Prod"));
        assert!(resolved.command.unwrap_or_default().contains("ssh"));
        assert!(matches!(
            resolved.transport,
            ResolvedLaunchTransport::SshProfile {
                ref profile_id,
                ref host_id,
                ..
            } if profile_id == "prod" && host_id == "host-1"
        ));
        assert!(resolved.remote);
    }

    #[test]
    fn reports_missing_profile() {
        let mut tile = tile();
        tile.connection_target = TileConnectionTarget::Profile("missing".into());
        let error = resolve_tile_launch(&tile, Path::new("/tmp"), &WorkspaceAssets::default())
            .expect_err("missing profile should error");
        assert!(error.contains("missing"));
    }
}
