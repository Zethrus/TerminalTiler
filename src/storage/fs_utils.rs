use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

#[cfg(unix)]
const PRIVATE_FILE_MODE: u32 = 0o600;

fn persistence_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub fn with_persistence_lock<T>(operation: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    let _guard = persistence_lock()
        .lock()
        .map_err(|_| io::Error::other("persistence lock is poisoned"))?;
    operation()
}

pub fn atomic_write_private(path: &Path, contents: &str) -> io::Result<()> {
    with_persistence_lock(|| atomic_write_private_unlocked(path, contents))
}

pub(crate) fn atomic_write_private_unlocked(path: &Path, contents: &str) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::other(format!("path '{}' does not have a parent", path.display()))
    })?;
    fs::create_dir_all(parent)?;

    let temp_path = sibling_temp_path(path);
    let mut file = new_private_file(&temp_path)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    drop(file);

    replace_file(&temp_path, path)?;
    sync_dir(parent);
    Ok(())
}

/// Stage every document before committing any target. If a later rename fails,
/// already committed targets are restored from their in-memory backups.
#[cfg(test)]
pub(crate) fn transactional_write_private_unlocked(writes: &[(PathBuf, String)]) -> io::Result<()> {
    transactional_apply_private_unlocked(writes, &[])
}

/// Apply a mixed set of writes and removals as one filesystem transaction.
///
/// Every replacement is staged before the first target changes. Existing
/// files are retained either in memory (writes) or as same-directory rollback
/// files (removals) until the complete commit succeeds. This is intentionally
/// lock-free: callers must hold the shared persistence lock for the whole
/// preflight/stage/commit sequence.
pub(crate) fn transactional_apply_private_unlocked(
    writes: &[(PathBuf, String)],
    removals: &[PathBuf],
) -> io::Result<()> {
    let mut targets = std::collections::BTreeSet::new();
    for target in writes
        .iter()
        .map(|(target, _)| target)
        .chain(removals.iter())
    {
        if !targets.insert(target.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("duplicate transaction target '{}'", target.display()),
            ));
        }
        if target.exists() && !target.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("sync target '{}' is not a regular file", target.display()),
            ));
        }
    }

    let mut staged = Vec::with_capacity(writes.len());
    for (target, contents) in writes {
        let parent = target.parent().ok_or_else(|| {
            io::Error::other(format!(
                "path '{}' does not have a parent",
                target.display()
            ))
        })?;
        fs::create_dir_all(parent)?;
        let stage = sibling_temp_path(target);
        let mut file = new_private_file(&stage)?;
        if let Err(error) = file
            .write_all(contents.as_bytes())
            .and_then(|_| file.sync_all())
        {
            let _ = fs::remove_file(&stage);
            for (_, staged_path, _) in &staged {
                let _ = fs::remove_file(staged_path);
            }
            return Err(error);
        }
        let backup = if target.is_file() {
            match fs::read(target) {
                Ok(bytes) => Some(bytes),
                Err(error) => {
                    let _ = fs::remove_file(&stage);
                    for (_, staged_path, _) in &staged {
                        let _ = fs::remove_file(staged_path);
                    }
                    return Err(error);
                }
            }
        } else {
            None
        };
        staged.push((target.clone(), stage, backup));
    }

    let mut moved_removals = Vec::new();
    for target in removals {
        if !target.exists() {
            continue;
        }
        let backup = sibling_temp_path(target);
        if let Err(error) = fs::rename(target, &backup) {
            for (original, previous_backup) in moved_removals.iter().rev() {
                let _ = fs::rename(previous_backup, original);
            }
            for (_, stage, _) in &staged {
                let _ = fs::remove_file(stage);
            }
            return Err(error);
        }
        moved_removals.push((target.clone(), backup));
    }

    for index in 0..staged.len() {
        let (target, stage, _) = &staged[index];
        if let Err(error) = replace_file(stage, target) {
            for (committed, _, backup) in staged[..index].iter().rev() {
                match backup {
                    Some(bytes) => {
                        let restored = String::from_utf8_lossy(bytes);
                        let _ = atomic_write_private_unlocked(committed, &restored);
                    }
                    None => {
                        let _ = fs::remove_file(committed);
                    }
                }
            }
            for (_, remaining_stage, _) in &staged[index..] {
                let _ = fs::remove_file(remaining_stage);
            }
            for (original, backup) in moved_removals.iter().rev() {
                let _ = fs::rename(backup, original);
            }
            return Err(error);
        }
        if let Some(parent) = target.parent() {
            sync_dir(parent);
        }
    }
    for (target, backup) in moved_removals {
        let _ = fs::remove_file(backup);
        if let Some(parent) = target.parent() {
            sync_dir(parent);
        }
    }
    Ok(())
}

/// Replace `target` with the staged sibling file without exposing a window
/// where the destination is absent. Unix `rename(2)` already has replacement
/// semantics. Windows' Rust `fs::rename` does not, so use `MoveFileExW` with
/// `MOVEFILE_REPLACE_EXISTING` for the equivalent same-volume operation.
#[cfg(not(windows))]
fn replace_file(staged: &Path, target: &Path) -> io::Result<()> {
    fs::rename(staged, target)
}

#[cfg(windows)]
fn replace_file(staged: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let staged = staged
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        MoveFileExW(
            staged.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
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
    use super::{
        atomic_write_private, preserve_corrupt_file, transactional_write_private_unlocked,
        with_persistence_lock,
    };
    use crate::platform::canonicalize_existing_dir;
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
    fn atomic_write_replaces_an_existing_file() {
        let dir = temp_dir("atomic-replace");
        let path = dir.join("state.toml");
        fs::write(&path, "old = true\n").unwrap();

        atomic_write_private(&path, "new = true\n").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new = true\n");
    }

    #[test]
    fn transactional_write_restores_committed_files_when_a_later_replace_fails() {
        let dir = temp_dir("transactional-rollback");
        let first = dir.join("first.toml");
        let invalid_target = dir.join("not-a-file");
        fs::write(&first, "value = 'before'\n").unwrap();
        fs::create_dir(&invalid_target).unwrap();

        let result = with_persistence_lock(|| {
            transactional_write_private_unlocked(&[
                (first.clone(), "value = 'after'\n".to_string()),
                (invalid_target.clone(), "value = 'invalid'\n".to_string()),
            ])
        });

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&first).unwrap(), "value = 'before'\n");
        assert!(invalid_target.is_dir());
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
