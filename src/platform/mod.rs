use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WslUncPath {
    pub distro: String,
    pub path: String,
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(windows_home_dir)
}

pub fn canonicalize_existing_dir(path: &Path) -> io::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    if !canonical.is_dir() {
        return Err(io::Error::other(format!(
            "path '{}' is not a directory",
            canonical.display()
        )));
    }
    Ok(canonical)
}

pub fn resolve_workspace_root(path: &Path) -> io::Result<PathBuf> {
    #[cfg(windows)]
    {
        let rendered = path.display().to_string();
        if parse_wsl_unc_path(&rendered).is_some() || looks_like_wsl_absolute_path(&rendered) {
            return Ok(path.to_path_buf());
        }
    }

    canonicalize_existing_dir(path)
}

pub fn translate_path_for_wsl(path: &str, expected_distro: &str) -> Result<String, String> {
    if path.trim().is_empty() {
        return Err("path is empty".into());
    }

    if let Some(unc) = parse_wsl_unc_path(path) {
        if !unc.distro.eq_ignore_ascii_case(expected_distro) {
            return Err(format!(
                "WSL UNC path targets distro '{}' instead of '{}'",
                unc.distro, expected_distro
            ));
        }
        return Ok(unc.path);
    }

    if looks_like_wsl_absolute_path(path) {
        return Ok(path.replace('\\', "/"));
    }

    if let Some(wsl_path) = translate_windows_drive_path(path) {
        return Ok(wsl_path);
    }

    Err(format!("unsupported WSL path '{}'", path))
}

pub fn parse_wsl_unc_path(path: &str) -> Option<WslUncPath> {
    let trimmed = path.trim();
    let normalized = trimmed.replace('/', "\\");
    let prefix = "\\\\wsl$\\";
    let suffix = normalized.strip_prefix(prefix)?;
    let (distro, remainder) = suffix.split_once('\\')?;
    let remainder = remainder.trim_start_matches('\\');

    Some(WslUncPath {
        distro: distro.to_string(),
        path: if remainder.is_empty() {
            "/".into()
        } else {
            format!("/{}", remainder.replace('\\', "/"))
        },
    })
}

fn windows_home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                let drive = std::env::var_os("HOMEDRIVE")?;
                let path = std::env::var_os("HOMEPATH")?;
                if drive.is_empty() || path.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(format!(
                        "{}{}",
                        drive.to_string_lossy(),
                        path.to_string_lossy()
                    )))
                }
            })
    }

    #[cfg(not(windows))]
    {
        None
    }
}

fn looks_like_wsl_absolute_path(path: &str) -> bool {
    let trimmed = path.trim();
    trimmed.starts_with('/') && !trimmed.starts_with("//")
}

fn translate_windows_drive_path(path: &str) -> Option<String> {
    let trimmed = path.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || (bytes[2] != b'\\' && bytes[2] != b'/')
    {
        return None;
    }

    let drive = (bytes[0] as char).to_ascii_lowercase();
    let remainder = trimmed[3..].replace('\\', "/");
    if remainder.is_empty() {
        Some(format!("/mnt/{drive}"))
    } else {
        Some(format!("/mnt/{drive}/{remainder}"))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        canonicalize_existing_dir, parse_wsl_unc_path, resolve_workspace_root,
        translate_path_for_wsl,
    };
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn temp_dir(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("terminaltiler-{prefix}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn canonicalizes_existing_directories() {
        let dir = temp_dir("canonicalize");
        let nested = dir.join("nested");
        fs::create_dir_all(&nested).unwrap();

        let resolved = canonicalize_existing_dir(&nested).unwrap();

        assert!(resolved.is_absolute());
        assert!(resolved.ends_with("nested"));
    }

    #[test]
    fn parses_matching_wsl_unc_paths() {
        let unc = parse_wsl_unc_path(r"\\wsl$\Ubuntu\home\dev\project").unwrap();

        assert_eq!(unc.distro, "Ubuntu");
        assert_eq!(unc.path, "/home/dev/project");
    }

    #[test]
    fn translates_windows_drive_paths_to_wsl_mounts() {
        assert_eq!(
            translate_path_for_wsl(r"C:\Users\dev\project", "Ubuntu").unwrap(),
            "/mnt/c/Users/dev/project"
        );
        assert_eq!(
            translate_path_for_wsl("D:/work/tree", "Ubuntu").unwrap(),
            "/mnt/d/work/tree"
        );
    }

    #[test]
    fn preserves_raw_wsl_absolute_paths() {
        assert_eq!(
            translate_path_for_wsl("/home/dev/project", "Ubuntu").unwrap(),
            "/home/dev/project"
        );
    }

    #[test]
    fn rejects_cross_distro_unc_paths() {
        let error = translate_path_for_wsl(r"\\wsl$\Debian\home\dev", "Ubuntu")
            .expect_err("cross-distro path should fail");

        assert!(error.contains("Debian"));
        assert!(error.contains("Ubuntu"));
    }

    #[test]
    fn resolves_existing_workspace_root() {
        let dir = temp_dir("workspace-root");
        let nested = dir.join("nested");
        fs::create_dir_all(&nested).unwrap();

        let resolved = resolve_workspace_root(&nested).unwrap();

        assert!(resolved.is_absolute());
        assert!(resolved.ends_with("nested"));
    }
}
