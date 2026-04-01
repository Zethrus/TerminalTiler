use std::path::Path;
use std::process::Command;

use crate::model::layout::{TileSpec, WorkingDirectory};
use crate::platform::{home_dir, parse_wsl_unc_path, translate_path_for_wsl};
use crate::storage::session_store::SavedSession;

const DEFAULT_WSL_SHELL: &str = "/bin/bash";
const TERM_EXPORTS: &str = "export TERM=xterm-256color COLORTERM=truecolor;";
const DEFAULT_POWERSHELL_ENV: &str = "$env:TERM='xterm-256color'; $env:COLORTERM='truecolor';";

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
    pub selection_reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PowerShellRuntime {
    pub program: String,
    pub selection_reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowsRuntime {
    Wsl(WslRuntime),
    PowerShell(PowerShellRuntime),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowsLaunchRuntime {
    Wsl { distro: String },
    PowerShell { shell: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsLaunchCommand {
    pub program: String,
    pub args: Vec<String>,
    pub runtime: WindowsLaunchRuntime,
    pub working_directory: String,
}

pub fn probe_runtime(preferred_distribution: Option<&str>) -> Result<WindowsRuntime, String> {
    let wsl_attempt = query_wsl_verbose_output()
        .and_then(|output| parse_verbose_list(&output))
        .and_then(|distributions| resolve_wsl_runtime(distributions, preferred_distribution));

    match wsl_attempt {
        Ok(runtime) => Ok(WindowsRuntime::Wsl(runtime)),
        Err(wsl_error) => {
            let shell = detect_powershell_shell().ok_or_else(|| {
                format!(
                    "{wsl_error}; neither 'pwsh.exe' nor 'powershell.exe' is available for fallback"
                )
            })?;
            Ok(WindowsRuntime::PowerShell(PowerShellRuntime {
                selection_reason: format!(
                    "{wsl_error}; falling back to native PowerShell via {shell}"
                ),
                program: shell,
            }))
        }
    }
}

pub fn build_launch_command(
    tile: &TileSpec,
    workspace_root: &Path,
    runtime: &WindowsRuntime,
) -> Result<WindowsLaunchCommand, String> {
    match runtime {
        WindowsRuntime::Wsl(runtime) => build_wsl_launch_command(tile, workspace_root, runtime),
        WindowsRuntime::PowerShell(runtime) => {
            build_powershell_launch_command(tile, workspace_root, runtime)
        }
    }
}

pub fn collect_session_launch_commands(
    session: &SavedSession,
    runtime: &WindowsRuntime,
) -> Result<Vec<WindowsLaunchCommand>, String> {
    let mut commands = Vec::new();

    for tab in &session.tabs {
        for tile in tab.preset.layout.tile_specs() {
            commands.push(build_launch_command(&tile, &tab.workspace_root, runtime)?);
        }
    }

    Ok(commands)
}

#[allow(dead_code)]
pub fn spawn_launch_command(command: &WindowsLaunchCommand) -> Result<(), String> {
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
        Err("launching Windows terminal commands is only supported on Windows".into())
    }
}

impl WindowsRuntime {
    pub fn label(&self) -> String {
        match self {
            Self::Wsl(runtime) => format!(
                "WSL2: {} (WSL {}, {})",
                runtime.selected.name, runtime.selected.version, runtime.selected.state
            ),
            Self::PowerShell(runtime) => format!("PowerShell: {}", runtime.program),
        }
    }

    pub fn selection_reason(&self) -> &str {
        match self {
            Self::Wsl(runtime) => &runtime.selection_reason,
            Self::PowerShell(runtime) => &runtime.selection_reason,
        }
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

fn resolve_wsl_runtime(
    distributions: Vec<WslDistribution>,
    preferred_distribution: Option<&str>,
) -> Result<WslRuntime, String> {
    let preferred_distribution = preferred_distribution
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut preferred_error = None;

    if let Some(preferred_distribution) = preferred_distribution {
        if let Some(distribution) = distributions.iter().find(|distribution| {
            distribution
                .name
                .eq_ignore_ascii_case(preferred_distribution)
        }) {
            if distribution.version == 2 {
                let selected = distribution.clone();
                let selection_reason = format!("using configured WSL2 distro '{}'", selected.name);
                return Ok(WslRuntime {
                    distributions,
                    selected,
                    selection_reason,
                });
            }

            preferred_error = Some(format!(
                "WSL distribution '{}' is version {} but TerminalTiler requires WSL 2",
                distribution.name, distribution.version
            ));
        } else {
            preferred_error = Some(format!(
                "WSL distribution '{}' is not installed",
                preferred_distribution
            ));
        }
    }

    let fallback = distributions
        .iter()
        .find(|distribution| distribution.is_default && distribution.version == 2)
        .or_else(|| {
            distributions
                .iter()
                .find(|distribution| distribution.version == 2)
        })
        .cloned();

    if let Some(selected) = fallback {
        let selection_reason = if let Some(error) = preferred_error {
            format!("{error}; using WSL2 distro '{}'", selected.name)
        } else if selected.is_default {
            format!("using default WSL2 distro '{}'", selected.name)
        } else {
            format!("using available WSL2 distro '{}'", selected.name)
        };

        Ok(WslRuntime {
            distributions,
            selected,
            selection_reason,
        })
    } else if let Some(error) = preferred_error {
        Err(error)
    } else {
        Err("no WSL 2 distributions were found".into())
    }
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

fn detect_powershell_shell() -> Option<String> {
    ["pwsh.exe", "powershell.exe"]
        .into_iter()
        .find(|program| shell_invocation_works(program))
        .map(str::to_string)
}

fn shell_invocation_works(program: &str) -> bool {
    Command::new(program)
        .args(["-NoLogo", "-NoProfile", "-Command", "exit 0"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn build_wsl_launch_command(
    tile: &TileSpec,
    workspace_root: &Path,
    runtime: &WslRuntime,
) -> Result<WindowsLaunchCommand, String> {
    let workspace_root = translate_path_for_wsl(
        &workspace_root.display().to_string(),
        &runtime.selected.name,
    )?;
    let working_directory = resolve_wsl_working_directory(
        &tile.working_directory,
        &workspace_root,
        &runtime.selected.name,
    )?;
    let command_script =
        build_wsl_shell_script(&working_directory, tile.startup_command.as_deref());

    Ok(WindowsLaunchCommand {
        program: "wsl.exe".into(),
        args: vec![
            "--distribution".into(),
            runtime.selected.name.clone(),
            "--exec".into(),
            DEFAULT_WSL_SHELL.into(),
            "-lc".into(),
            command_script,
        ],
        runtime: WindowsLaunchRuntime::Wsl {
            distro: runtime.selected.name.clone(),
        },
        working_directory,
    })
}

fn build_powershell_launch_command(
    tile: &TileSpec,
    workspace_root: &Path,
    runtime: &PowerShellRuntime,
) -> Result<WindowsLaunchCommand, String> {
    let working_directory =
        resolve_powershell_working_directory(&tile.working_directory, workspace_root)?;
    let command_script =
        build_powershell_script(&working_directory, tile.startup_command.as_deref());

    Ok(WindowsLaunchCommand {
        program: runtime.program.clone(),
        args: vec![
            "-NoLogo".into(),
            "-NoExit".into(),
            "-Command".into(),
            command_script,
        ],
        runtime: WindowsLaunchRuntime::PowerShell {
            shell: runtime.program.clone(),
        },
        working_directory,
    })
}

fn resolve_wsl_working_directory(
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

fn resolve_powershell_working_directory(
    working_directory: &WorkingDirectory,
    workspace_root: &Path,
) -> Result<String, String> {
    let workspace_root = validate_powershell_path(&workspace_root.display().to_string())?;

    match working_directory {
        WorkingDirectory::Home => {
            let home = home_dir()
                .and_then(|path| validate_powershell_path(&path.display().to_string()).ok())
                .unwrap_or_else(|| workspace_root.clone());
            Ok(home)
        }
        WorkingDirectory::WorkspaceRoot => Ok(workspace_root),
        WorkingDirectory::Relative(path) => Ok(join_windows_path(&workspace_root, path)),
        WorkingDirectory::Absolute(path) => validate_powershell_path(&path.display().to_string()),
    }
}

fn validate_powershell_path(path: &str) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("path is empty".into());
    }
    if parse_wsl_unc_path(trimmed).is_some() || looks_like_wsl_absolute_path(trimmed) {
        return Err(format!(
            "path '{}' requires WSL and cannot be used with PowerShell fallback",
            trimmed
        ));
    }
    if !looks_like_windows_absolute_path(trimmed) {
        return Err(format!(
            "path '{}' is not a native Windows absolute path",
            trimmed
        ));
    }
    Ok(normalize_windows_path(trimmed))
}

fn build_wsl_shell_script(working_directory: &str, startup_command: Option<&str>) -> String {
    let change_dir = if working_directory == "~" {
        "cd ~".to_string()
    } else {
        format!("cd {}", shell_quote(working_directory))
    };

    if let Some(startup_command) = startup_command.filter(|value| !value.trim().is_empty()) {
        format!("{TERM_EXPORTS} {change_dir} && {startup_command}")
    } else {
        format!("{TERM_EXPORTS} {change_dir} && exec \"${{SHELL:-{DEFAULT_WSL_SHELL}}}\" -l")
    }
}

fn build_powershell_script(working_directory: &str, startup_command: Option<&str>) -> String {
    let set_location = format!(
        "Set-Location -LiteralPath {}",
        powershell_single_quote(working_directory)
    );
    if let Some(startup_command) = startup_command.filter(|value| !value.trim().is_empty()) {
        format!("& {{ {DEFAULT_POWERSHELL_ENV} {set_location}; {startup_command} }}")
    } else {
        format!("& {{ {DEFAULT_POWERSHELL_ENV} {set_location} }}")
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

fn join_windows_path(base: &str, relative: &str) -> String {
    let base = normalize_windows_path(base);
    let relative = relative.trim().trim_start_matches(['\\', '/']);
    if relative.is_empty() {
        return base;
    }
    if base.ends_with('\\') || base.ends_with('/') {
        format!("{base}{}", relative.replace('/', "\\"))
    } else {
        format!("{base}\\{}", relative.replace('/', "\\"))
    }
}

fn normalize_windows_path(path: &str) -> String {
    path.replace('/', "\\")
}

fn shell_quote(value: &str) -> String {
    let escaped = value.replace('\'', "'\"'\"'");
    format!("'{escaped}'")
}

fn powershell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn looks_like_wsl_absolute_path(path: &str) -> bool {
    let trimmed = path.trim();
    trimmed.starts_with('/') && !trimmed.starts_with("//")
}

fn looks_like_windows_absolute_path(path: &str) -> bool {
    let trimmed = path.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }

    trimmed.starts_with("\\\\") || trimmed.starts_with("//")
}

#[cfg(test)]
mod tests {
    use super::{
        PowerShellRuntime, WindowsLaunchRuntime, WindowsRuntime, WslDistribution,
        build_launch_command, build_powershell_script, build_wsl_shell_script,
        collect_session_launch_commands, parse_verbose_list, resolve_wsl_runtime,
    };
    use crate::model::layout::{ReconnectPolicy, TileSpec, WorkingDirectory};
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
            connection_target: Default::default(),
            pane_groups: Vec::new(),
            reconnect_policy: ReconnectPolicy::Manual,
            applied_role_id: None,
            output_helpers: Vec::new(),
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

    fn sample_powershell_runtime() -> WindowsRuntime {
        WindowsRuntime::PowerShell(PowerShellRuntime {
            program: "pwsh.exe".into(),
            selection_reason: "falling back to native PowerShell via pwsh.exe".into(),
        })
    }

    fn sample_wsl_runtime() -> WindowsRuntime {
        WindowsRuntime::Wsl(resolve_wsl_runtime(sample_distributions(), None).unwrap())
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
    fn resolves_requested_or_default_wsl_runtime() {
        let runtime = resolve_wsl_runtime(sample_distributions(), Some("Debian")).unwrap();
        assert_eq!(runtime.selected.name, "Debian");
        assert!(runtime.selection_reason.contains("configured"));

        let runtime = resolve_wsl_runtime(sample_distributions(), None).unwrap();
        assert_eq!(runtime.selected.name, "Ubuntu");
        assert!(runtime.selection_reason.contains("default"));
    }

    #[test]
    fn falls_back_to_default_wsl2_when_preference_is_invalid() {
        let runtime = resolve_wsl_runtime(sample_distributions(), Some("Missing")).unwrap();

        assert_eq!(runtime.selected.name, "Ubuntu");
        assert!(runtime.selection_reason.contains("Missing"));
        assert!(
            runtime
                .selection_reason
                .contains("using WSL2 distro 'Ubuntu'")
        );
    }

    #[test]
    fn rejects_when_no_wsl2_distribution_exists() {
        let distributions = vec![WslDistribution {
            name: "Ubuntu".into(),
            state: "Stopped".into(),
            version: 1,
            is_default: true,
        }];

        let error = resolve_wsl_runtime(distributions, None)
            .expect_err("WSL 1 should be rejected for runtime resolution");

        assert!(error.contains("no WSL 2 distributions"));
    }

    #[test]
    fn builds_interactive_wsl_shell_script_for_blank_startup_command() {
        assert_eq!(
            build_wsl_shell_script("/mnt/c/Users/dev/project", None),
            "export TERM=xterm-256color COLORTERM=truecolor; cd '/mnt/c/Users/dev/project' && exec \"${SHELL:-/bin/bash}\" -l"
        );
    }

    #[test]
    fn builds_interactive_powershell_script_for_blank_startup_command() {
        assert_eq!(
            build_powershell_script(r"C:\Users\dev\project", None),
            "& { $env:TERM='xterm-256color'; $env:COLORTERM='truecolor'; Set-Location -LiteralPath 'C:\\Users\\dev\\project' }"
        );
    }

    #[test]
    fn builds_wsl_launch_command_for_workspace_root() {
        let command = build_launch_command(
            &sample_tile(WorkingDirectory::WorkspaceRoot, Some("cargo test")),
            &PathBuf::from(r"C:\Users\dev\project"),
            &sample_wsl_runtime(),
        )
        .unwrap();

        assert_eq!(command.program, "wsl.exe");
        assert_eq!(
            command.runtime,
            WindowsLaunchRuntime::Wsl {
                distro: "Ubuntu".into(),
            }
        );
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
    fn builds_powershell_launch_command_for_workspace_root() {
        let command = build_launch_command(
            &sample_tile(WorkingDirectory::WorkspaceRoot, Some("cargo test")),
            &PathBuf::from(r"C:\Users\dev\project"),
            &sample_powershell_runtime(),
        )
        .unwrap();

        assert_eq!(command.program, "pwsh.exe");
        assert_eq!(
            command.runtime,
            WindowsLaunchRuntime::PowerShell {
                shell: "pwsh.exe".into(),
            }
        );
        assert_eq!(command.working_directory, r"C:\Users\dev\project");
        assert_eq!(
            command.args,
            vec![
                "-NoLogo",
                "-NoExit",
                "-Command",
                "& { $env:TERM='xterm-256color'; $env:COLORTERM='truecolor'; Set-Location -LiteralPath 'C:\\Users\\dev\\project'; cargo test }",
            ]
        );
    }

    #[test]
    fn builds_powershell_launch_command_for_relative_directories() {
        let command = build_launch_command(
            &sample_tile(WorkingDirectory::Relative("src\\tools".into()), None),
            &PathBuf::from(r"C:\Users\dev\project"),
            &sample_powershell_runtime(),
        )
        .unwrap();

        assert_eq!(command.working_directory, r"C:\Users\dev\project\src\tools");
        assert!(
            command
                .args
                .last()
                .unwrap()
                .contains(r"Set-Location -LiteralPath 'C:\Users\dev\project\src\tools'")
        );
    }

    #[test]
    fn rejects_wsl_only_paths_in_powershell_mode() {
        let error = build_launch_command(
            &sample_tile(
                WorkingDirectory::Absolute(PathBuf::from(r"\\wsl$\Ubuntu\home\dev")),
                None,
            ),
            &PathBuf::from(r"C:\Users\dev\project"),
            &sample_powershell_runtime(),
        )
        .expect_err("WSL path should fail in PowerShell mode");

        assert!(error.contains("requires WSL"));
    }

    #[test]
    fn collects_launch_commands_for_restored_session_in_both_modes() {
        let session = sample_session();

        let wsl = collect_session_launch_commands(&session, &sample_wsl_runtime()).unwrap();
        let powershell =
            collect_session_launch_commands(&session, &sample_powershell_runtime()).unwrap();

        assert_eq!(wsl.len(), 1);
        assert_eq!(powershell.len(), 1);
        assert_eq!(wsl[0].working_directory, "/mnt/c/Users/dev/project");
        assert_eq!(powershell[0].working_directory, r"C:\Users\dev\project");
    }
}
