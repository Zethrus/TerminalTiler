//! Usage statistics engine.
//!
//! Platform-neutral typing counters shared by every terminal backend. Only
//! manual printable keyboard text should call `record_manual_typing`, which
//! keeps in-memory totals plus rolling per-day buckets. Persistence lives in
//! [`crate::storage::stats_store`]; this module owns only counting and the
//! read-only [`StatsSnapshot`] used by the UI.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Keystroke gaps longer than this do not accrue active typing time.
const IDLE_GAP: Duration = Duration::from_secs(2);
/// Daily buckets kept for the today / this-week views.
const MAX_DAYS: usize = 90;
/// Characters per word for standard WPM (industry convention).
#[cfg(any(target_os = "linux", feature = "windows-gtk-shell", test))]
const CHARS_PER_WORD: u64 = 5;

/// Monotonic lifetime totals. Never pruned.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifetimeTotals {
    #[serde(default)]
    pub chars: u64,
    #[serde(default)]
    pub words_ws: u64,
    #[serde(default)]
    pub keystrokes: u64,
    #[serde(default)]
    pub active_ms: u64,
}

/// Counters for a single local calendar day.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayBucket {
    /// Local calendar date, `YYYY-MM-DD`.
    pub date: String,
    #[serde(default)]
    pub chars: u64,
    #[serde(default)]
    pub words_ws: u64,
    #[serde(default)]
    pub keystrokes: u64,
    #[serde(default)]
    pub active_ms: u64,
}

/// Read-only view computed for the stats dialog.
#[cfg(any(target_os = "linux", feature = "windows-gtk-shell", test))]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatsSnapshot {
    pub total_chars: u64,
    pub chars_today: u64,
    pub chars_week: u64,
    pub total_words: u64,
    pub words_today: u64,
    pub words_week: u64,
    pub avg_wpm: f64,
    pub today_wpm: f64,
    pub week_wpm: f64,
    pub total_active_minutes: f64,
    pub today_active_minutes: f64,
}

/// In-memory counters. Wrapped by [`StatsRecorder`] for shared access.
#[derive(Debug, Default)]
struct StatsState {
    lifetime: LifetimeTotals,
    days: VecDeque<DayBucket>,
    last_input: Option<Instant>,
    dirty: bool,
}

impl StatsState {
    fn from_persisted(lifetime: LifetimeTotals, days: Vec<DayBucket>) -> Self {
        let mut days: VecDeque<DayBucket> = days.into();
        prune(&mut days);
        Self {
            lifetime,
            days,
            last_input: None,
            dirty: false,
        }
    }

    /// Append `printable`/`words`/timing to today's bucket and lifetime totals.
    /// `now` is injected so tests can drive active-time deterministically.
    fn record(&mut self, printable: u64, words: u64, today: &str, now: Instant) {
        if printable == 0 {
            return;
        }

        let mut active_ms = 0_u64;
        if let Some(last) = self.last_input {
            let gap = now.saturating_duration_since(last);
            if gap <= IDLE_GAP {
                active_ms = gap.as_millis() as u64;
            }
        }
        self.last_input = Some(now);

        let bucket = self.today_bucket(today);
        bucket.chars += printable;
        bucket.words_ws += words;
        bucket.keystrokes += 1;
        bucket.active_ms += active_ms;

        self.lifetime.chars += printable;
        self.lifetime.words_ws += words;
        self.lifetime.keystrokes += 1;
        self.lifetime.active_ms += active_ms;

        self.dirty = true;
    }

    /// Clear all counters and pending typing timing.
    fn reset(&mut self) {
        self.lifetime = LifetimeTotals::default();
        self.days.clear();
        self.last_input = None;
        self.dirty = true;
    }

    /// Mutable handle to today's bucket, creating + pruning as needed.
    fn today_bucket(&mut self, today: &str) -> &mut DayBucket {
        if self.days.back().map(|bucket| bucket.date.as_str()) != Some(today) {
            self.days.push_back(DayBucket {
                date: today.to_string(),
                ..DayBucket::default()
            });
            prune(&mut self.days);
        }
        self.days
            .back_mut()
            .expect("today's bucket was just ensured")
    }

    #[cfg(any(target_os = "linux", feature = "windows-gtk-shell", test))]
    fn snapshot(&self, today: &str) -> StatsSnapshot {
        let today_ord = day_ordinal(today);

        let mut chars_today = 0;
        let mut chars_week = 0;
        let mut active_ms_today = 0;
        let mut active_ms_week = 0;

        for bucket in &self.days {
            let in_today;
            let in_week;
            match (day_ordinal(&bucket.date), today_ord) {
                (Some(bucket_ord), Some(today_ord)) => {
                    let delta = today_ord - bucket_ord;
                    in_today = delta == 0;
                    in_week = (0..=6).contains(&delta);
                }
                // Unparseable date: fall back to string match for "today".
                _ => {
                    in_today = bucket.date == today;
                    in_week = in_today;
                }
            }
            if in_week {
                chars_week += bucket.chars;
                active_ms_week += bucket.active_ms;
            }
            if in_today {
                chars_today += bucket.chars;
                active_ms_today += bucket.active_ms;
            }
        }

        StatsSnapshot {
            total_chars: self.lifetime.chars,
            chars_today,
            chars_week,
            total_words: words_from_chars(self.lifetime.chars),
            words_today: words_from_chars(chars_today),
            words_week: words_from_chars(chars_week),
            avg_wpm: wpm(self.lifetime.chars, self.lifetime.active_ms),
            today_wpm: wpm(chars_today, active_ms_today),
            week_wpm: wpm(chars_week, active_ms_week),
            total_active_minutes: minutes(self.lifetime.active_ms),
            today_active_minutes: minutes(active_ms_today),
        }
    }
}

/// Cheap shared handle cloned into every terminal session / pane.
#[derive(Clone)]
pub struct StatsRecorder {
    state: Rc<RefCell<StatsState>>,
}

impl Default for StatsRecorder {
    fn default() -> Self {
        Self {
            state: Rc::new(RefCell::new(StatsState::default())),
        }
    }
}

impl StatsRecorder {
    /// Seed from persisted storage on startup.
    pub fn from_persisted(lifetime: LifetimeTotals, days: Vec<DayBucket>) -> Self {
        Self {
            state: Rc::new(RefCell::new(StatsState::from_persisted(lifetime, days))),
        }
    }

    /// Record one unit of manual keyboard typing. Counts printable Unicode
    /// chars only, discarding control chars and full escape sequences so arrow
    /// keys, Enter, function keys, paste wrappers, etc. do not inflate totals.
    /// Programmatic sends, paste, voice, runbooks, snippets, broadcasts, and
    /// terminal responses must not call this API.
    pub fn record_manual_typing(&self, text: &str) {
        let (printable, words) = measure(text);
        if printable == 0 {
            return;
        }
        let today = local_today_ymd();
        self.state
            .borrow_mut()
            .record(printable, words, &today, Instant::now());
    }

    /// Compute the current read-only view for the UI.
    #[cfg(any(target_os = "linux", feature = "windows-gtk-shell", test))]
    pub fn snapshot(&self) -> StatsSnapshot {
        let today = local_today_ymd();
        self.state.borrow().snapshot(&today)
    }

    /// Clear all counters and pending timing state. The next flush persists the
    /// empty state so the reset survives application restarts.
    pub fn reset(&self) {
        self.state.borrow_mut().reset();
    }

    /// If counters changed since the last flush, return a clone of the data to
    /// persist and clear the dirty flag. Returns `None` when nothing to write.
    pub fn take_persist_payload(&self) -> Option<(LifetimeTotals, Vec<DayBucket>)> {
        let mut state = self.state.borrow_mut();
        if !state.dirty {
            return None;
        }
        state.dirty = false;
        Some((state.lifetime.clone(), state.days.iter().cloned().collect()))
    }
}

/// Measure user input: `(printable_chars, words)`.
///
/// Strips terminal escape sequences (`ESC [ … final`, `ESC O …`, and bare
/// `ESC x`) entirely, so navigation / function keys add nothing. Control
/// whitespace (`\r`, `\n`, `\t`) is treated as a word separator but is not
/// counted as a typed character; every other printable char — spaces
/// included — counts once.
fn measure(text: &str) -> (u64, u64) {
    let mut cleaned = String::with_capacity(text.len());
    let mut char_count = 0_u64;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                // CSI / SS3: consume params up to and including the final byte.
                Some('[') | Some('O') => {
                    chars.next();
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if ('\u{40}'..='\u{7e}').contains(&next) {
                            break;
                        }
                    }
                }
                // Bare ESC + one char (e.g. Alt-modified key).
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
            continue;
        }

        if c.is_control() {
            if matches!(c, '\n' | '\r' | '\t') {
                cleaned.push(' ');
            }
            continue;
        }

        cleaned.push(c);
        char_count += 1;
    }

    let words = cleaned.split_whitespace().count() as u64;
    (char_count, words)
}

fn prune(days: &mut VecDeque<DayBucket>) {
    while days.len() > MAX_DAYS {
        days.pop_front();
    }
}

#[cfg(any(target_os = "linux", feature = "windows-gtk-shell", test))]
fn words_from_chars(chars: u64) -> u64 {
    chars / CHARS_PER_WORD
}

#[cfg(any(target_os = "linux", feature = "windows-gtk-shell", test))]
fn minutes(active_ms: u64) -> f64 {
    active_ms as f64 / 60_000.0
}

/// Standard WPM: (chars / 5) per active minute. Zero when no active time.
#[cfg(any(target_os = "linux", feature = "windows-gtk-shell", test))]
fn wpm(chars: u64, active_ms: u64) -> f64 {
    if active_ms == 0 {
        return 0.0;
    }
    let words = chars as f64 / CHARS_PER_WORD as f64;
    words / (active_ms as f64 / 60_000.0)
}

/// Convert a `YYYY-MM-DD` string to a day number for today/week comparisons.
#[cfg(any(target_os = "linux", feature = "windows-gtk-shell", test))]
fn day_ordinal(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

/// Days since 1970-01-01 (Howard Hinnant's civil-from-days algorithm).
#[cfg(any(target_os = "linux", feature = "windows-gtk-shell", test))]
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = (if year >= 0 { year } else { year - 399 }) / 400;
    let yoe = year - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Current local calendar date, `YYYY-MM-DD`.
#[cfg(target_os = "linux")]
fn local_today_ymd() -> String {
    // SAFETY: `localtime_r` fills a caller-owned `tm`; `time` takes a null ptr.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return String::from("1970-01-01");
        }
        format!(
            "{:04}-{:02}-{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday
        )
    }
}

#[cfg(target_os = "windows")]
fn local_today_ymd() -> String {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    // SAFETY: `GetLocalTime` fills a caller-owned, zeroed SYSTEMTIME.
    let st = unsafe {
        let mut st = std::mem::zeroed();
        GetLocalTime(&mut st);
        st
    };
    format!("{:04}-{:02}-{:02}", st.wYear, st.wMonth, st.wDay)
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn local_today_ymd() -> String {
    String::from("1970-01-01")
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: &str = "2026-06-17";

    fn instant_after(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn measure_counts_printable_and_strips_escape_sequences() {
        // "ls -la" + Enter + Up-arrow. Printable: l s space - l a = 6; the
        // CSI sequence \x1b[A contributes nothing. Words: "ls", "-la" = 2.
        assert_eq!(measure("ls -la\r\u{1b}[A"), (6, 2));
        // Pure navigation keys count as nothing.
        assert_eq!(measure("\u{1b}[A\u{1b}[B\u{1b}OP"), (0, 0));
        // Multi-line paste: newline separates words but is not counted;
        // the literal space between "bar" and "baz" is a printable char.
        // f,o,o,b,a,r,<space>,b,a,z = 10 chars; 3 words.
        assert_eq!(measure("foo\nbar baz"), (10, 3));
        // Unicode printable counts per char.
        assert_eq!(measure("héllo"), (5, 1));
    }

    #[test]
    fn measure_counts_bracketed_paste_payload_without_wrappers() {
        assert_eq!(measure("\u{1b}[200~hello world\u{1b}[201~"), (11, 2));
    }

    #[test]
    fn record_counts_into_lifetime_and_today() {
        let mut state = StatsState::default();
        state.record(6, 2, T0, Instant::now());
        assert_eq!(state.lifetime.chars, 6);
        assert_eq!(state.lifetime.words_ws, 2);
        assert_eq!(state.days.back().unwrap().chars, 6);
    }

    #[test]
    fn ignores_empty_records() {
        let mut state = StatsState::default();
        state.record(0, 0, T0, Instant::now());
        assert!(state.days.is_empty());
        assert!(!state.dirty);
    }

    #[test]
    fn active_time_only_accrues_within_idle_gap() {
        let mut state = StatsState::default();
        let base = Instant::now();
        // First keystroke: no prior, no active time.
        state.record(1, 1, T0, base);
        // 500ms later (within gap): +500ms active.
        state.record(1, 1, T0, instant_after(base, 500));
        // 5s later (beyond 2s gap): no active time added.
        state.record(1, 1, T0, instant_after(base, 5_500));

        assert_eq!(state.lifetime.active_ms, 500);
    }

    #[test]
    fn snapshot_separates_today_and_week_and_total() {
        let mut state = StatsState::default();
        // 10 days ago: outside the week, inside lifetime.
        state.days.push_back(DayBucket {
            date: "2026-06-07".into(),
            chars: 100,
            words_ws: 20,
            keystrokes: 10,
            active_ms: 60_000,
        });
        // 3 days ago: inside the week.
        state.days.push_back(DayBucket {
            date: "2026-06-14".into(),
            chars: 50,
            words_ws: 10,
            keystrokes: 5,
            active_ms: 30_000,
        });
        // today.
        state.days.push_back(DayBucket {
            date: T0.into(),
            chars: 25,
            words_ws: 5,
            keystrokes: 3,
            active_ms: 15_000,
        });
        state.lifetime = LifetimeTotals {
            chars: 175,
            words_ws: 35,
            keystrokes: 18,
            active_ms: 105_000,
        };

        let snap = state.snapshot(T0);
        assert_eq!(snap.chars_today, 25);
        assert_eq!(snap.chars_week, 75); // 50 + 25
        assert_eq!(snap.total_chars, 175);
        assert_eq!(snap.total_words, 35); // 175 / 5
        assert_eq!(snap.words_today, 5); // 25 / 5
        // Today: 25 chars => 5 words over 0.25 min => 20 WPM.
        assert!((snap.today_wpm - 20.0).abs() < 1e-9);
    }

    #[test]
    fn wpm_guards_zero_active_time() {
        assert_eq!(wpm(100, 0), 0.0);
    }

    #[test]
    fn prunes_to_ninety_days() {
        let mut state = StatsState::default();
        for day in 1..=95 {
            // Fabricated ascending dates within a single month range is fine for
            // pruning (count-based, date values irrelevant here).
            state.days.push_back(DayBucket {
                date: format!("2026-{:02}-{:02}", (day / 28) + 1, (day % 28) + 1),
                chars: 1,
                ..DayBucket::default()
            });
            prune(&mut state.days);
        }
        assert_eq!(state.days.len(), MAX_DAYS);
    }

    #[test]
    fn rollover_creates_new_bucket() {
        let mut state = StatsState::default();
        let now = Instant::now();
        state.record(5, 1, "2026-06-16", now);
        state.record(5, 1, "2026-06-17", instant_after(now, 100));
        assert_eq!(state.days.len(), 2);
        assert_eq!(state.days.back().unwrap().date, "2026-06-17");
        assert_eq!(state.days.back().unwrap().chars, 5);
    }

    #[test]
    fn dirty_flag_drives_persistence_payload() {
        let recorder = StatsRecorder::default();
        assert!(recorder.take_persist_payload().is_none());
        recorder.record_manual_typing("hello");
        let payload = recorder.take_persist_payload();
        assert!(payload.is_some());
        // Second call without new input yields nothing.
        assert!(recorder.take_persist_payload().is_none());
    }

    #[test]
    fn manual_typing_counts_printable_chars_words_and_wpm() {
        let recorder = StatsRecorder::default();
        recorder.record_manual_typing("hello world");

        let snapshot = recorder.snapshot();
        assert_eq!(snapshot.total_chars, 11);
        assert_eq!(snapshot.total_words, 2);
        assert_eq!(snapshot.chars_today, 11);
        assert_eq!(snapshot.words_today, 2);
    }

    #[test]
    fn manual_typing_ignores_control_and_navigation_payloads() {
        let recorder = StatsRecorder::default();
        recorder.record_manual_typing("\r\n\t\u{1b}[A\u{1b}[B\u{1b}OP\u{7f}");

        assert!(recorder.take_persist_payload().is_none());
        assert_eq!(recorder.snapshot().total_chars, 0);
    }

    #[test]
    fn reset_clears_snapshot_and_persists_empty_payload() {
        let recorder = StatsRecorder::default();
        recorder.record_manual_typing("hello world");
        let _ = recorder.take_persist_payload();

        recorder.reset();

        let snapshot = recorder.snapshot();
        assert_eq!(snapshot, StatsSnapshot::default());
        let (lifetime, days) = recorder
            .take_persist_payload()
            .expect("reset should be persisted even after previous flush");
        assert_eq!(lifetime, LifetimeTotals::default());
        assert!(days.is_empty());
    }

    #[test]
    fn reset_clears_active_typing_timer() {
        let mut state = StatsState::default();
        let base = Instant::now();
        state.record(1, 1, T0, base);

        state.reset();
        state.record(1, 1, T0, instant_after(base, 500));

        assert_eq!(state.lifetime.active_ms, 0);
        assert_eq!(state.lifetime.chars, 1);
        assert_eq!(state.days.len(), 1);
    }

    #[test]
    fn day_ordinal_parses_and_rejects() {
        assert!(day_ordinal("2026-06-17").is_some());
        assert!(day_ordinal("garbage").is_none());
        assert!(day_ordinal("2026-13-01").is_none());
        let a = day_ordinal("2026-06-17").unwrap();
        let b = day_ordinal("2026-06-10").unwrap();
        assert_eq!(a - b, 7);
    }
}
