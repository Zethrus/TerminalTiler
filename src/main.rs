#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::ffi::OsStr;
use std::path::PathBuf;

enum RuntimeCapabilitiesDestination {
    StandardOutput,
    File(PathBuf),
}

fn runtime_capabilities_destination() -> Option<Result<RuntimeCapabilitiesDestination, String>> {
    if let Some(path) = std::env::var_os("TERMINALTILER_RUNTIME_CAPABILITIES_FILE") {
        let path = PathBuf::from(path);
        return Some(
            path.is_absolute()
                .then_some(RuntimeCapabilitiesDestination::File(path))
                .ok_or_else(|| {
                    "TERMINALTILER_RUNTIME_CAPABILITIES_FILE requires an absolute output path"
                        .to_string()
                }),
        );
    }

    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == OsStr::new("--runtime-capabilities") {
            return Some(Ok(RuntimeCapabilitiesDestination::StandardOutput));
        }

        if argument == OsStr::new("--runtime-capabilities-file") {
            return Some(
                arguments
                    .next()
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .map(RuntimeCapabilitiesDestination::File)
                    .ok_or_else(|| {
                        "--runtime-capabilities-file requires an absolute output path".to_string()
                    }),
            );
        }

        if let Some(path) = argument
            .to_string_lossy()
            .strip_prefix("--runtime-capabilities-file=")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
        {
            return Some(
                path.is_absolute()
                    .then_some(RuntimeCapabilitiesDestination::File(path))
                    .ok_or_else(|| {
                        "--runtime-capabilities-file requires an absolute output path".to_string()
                    }),
            );
        }
    }

    None
}

fn write_runtime_capabilities() -> Result<bool, String> {
    let Some(destination) = runtime_capabilities_destination() else {
        return Ok(false);
    };
    let capabilities = terminaltiler::extension::runtime_capabilities_json();

    match destination? {
        RuntimeCapabilitiesDestination::StandardOutput => println!("{capabilities}"),
        RuntimeCapabilitiesDestination::File(path) => {
            std::fs::write(&path, capabilities).map_err(|error| {
                format!(
                    "could not write runtime capabilities to {}: {error}",
                    path.display()
                )
            })?
        }
    }
    Ok(true)
}

#[cfg(target_os = "linux")]
fn main() -> adw::glib::ExitCode {
    match write_runtime_capabilities() {
        Ok(true) => adw::glib::ExitCode::SUCCESS,
        Ok(false) => terminaltiler::run(),
        Err(error) => {
            eprintln!("TerminalTiler: {error}");
            adw::glib::ExitCode::FAILURE
        }
    }
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    match write_runtime_capabilities() {
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) => terminaltiler::run(),
        Err(error) => {
            eprintln!("TerminalTiler: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn main() {
    match write_runtime_capabilities() {
        Ok(true) => {}
        Ok(false) => terminaltiler::run(),
        Err(error) => eprintln!("TerminalTiler: {error}"),
    }
}
