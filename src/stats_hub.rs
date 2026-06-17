//! Process-wide usage statistics singleton.
//!
//! Both the Linux GTK app and the Windows shells need a single shared
//! [`StatsRecorder`] (so concurrent windows never clobber the same
//! `usage-stats.toml`) seeded from a single [`StatsStore`]. This module owns
//! that singleton and the immediate-flush helper. Scheduling periodic flushes
//! is left to each platform's UI layer, which already owns a main-loop timer.

use std::cell::OnceCell;
use std::rc::Rc;

use crate::services::stats::StatsRecorder;
use crate::storage::stats_store::StatsStore;

thread_local! {
    static HUB: OnceCell<(Rc<StatsStore>, StatsRecorder)> = const { OnceCell::new() };
}

/// The shared store + recorder, seeded from disk on first access.
fn shared() -> (Rc<StatsStore>, StatsRecorder) {
    HUB.with(|cell| {
        cell.get_or_init(|| {
            let store = Rc::new(StatsStore::new());
            let loaded = store.load();
            let recorder = StatsRecorder::from_persisted(loaded.lifetime, loaded.days);
            (store, recorder)
        })
        .clone()
    })
}

/// The shared recorder. Clone it into each terminal session / pane.
pub fn recorder() -> StatsRecorder {
    shared().1
}

/// Persist any pending counters immediately (periodic tick or on close).
pub fn flush() {
    let (store, recorder) = shared();
    flush_recorder(&store, &recorder);
}

/// Clear all shared usage statistics and persist the empty counters.
pub fn reset() {
    let (store, recorder) = shared();
    recorder.reset();
    flush_recorder(&store, &recorder);
}

fn flush_recorder(store: &StatsStore, recorder: &StatsRecorder) {
    if let Some((lifetime, days)) = recorder.take_persist_payload() {
        store.save(&lifetime, &days);
    }
}
