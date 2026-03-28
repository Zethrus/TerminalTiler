use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

const PRIVATE_FILE_MODE: u32 = 0o600;

pub fn atomic_write_private(path: &Path, contents: &str) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::other(format!("path '{}' does not have a parent", path.display()))
    })?;
    fs::create_dir_all(parent)?;

    let temp_path = sibling_temp_path(path);
    let mut file = new_private_file(&temp_path)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    drop(file);

    fs::rename(&temp_path, path)?;
    sync_dir(parent);
    Ok(())
}

pub fn preserve_corrupt_file(path: &Path) -> io::Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }

    let file_name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".into());
    let preserved =
        path.with_file_name(format!("{}.corrupt-{}", file_name, Uuid::new_v4().simple()));
    fs::rename(path, &preserved)?;
    sync_dir(
        preserved
            .parent()
            .ok_or_else(|| io::Error::other("preserved document does not have a parent"))?,
    );
    Ok(Some(preserved))
}

pub fn canonicalize_existing_dir(path: &Path) -> io::Result<PathBuf> {
    let canonical = fs::canonicalize(path)?;
    if !canonical.is_dir() {
        return Err(io::Error::other(format!(
            "path '{}' is not a directory",
            canonical.display()
        )));
    }
    Ok(canonical)
}

fn sibling_temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".into());
    path.with_file_name(format!(".{}.tmp-{}", file_name, Uuid::new_v4().simple()))
}

fn new_private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(PRIVATE_FILE_MODE);
    options.open(path)
}

fn sync_dir(path: &Path) {
    if let Ok(dir) = File::open(path) {
        let _ = dir.sync_all();
    }
}

#[cfg(test)]
mod tests {
    use super::{atomic_write_private, canonicalize_existing_dir, preserve_corrupt_file};
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn temp_dir(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("terminaltiler-{prefix}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn atomically_writes_text_file() {
        let dir = temp_dir("atomic-write");
        let path = dir.join("state.toml");

        atomic_write_private(&path, "hello = 'world'\n").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "hello = 'world'\n");
    }

    #[test]
    fn preserves_corrupt_file_by_renaming_it() {
        let dir = temp_dir("preserve-corrupt");
        let path = dir.join("session.toml");
        fs::write(&path, "broken").unwrap();

        let preserved = preserve_corrupt_file(&path).unwrap().unwrap();

        assert!(!path.exists());
        assert_eq!(fs::read_to_string(preserved).unwrap(), "broken");
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
}
