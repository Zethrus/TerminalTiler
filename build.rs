use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=resources/windows/terminaltiler.rc");
    println!("cargo:rerun-if-changed=resources/windows/terminaltiler.ico");
    println!("cargo:rerun-if-env-changed=PACKAGE_VERSION");

    // Packaging resolves a release tag independently from Cargo.toml's base
    // development version.  Embed that resolved identity in every binary so
    // About, capability probes, and the updater all agree on the artifact.
    let package_version = env::var("PACKAGE_VERSION")
        .ok()
        .filter(|version| !version.trim().is_empty())
        .unwrap_or_else(|| env::var("CARGO_PKG_VERSION").expect("Cargo supplies package version"));
    println!("cargo:rustc-env=TERMINALTILER_PACKAGE_VERSION={package_version}");

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
    let version_rc_path = out_dir.join("terminaltiler-version.rc");
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
    if let Err(error) = fs::write(
        &version_rc_path,
        windows_version_resource(&icon_path, &package_version),
    ) {
        panic!("could not write TerminalTiler Windows version resource: {error}");
    }

    let status = Command::new(&rc_exe)
        .current_dir(&resource_dir)
        .arg("/nologo")
        .arg(format!("/fo{}", res_path.display()))
        .arg(&version_rc_path)
        .status();

    match status {
        Ok(status) if status.success() => {
            // Link the resource into every binary target.  This avoids
            // coupling the resource to Cargo's implicit package-binary name
            // and preserves the icon/version metadata for shipped helpers.
            println!("cargo:rustc-link-arg-bins={}", res_path.display());
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

fn windows_version_resource(icon_path: &Path, package_version: &str) -> String {
    let escaped_icon_path = icon_path.display().to_string().replace('\\', "/");
    let mut components = package_version.split('.').map(|component| {
        component
            .parse::<u16>()
            .expect("PACKAGE_VERSION must be numeric")
    });
    let major = components
        .next()
        .expect("PACKAGE_VERSION must include major");
    let minor = components
        .next()
        .expect("PACKAGE_VERSION must include minor");
    let patch = components
        .next()
        .expect("PACKAGE_VERSION must include patch");
    assert!(
        components.next().is_none(),
        "PACKAGE_VERSION must be semver"
    );

    format!(
        r#"1 ICON "{}"
VS_VERSION_INFO VERSIONINFO
 FILEVERSION {major},{minor},{patch},0
 PRODUCTVERSION {major},{minor},{patch},0
 FILEFLAGSMASK 0x3fL
 FILEFLAGS 0x0L
 FILEOS 0x40004L
 FILETYPE 0x1L
 FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904B0"
        BEGIN
            VALUE "FileVersion", "{package_version}\0"
            VALUE "ProductVersion", "{package_version}\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x0409, 1200
    END
END
"#,
        escaped_icon_path
    )
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

#[cfg(test)]
mod tests {
    use super::windows_version_resource;
    use std::path::Path;

    #[test]
    fn version_resource_embeds_the_resolved_package_version() {
        let resource = windows_version_resource(Path::new(r"C:\icons\terminaltiler.ico"), "1.2.3");

        assert!(resource.contains(r#"1 ICON "C:/icons/terminaltiler.ico""#));
        assert!(resource.contains("FILEVERSION 1,2,3,0"));
        assert!(resource.contains("PRODUCTVERSION 1,2,3,0"));
        assert!(resource.contains(r#"VALUE "FileVersion", "1.2.3\0""#));
        assert!(resource.contains(r#"VALUE "ProductVersion", "1.2.3\0""#));
    }
}
