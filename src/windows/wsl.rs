use std::path::Path;
use std::process::Command;

use crate::model::layout::{TileSpec, WorkingDirectory};
use crate::platform::translate_path_for_wsl;
use crate::storage::session_store::SavedSession;

const DEFAULT_WSL_SHELL: &str = "/bin/bash";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WslDistribution {
    pub name: String,
    pub state: String,
    pub version: u8,
    pub is_default: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WslRuntime {
    pub distributions: Vec<WslDistribution>,
    pub selected: WslDistribution,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WslLaunchCommand {
    pub program: String,
    pub args: Vec<String>,
    pub distro: String,
    pub working_directory: String,
}

pub fn probe_runtime(preferred_distribution: Option<&str>) -> Result<WslRuntime, String> {
    let output = query_wsl_verbose_output()?;
    let distributions = parse_verbose_list(&output)?;
    let selected = resolve_distribution(&distributions, preferred_distribution)?.clone();

    Ok(WslRuntime {
        distributions,
        selected,
    })
}

pub fn build_launch_command(
    tile: &TileSpec,
    workspace_root: &Path,
    distribution: &str,
) -> Result<WslLaunchCommand, String> {
    let workspace_root =
        translate_path_for_wsl(&workspace_root.display().to_string(), distribution)?;
    let working_directory =
        resolve_working_directory(&tile.working_directory, &workspace_root, distribution)?;
    let command_script = build_shell_script(&working_directory, tile.startup_command.as_deref());

    Ok(WslLaunchCommand {
        program: "wsl.exe".into(),
        args: vec![
            "--distribution".into(),
            distribution.into(),
            "--exec".into(),
            DEFAULT_WSL_SHELL.into(),
            "-lc".into(),
            command_script,
        ],
        distro: distribution.into(),
        working_directory,
    })
}

pub fn collect_session_launch_commands(
    session: &SavedSession,
    distribution: &str,
) -> Result<Vec<WslLaunchCommand>, String> {
    let mut commands = Vec::new();

    for tab in &session.tabs {
        for tile in tab.preset.layout.tile_specs() {
            commands.push(build_launch_command(
                &tile,
                &tab.workspace_root,
                distribution,
            )?);
        }
    }

    Ok(commands)
}

pub fn spawn_launch_command(command: &WslLaunchCommand) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::CREATE_NEW_CONSOLE;

        Command::new(&command.program)
            .args(&command.args)
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()
            .map(|_| ())
            .map_err(|error| {
                format!(
                    "failed to spawn '{}' with args {:?}: {}",
                    command.program, command.args, error
                )
            })
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = command;
        Err("launching WSL commands is only supported on Windows".into())
    }
}

pub fn parse_verbose_list(output: &str) -> Result<Vec<WslDistribution>, String> {
    let mut distributions = Vec::new();

    for raw_line in output.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("NAME") {
            continue;
        }

        let is_default = line.starts_with('*');
        let body = if is_default {
            line.trim_start_matches('*').trim_start()
        } else {
            line
        };
        let parts = body.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 3 {
            continue;
        }

        let version = parts[parts.len() - 1].parse::<u8>().map_err(|error| {
            format!(
                "could not parse WSL version from '{}': {}",
                raw_line.trim(),
                error
            )
        })?;
        let state = parts[parts.len() - 2].to_string();
        let name = parts[..parts.len() - 2].join(" ");
        if name.is_empty() {
            continue;
        }

        distributions.push(WslDistribution {
            name,
            state,
            version,
            is_default,
        });
    }

    if distributions.is_empty() {
        Err("no WSL distributions were found".into())
    } else {
        Ok(distributions)
    }
}

pub fn resolve_distribution<'a>(
    distributions: &'a [WslDistribution],
    preferred_distribution: Option<&str>,
) -> Result<&'a WslDistribution, String> {
    let selected = if let Some(preferred_distribution) = preferred_distribution
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        distributions
            .iter()
            .find(|distribution| {
                distribution
                    .name
                    .eq_ignore_ascii_case(preferred_distribution)
            })
            .ok_or_else(|| {
                format!(
                    "WSL distribution '{}' is not installed",
                    preferred_distribution
                )
            })?
    } else {
        distributions
            .iter()
            .find(|distribution| distribution.is_default)
            .or_else(|| distributions.first())
            .ok_or_else(|| "no WSL distributions were found".to_string())?
    };

    if selected.version != 2 {
        return Err(format!(
            "WSL distribution '{}' is version {} but TerminalTiler requires WSL 2",
            selected.name, selected.version
        ));
    }

    Ok(selected)
}

fn query_wsl_verbose_output() -> Result<String, String> {
    let output = Command::new("wsl.exe")
        .args(["--list", "--verbose"])
        .output()
        .map_err(|error| format!("failed to start wsl.exe: {error}"))?;

    if output.status.success() {
        String::from_utf8(output.stdout)
            .map_err(|error| format!("WSL output was not valid UTF-8: {error}"))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(if stderr.trim().is_empty() {
            "wsl.exe exited with a non-zero status".into()
        } else {
            stderr.trim().to_string()
        })
    }
}

fn resolve_working_directory(
    working_directory: &WorkingDirectory,
    workspace_root: &str,
    distribution: &str,
) -> Result<String, String> {
    match working_directory {
        WorkingDirectory::Home => Ok("~".into()),
        WorkingDirectory::WorkspaceRoot => Ok(workspace_root.into()),
        WorkingDirectory::Relative(path) => Ok(join_posix(workspace_root, path)),
        WorkingDirectory::Absolute(path) => {
            translate_path_for_wsl(&path.display().to_string(), distribution)
        }
    }
}

fn build_shell_script(working_directory: &str, startup_command: Option<&str>) -> String {
    let exports = "export TERM=xterm-256color COLORTERM=truecolor;";
    let change_dir = if working_directory == "~" {
        "cd ~".to_string()
    } else {
        format!("cd {}", shell_quote(working_directory))
    };

    if let Some(startup_command) = startup_command.filter(|value| !value.trim().is_empty()) {
        format!("{exports} {change_dir} && {startup_command}")
    } else {
        format!("{exports} {change_dir} && exec \"${{SHELL:-{DEFAULT_WSL_SHELL}}}\" -l")
    }
}

fn join_posix(base: &str, relative: &str) -> String {
    let trimmed_base = base.trim_end_matches('/');
    let trimmed_relative = relative.trim_start_matches('/');
    if trimmed_relative.is_empty() {
        trimmed_base.to_string()
    } else if trimmed_base.is_empty() {
        format!("/{trimmed_relative}")
    } else {
        format!("{trimmed_base}/{trimmed_relative}")
    }
}

fn shell_quote(value: &str) -> String {
    let escaped = value.replace('\'', "'\"'\"'");
    format!("'{escaped}'")
}

#[cfg(test)]
mod tests {
    use super::{
        WslDistribution, build_launch_command, build_shell_script, collect_session_launch_commands,
        parse_verbose_list, resolve_distribution,
    };
    use crate::model::layout::TileSpec;
    use crate::model::layout::WorkingDirectory;
    use crate::model::preset::{ApplicationDensity, ThemeMode, WorkspacePreset};
    use crate::storage::session_store::{SavedSession, SavedTab};
    use std::path::PathBuf;

    fn sample_distributions() -> Vec<WslDistribution> {
        vec![
            WslDistribution {
                name: "Ubuntu".into(),
                state: "Running".into(),
                version: 2,
                is_default: true,
            },
            WslDistribution {
                name: "Debian".into(),
                state: "Stopped".into(),
                version: 2,
                is_default: false,
            },
        ]
    }

    fn sample_tile(working_directory: WorkingDirectory, startup_command: Option<&str>) -> TileSpec {
        TileSpec {
            id: "tile-1".into(),
            title: "Primary".into(),
            agent_label: "Shell".into(),
            accent_class: "accent-cyan".into(),
            working_directory,
            startup_command: startup_command.map(str::to_owned),
        }
    }

    fn sample_session() -> SavedSession {
        SavedSession {
            tabs: vec![SavedTab {
                preset: WorkspacePreset {
                    id: "preset-1".into(),
                    name: "Sample".into(),
                    description: String::new(),
                    tags: Vec::new(),
                    root_label: "Workspace root".into(),
                    theme: ThemeMode::System,
                    density: ApplicationDensity::Compact,
                    layout: crate::model::layout::tile(
                        "tile-1",
                        "Primary",
                        "Shell",
                        "accent-cyan",
                        WorkingDirectory::WorkspaceRoot,
                        Some("cargo test"),
                    ),
                },
                workspace_root: PathBuf::from(r"C:\Users\dev\project"),
                custom_title: None,
                terminal_zoom_steps: 0,
            }],
            active_tab_index: 0,
        }
    }

    #[test]
    fn parses_verbose_wsl_list_output() {
        let parsed = parse_verbose_list(
            "  NAME                   STATE           VERSION\n* Ubuntu                 Running         2\n  Debian                 Stopped         2\n",
        )
        .unwrap();

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "Ubuntu");
        assert!(parsed[0].is_default);
        assert_eq!(parsed[1].state, "Stopped");
    }

    #[test]
    fn resolves_requested_or_default_distribution() {
        let distributions = sample_distributions();

        assert_eq!(
            resolve_distribution(&distributions, Some("Debian"))
                .unwrap()
                .name,
            "Debian"
        );
        assert_eq!(
            resolve_distribution(&distributions, None).unwrap().name,
            "Ubuntu"
        );
    }

    #[test]
    fn rejects_wsl1_distribution_selection() {
        let distributions = vec![WslDistribution {
            name: "Ubuntu".into(),
            state: "Stopped".into(),
            version: 1,
            is_default: true,
        }];

        let error =
            resolve_distribution(&distributions, None).expect_err("WSL 1 should be rejected");

        assert!(error.contains("requires WSL 2"));
    }

    #[test]
    fn builds_interactive_shell_script_for_blank_startup_command() {
        assert_eq!(
            build_shell_script("/mnt/c/Users/dev/project", None),
            "export TERM=xterm-256color COLORTERM=truecolor; cd '/mnt/c/Users/dev/project' && exec \"${SHELL:-/bin/bash}\" -l"
        );
    }

    #[test]
    fn builds_launch_command_for_workspace_root() {
        let command = build_launch_command(
            &sample_tile(WorkingDirectory::WorkspaceRoot, Some("cargo test")),
            &PathBuf::from(r"C:\Users\dev\project"),
            "Ubuntu",
        )
        .unwrap();

        assert_eq!(command.program, "wsl.exe");
        assert_eq!(command.distro, "Ubuntu");
        assert_eq!(command.working_directory, "/mnt/c/Users/dev/project");
        assert_eq!(
            command.args,
            vec![
                "--distribution",
                "Ubuntu",
                "--exec",
                "/bin/bash",
                "-lc",
                "export TERM=xterm-256color COLORTERM=truecolor; cd '/mnt/c/Users/dev/project' && cargo test",
            ]
        );
    }

    #[test]
    fn builds_launch_command_for_relative_and_home_directories() {
        let relative = build_launch_command(
            &sample_tile(WorkingDirectory::Relative("src".into()), None),
            &PathBuf::from(r"C:\Users\dev\project"),
            "Ubuntu",
        )
        .unwrap();
        assert_eq!(relative.working_directory, "/mnt/c/Users/dev/project/src");

        let home = build_launch_command(
            &sample_tile(WorkingDirectory::Home, None),
            &PathBuf::from(r"C:\Users\dev\project"),
            "Ubuntu",
        )
        .unwrap();
        assert_eq!(home.working_directory, "~");
        assert!(home.args.last().unwrap().contains("cd ~"));
    }

    #[test]
    fn rejects_cross_distro_absolute_unc_paths() {
        let error = build_launch_command(
            &sample_tile(
                WorkingDirectory::Absolute(PathBuf::from(r"\\wsl$\Debian\home\dev")),
                None,
            ),
            &PathBuf::from(r"C:\Users\dev\project"),
            "Ubuntu",
        )
        .expect_err("cross-distro absolute path should fail");

        assert!(error.contains("Debian"));
    }

    #[test]
    fn collects_launch_commands_for_restored_session() {
        let commands = collect_session_launch_commands(&sample_session(), "Ubuntu").unwrap();

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].distro, "Ubuntu");
        assert_eq!(commands[0].working_directory, "/mnt/c/Users/dev/project");
    }
}
