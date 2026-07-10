//! Copilot CLI sessions: `~/.copilot/session-store.db` (SQLite).
//!
//! The title lives only in the database (there is none in the plaintext session-state files):
//! the `sessions` table keys sessions by `cwd`/`updated_at`, and `checkpoints` holds titled
//! milestones. We open the db read-only, find the newest session for the cwd, and use its
//! latest checkpoint title, falling back to the session `summary`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use super::util;
use super::{AgentKind, ResolvedTitle, SessionTitleSource};
use crate::platform::home_dir;

/// Cap on candidate session rows inspected for a cwd match.
const MAX_CANDIDATES: usize = 64;

pub struct CopilotSource {
    db: Option<PathBuf>,
}

impl Default for CopilotSource {
    fn default() -> Self {
        Self {
            db: home_dir().map(|home| home.join(".copilot").join("session-store.db")),
        }
    }
}

impl CopilotSource {
    #[cfg(test)]
    pub fn with_db(db: PathBuf) -> Self {
        Self { db: Some(db) }
    }
}

impl SessionTitleSource for CopilotSource {
    fn active_title(&self, cwd: &Path, max_age: Duration) -> Option<ResolvedTitle> {
        let db = self.db.as_ref()?;
        // A cheap stat gates the SQLite open: if the store has not been written within the
        // window, no Copilot session anywhere is active, so no tile needs a query.
        if !util::mtime(db).is_some_and(|m| util::is_recent(m, max_age)) {
            return None;
        }
        // Read-only so we never contend with a running Copilot process.
        let conn = Connection::open_with_flags(
            db,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .ok()?;
        query_active_title(&conn, cwd, max_age)
    }
}

fn query_active_title(conn: &Connection, cwd: &Path, max_age: Duration) -> Option<ResolvedTitle> {
    let mut stmt = conn
        .prepare("SELECT id, cwd, summary, updated_at FROM sessions ORDER BY updated_at DESC")
        .ok()?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            ))
        })
        .ok()?;

    for (index, row) in rows.flatten().enumerate() {
        if index >= MAX_CANDIDATES {
            break;
        }
        let (id, session_cwd, summary, updated_at) = row;
        if !util::cwd_matches(cwd, &session_cwd) {
            continue;
        }
        // Skip a row with an unparseable timestamp rather than abandoning the lookup: an
        // older but still-active session for this cwd may follow.
        let Some(updated_at) = util::parse_iso8601_utc(&updated_at) else {
            continue;
        };
        if !util::is_recent(updated_at, max_age) {
            // Rows are newest-first; once a parseable one is stale the rest are too.
            return None;
        }
        let title = checkpoint_title(conn, &id)
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| util::clean_title(&summary));
        if title.is_empty() {
            return None;
        }
        return Some(ResolvedTitle {
            title,
            agent: AgentKind::Copilot,
        });
    }
    None
}

/// The most recent non-empty checkpoint title for a session.
fn checkpoint_title(conn: &Connection, session_id: &str) -> Option<String> {
    let mut stmt = conn
        .prepare(
            "SELECT title FROM checkpoints WHERE session_id = ?1 \
             AND title IS NOT NULL AND title <> '' ORDER BY created_at DESC LIMIT 1",
        )
        .ok()?;
    let title: String = stmt.query_row([session_id], |row| row.get(0)).ok()?;
    Some(util::clean_title(&title))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn seed_db(path: &Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (id TEXT, cwd TEXT, summary TEXT, updated_at TEXT);
             CREATE TABLE checkpoints (session_id TEXT, title TEXT, created_at TEXT);",
        )
        .unwrap();
        conn.close().unwrap();
    }

    #[test]
    fn prefers_latest_checkpoint_title() {
        let tmp = tempdir();
        let cwd = tmp.join("session-title-project");
        fs::create_dir_all(&cwd).unwrap();
        let db = tmp.join("session-store.db");
        seed_db(&db);
        let conn = Connection::open(&db).unwrap();
        conn.execute(
            "INSERT INTO sessions VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                "s1",
                cwd.to_string_lossy(),
                "old summary",
                "2026-07-09T18:08:35.681Z"
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO checkpoints VALUES (?1, ?2, ?3)",
            rusqlite::params!["s1", "Guided Installer UX", "2026-07-09T18:00:00.000Z"],
        )
        .unwrap();
        conn.close().unwrap();

        let source = CopilotSource::with_db(db);
        // Huge window so the fixed 2026 timestamp always counts as recent.
        let resolved = source
            .active_title(&cwd, Duration::from_secs(1_000_000_000_000))
            .expect("title");
        assert_eq!(resolved.title, "Guided Installer UX");
        assert_eq!(resolved.agent, AgentKind::Copilot);
    }

    #[test]
    fn skips_row_with_unparseable_timestamp() {
        let tmp = tempdir();
        let cwd = tmp.join("proj");
        fs::create_dir_all(&cwd).unwrap();
        let db = tmp.join("session-store.db");
        seed_db(&db);
        let conn = Connection::open(&db).unwrap();
        // Newest row (by string order) has a garbage timestamp; an older valid one follows.
        conn.execute(
            "INSERT INTO sessions VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                "bad",
                cwd.to_string_lossy(),
                "should be skipped",
                "not-a-date"
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                "good",
                cwd.to_string_lossy(),
                "Real title",
                "2026-07-09T18:08:35.681Z"
            ],
        )
        .unwrap();
        conn.close().unwrap();

        let source = CopilotSource::with_db(db);
        let resolved = source
            .active_title(&cwd, Duration::from_secs(1_000_000_000_000))
            .expect("title");
        assert_eq!(resolved.title, "Real title");
    }

    #[test]
    fn falls_back_to_summary_without_checkpoint() {
        let tmp = tempdir();
        let cwd = tmp.join("proj");
        fs::create_dir_all(&cwd).unwrap();
        let db = tmp.join("session-store.db");
        seed_db(&db);
        let conn = Connection::open(&db).unwrap();
        conn.execute(
            "INSERT INTO sessions VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                "s2",
                cwd.to_string_lossy(),
                "Investigate flaky deploy",
                "2026-07-09T18:08:35.681Z"
            ],
        )
        .unwrap();
        conn.close().unwrap();

        let source = CopilotSource::with_db(db);
        let resolved = source
            .active_title(&cwd, Duration::from_secs(1_000_000_000_000))
            .expect("title");
        assert_eq!(resolved.title, "Investigate flaky deploy");
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tt-copilot-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
