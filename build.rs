use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=resources/windows/terminaltiler.rc");
    println!("cargo:rerun-if-changed=resources/windows/terminaltiler.ico");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" || target_env != "msvc" {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let resource_dir = manifest_dir.join("resources").join("windows");
    let rc_path = resource_dir.join("terminaltiler.rc");
    let icon_path = resource_dir.join("terminaltiler.ico");
    if !rc_path.exists() || !icon_path.exists() {
        println!(
            "cargo:warning=TerminalTiler Windows icon resources are missing; skipping embedded app icon"
        );
        return;
    }

    let host = env::var("HOST").unwrap_or_default();
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let res_path = out_dir.join("terminaltiler.res");
    let Some(rc_exe) = find_resource_compiler() else {
        if host.contains("windows") {
            panic!("rc.exe was not available to embed TerminalTiler icon");
        }
        println!(
            "cargo:warning=rc.exe unavailable; skipping embedded Windows icon for non-Windows host check"
        );
        return;
    };
    let status = Command::new(&rc_exe)
        .current_dir(&resource_dir)
        .arg("/nologo")
        .arg(format!("/fo{}", res_path.display()))
        .arg(&rc_path)
        .status();

    match status {
        Ok(status) if status.success() => {
            println!(
                "cargo:rustc-link-arg-bin=terminaltiler={}",
                res_path.display()
            );
        }
        Ok(status) if host.contains("windows") => {
            panic!("rc.exe failed while embedding TerminalTiler icon: {status}");
        }
        Ok(status) => {
            println!(
                "cargo:warning=rc.exe exited with {status}; skipping embedded Windows icon for non-Windows host check"
            );
        }
        Err(error) if host.contains("windows") => {
            panic!("rc.exe was not available to embed TerminalTiler icon: {error}");
        }
        Err(error) => {
            println!(
                "cargo:warning=rc.exe unavailable ({error}); skipping embedded Windows icon for non-Windows host check"
            );
        }
    }
}

fn find_resource_compiler() -> Option<PathBuf> {
    find_on_path("rc.exe").or_else(find_windows_kit_resource_compiler)
}

fn find_on_path(binary_name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|path| path.join(binary_name))
        .find(|path| path.is_file())
}

fn find_windows_kit_resource_compiler() -> Option<PathBuf> {
    let host = env::var("HOST").unwrap_or_default();
    if !host.contains("windows") {
        return None;
    }

    let kit_arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        _ => "x64",
    };
    ["ProgramFiles(x86)", "ProgramFiles"]
        .into_iter()
        .filter_map(|name| env::var_os(name).map(PathBuf::from))
        .map(|root| root.join("Windows Kits").join("10").join("bin"))
        .find_map(|bin_root| newest_windows_kit_rc(&bin_root, kit_arch))
}

fn newest_windows_kit_rc(bin_root: &Path, kit_arch: &str) -> Option<PathBuf> {
    let mut candidates = fs::read_dir(bin_root)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join(kit_arch).join("rc.exe"))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.pop()
}
