use std::io;
use std::path::{Path, PathBuf};

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
        if crate::platform::parse_wsl_unc_path(&rendered).is_some()
            || crate::platform::looks_like_wsl_absolute_path(&rendered)
        {
            return Ok(path.to_path_buf());
        }
    }

    canonicalize_existing_dir(path)
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

#[cfg(test)]
mod tests {
    use super::{canonicalize_existing_dir, resolve_workspace_root};
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
    fn resolves_existing_workspace_root() {
        let dir = temp_dir("workspace-root");
        let nested = dir.join("nested");
        fs::create_dir_all(&nested).unwrap();

        let resolved = resolve_workspace_root(&nested).unwrap();

        assert!(resolved.is_absolute());
        assert!(resolved.ends_with("nested"));
    }
}
