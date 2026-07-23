//! Per-project Kanban board persistence.
//!
//! The board lives in `<project_root>/.terminaltiler/board.json` so each project owns its
//! own board. Both the GTK app and the `terminaltiler-mcp` server read and write through
//! this module, using the same atomic write as the rest of the app.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::logging;
use crate::model::board::{BOARD_VERSION, Board};
use crate::storage::document::preserve_corrupt_warning;
use crate::storage::fs_utils::atomic_write_private;

/// Hidden per-project directory that holds TerminalTiler project state.
pub const BOARD_DIR_NAME: &str = ".terminaltiler";
/// Board file name within [`BOARD_DIR_NAME`].
pub const BOARD_FILE_NAME: &str = "board.json";
const BOARD_LOCK_DIR_NAME: &str = "board.lock";
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(10);
const LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const LOCK_STALE_AFTER: Duration = Duration::from_secs(300);

/// `<project_root>/.terminaltiler`.
pub fn board_dir(project_root: &Path) -> PathBuf {
    project_root.join(BOARD_DIR_NAME)
}

/// `<project_root>/.terminaltiler/board.json`.
pub fn board_path(project_root: &Path) -> PathBuf {
    board_dir(project_root).join(BOARD_FILE_NAME)
}

/// Whether a board was previously set up for this project (its `board.json` exists on disk).
pub fn board_exists(project_root: &Path) -> bool {
    board_path(project_root).exists()
}

/// Load the board for a project. A missing file yields an empty board; a corrupt file is
/// preserved aside and replaced with a fresh empty board (mirroring `SessionStore`).
pub fn load(project_root: &Path) -> Board {
    let path = board_path(project_root);
    match std::fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<Board>(&raw) {
            Ok(board) => board,
            Err(error) => {
                preserve_corrupt_warning(
                    &path,
                    &format!(
                        "TerminalTiler found a corrupt Kanban board ({error}) and started a fresh one."
                    ),
                );
                Board::default()
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Board::default(),
        Err(error) => {
            logging::error(format!(
                "could not read board file '{}': {error}",
                path.display()
            ));
            Board::default()
        }
    }
}

/// Atomically write the board, creating `.terminaltiler/` if needed.
///
/// The save is protected by the same cross-process lock used by [`update`] so a raw
/// writer cannot race a read-modify-write board mutation. Prefer [`update`] for
/// mutations that start from the current on-disk board.
#[allow(dead_code)]
pub fn save(project_root: &Path, board: &Board) -> std::io::Result<()> {
    with_exclusive_lock(project_root, || save_unlocked(project_root, board))
}

/// Load, mutate, and atomically save the board while holding a cross-process lock.
///
/// This keeps concurrent MCP clients and the GTK UI from silently overwriting each
/// other's changes: every writer observes the result of the previous writer before
/// applying its own mutation. The closure's return value is passed through after the
/// updated board has been persisted.
pub fn update<R>(project_root: &Path, mutate: impl FnOnce(&mut Board) -> R) -> std::io::Result<R> {
    with_exclusive_lock(project_root, || {
        let mut board = load(project_root);
        let result = mutate(&mut board);
        save_unlocked(project_root, &board)?;
        Ok(result)
    })
}

fn save_unlocked(project_root: &Path, board: &Board) -> std::io::Result<()> {
    let path = board_path(project_root);
    let serialized = if board.version == BOARD_VERSION {
        serde_json::to_string_pretty(board)
    } else {
        let mut normalized = board.clone();
        normalized.version = BOARD_VERSION;
        serde_json::to_string_pretty(&normalized)
    }
    .map_err(|error| std::io::Error::other(error.to_string()))?;
    atomic_write_private(&path, &serialized)
}

fn with_exclusive_lock<R>(
    project_root: &Path,
    operation: impl FnOnce() -> std::io::Result<R>,
) -> std::io::Result<R> {
    let _guard = BoardLock::acquire(project_root)?;
    operation()
}

struct BoardLock {
    path: PathBuf,
}

impl BoardLock {
    fn acquire(project_root: &Path) -> std::io::Result<Self> {
        let dir = board_dir(project_root);
        fs::create_dir_all(&dir)?;
        let path = dir.join(BOARD_LOCK_DIR_NAME);
        let started = Instant::now();

        loop {
            match fs::create_dir(&path) {
                Ok(()) => {
                    let _ = fs::write(path.join("owner"), std::process::id().to_string());
                    return Ok(Self { path });
                }
                Err(error) if is_lock_contention_error(&error, &path) => {
                    if is_stale_lock(&path) {
                        let _ = fs::remove_dir_all(&path);
                        continue;
                    }
                    if started.elapsed() >= LOCK_TIMEOUT {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            format!("timed out waiting for board lock '{}'", path.display()),
                        ));
                    }
                    std::thread::sleep(LOCK_RETRY_DELAY);
                }
                Err(error) => return Err(error),
            }
        }
    }
}

impl Drop for BoardLock {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path) {
            logging::error(format!(
                "failed to remove board lock '{}': {error}",
                self.path.display()
            ));
        }
    }
}

fn is_stale_lock(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age >= LOCK_STALE_AFTER)
}

fn is_lock_contention_error(error: &std::io::Error, path: &Path) -> bool {
    match error.kind() {
        std::io::ErrorKind::AlreadyExists => true,
        std::io::ErrorKind::PermissionDenied => is_windows_lock_contention(path),
        _ => false,
    }
}

#[cfg(windows)]
fn is_windows_lock_contention(path: &Path) -> bool {
    match fs::metadata(path) {
        Ok(metadata) => metadata.is_dir(),
        Err(error) => matches!(
            error.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
        ),
    }
}

#[cfg(not(windows))]
fn is_windows_lock_contention(_path: &Path) -> bool {
    false
}

/// Modification time of the board file, for cheap change detection by the UI poller.
pub fn mtime(project_root: &Path) -> Option<SystemTime> {
    std::fs::metadata(board_path(project_root))
        .ok()?
        .modified()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::board::TaskStatus;
    use crate::services::board::create_task;
    use std::fs;
    use uuid::Uuid;

    fn temp_root(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("terminaltiler-{prefix}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn missing_board_loads_empty() {
        let root = temp_root("board-missing");
        let board = load(&root);
        assert!(board.tasks.is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let root = temp_root("board-roundtrip");
        let mut board = Board::default();
        create_task(&mut board, "Wire MCP", "details", TaskStatus::Todo);
        save(&root, &board).unwrap();

        assert!(board_path(&root).exists());
        let loaded = load(&root);
        assert_eq!(loaded, board);
        assert!(mtime(&root).is_some());
    }

    #[test]
    fn save_normalizes_legacy_board_version() {
        let root = temp_root("board-version-normalize");
        let mut board = Board {
            version: 1,
            ..Board::default()
        };
        create_task(&mut board, "Version", "", TaskStatus::Todo);

        save(&root, &board).unwrap();

        let loaded = load(&root);
        assert_eq!(loaded.version, BOARD_VERSION);
    }

    #[test]
    fn update_serializes_concurrent_writers() {
        let root = std::sync::Arc::new(temp_root("board-concurrent-update"));
        let writers = 12;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(writers));

        let handles = (0..writers)
            .map(|index| {
                let root = root.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    update(&root, |board| {
                        create_task(board, format!("task-{index}"), "", TaskStatus::Todo);
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    })
                    .unwrap();
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().unwrap();
        }

        let board = load(&root);
        assert_eq!(board.tasks.len(), writers);
        for index in 0..writers {
            assert!(
                board
                    .tasks
                    .iter()
                    .any(|task| task.title == format!("task-{index}")),
                "missing task-{index}"
            );
        }
    }

    #[test]
    fn lock_contention_includes_existing_lock_directory() {
        let root = temp_root("board-lock-contention-existing");
        let path = board_dir(&root).join(BOARD_LOCK_DIR_NAME);
        let error = std::io::Error::from(std::io::ErrorKind::AlreadyExists);

        assert!(is_lock_contention_error(&error, &path));
    }

    #[test]
    #[cfg(not(windows))]
    fn lock_contention_keeps_permission_denied_fatal_off_windows() {
        let root = temp_root("board-lock-contention-permission");
        let path = board_dir(&root).join(BOARD_LOCK_DIR_NAME);
        let error = std::io::Error::from(std::io::ErrorKind::PermissionDenied);

        assert!(!is_lock_contention_error(&error, &path));
    }

    #[test]
    #[cfg(windows)]
    fn lock_contention_retries_transient_windows_permission_denied() {
        let root = temp_root("board-lock-contention-windows-permission");
        let path = board_dir(&root).join(BOARD_LOCK_DIR_NAME);
        let error = std::io::Error::from(std::io::ErrorKind::PermissionDenied);

        assert!(is_lock_contention_error(&error, &path));
    }

    #[test]
    fn corrupt_board_is_preserved_and_reset() {
        let root = temp_root("board-corrupt");
        fs::create_dir_all(board_dir(&root)).unwrap();
        fs::write(board_path(&root), "{ not json").unwrap();

        let board = load(&root);
        assert!(board.tasks.is_empty());
        assert!(!board_path(&root).exists());

        let corrupt_copies = fs::read_dir(board_dir(&root))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains("board.json.corrupt-")
            })
            .count();
        assert_eq!(corrupt_copies, 1);
    }
}
