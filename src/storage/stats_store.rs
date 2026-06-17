use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::app_paths;
use crate::logging;
use crate::services::stats::{DayBucket, LifetimeTotals};
use crate::storage::document::{
    preserve_corrupt_warning, read_optional_string, write_toml_private,
};

const STATS_VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
struct StatsDocument {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    lifetime: LifetimeTotals,
    #[serde(default)]
    days: Vec<DayBucket>,
}

/// Loaded usage data, ready to seed a `StatsRecorder`.
#[derive(Debug, Default)]
pub struct LoadedStats {
    pub lifetime: LifetimeTotals,
    pub days: Vec<DayBucket>,
}

/// Persists usage statistics to `usage-stats.toml` in the data directory.
/// Mirrors [`crate::storage::session_store::SessionStore`]: versioned TOML,
/// atomic private writes, corrupt-file recovery.
#[derive(Clone, Debug)]
pub struct StatsStore {
    path: Option<PathBuf>,
}

impl Default for StatsStore {
    fn default() -> Self {
        Self::new()
    }
}

impl StatsStore {
    pub fn new() -> Self {
        let path = app_paths::data_dir().map(|dir| dir.join("usage-stats.toml"));
        Self { path }
    }

    /// Load saved usage data. Missing file returns defaults; corrupt or
    /// version-mismatched files are moved aside and replaced with defaults.
    pub fn load(&self) -> LoadedStats {
        let Some(path) = self.path.as_ref() else {
            return LoadedStats::default();
        };

        let raw = match read_optional_string(path) {
            Ok(Some(raw)) => raw,
            Ok(None) => return LoadedStats::default(),
            Err(error) => {
                logging::info(format!(
                    "failed to read usage stats file '{}': {}",
                    path.display(),
                    error
                ));
                return LoadedStats::default();
            }
        };

        match toml::from_str::<StatsDocument>(&raw) {
            Ok(document) if document.version == STATS_VERSION => LoadedStats {
                lifetime: document.lifetime,
                days: document.days,
            },
            Ok(_) => {
                preserve_corrupt_warning(
                    path,
                    "TerminalTiler moved an incompatible usage stats file aside and started fresh.",
                );
                LoadedStats::default()
            }
            Err(error) => {
                preserve_corrupt_warning(
                    path,
                    &format!(
                        "TerminalTiler found a corrupt usage stats file ({}) and moved it aside.",
                        error
                    ),
                );
                LoadedStats::default()
            }
        }
    }

    /// Atomically write the supplied counters.
    pub fn save(&self, lifetime: &LifetimeTotals, days: &[DayBucket]) {
        let Some(path) = &self.path else {
            return;
        };

        let document = StatsDocument {
            version: STATS_VERSION,
            lifetime: lifetime.clone(),
            days: days.to_vec(),
        };

        if let Err(error) = write_toml_private(path, &document) {
            logging::info(format!("failed to write usage stats file: {}", error));
        }
    }
}

#[cfg(test)]
impl StatsStore {
    fn from_path(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use uuid::Uuid;

    use super::{STATS_VERSION, StatsStore};
    use crate::services::stats::{DayBucket, LifetimeTotals};

    fn temp_path(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("terminaltiler-{prefix}-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("usage-stats.toml")
    }

    #[test]
    fn missing_file_returns_defaults() {
        let store = StatsStore::from_path(temp_path("stats-missing"));
        let loaded = store.load();
        assert_eq!(loaded.lifetime, LifetimeTotals::default());
        assert!(loaded.days.is_empty());
    }

    #[test]
    fn roundtrips_saved_stats() {
        let store = StatsStore::from_path(temp_path("stats-roundtrip"));
        let lifetime = LifetimeTotals {
            chars: 1234,
            words_ws: 200,
            keystrokes: 300,
            active_ms: 600_000,
        };
        let days = vec![DayBucket {
            date: "2026-06-17".into(),
            chars: 50,
            words_ws: 10,
            keystrokes: 12,
            active_ms: 30_000,
        }];

        store.save(&lifetime, &days);
        let loaded = store.load();

        assert_eq!(loaded.lifetime, lifetime);
        assert_eq!(loaded.days, days);
    }

    #[test]
    fn corrupt_file_is_moved_aside() {
        let path = temp_path("stats-corrupt");
        fs::write(&path, "this is not valid toml {{{").unwrap();
        let store = StatsStore::from_path(path.clone());

        let loaded = store.load();
        assert!(loaded.days.is_empty());
        assert!(!path.exists(), "corrupt file should have been moved aside");
    }

    #[test]
    fn version_mismatch_starts_fresh() {
        let path = temp_path("stats-version");
        let doc = format!("version = {}\n", STATS_VERSION + 1);
        fs::write(&path, doc).unwrap();
        let store = StatsStore::from_path(path);

        let loaded = store.load();
        assert_eq!(loaded.lifetime, LifetimeTotals::default());
    }
}
