//! Small filesystem/parsing helpers shared by the per-agent session-title sources.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Modification time of a filesystem entry, if available.
pub fn mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Whether `when` is no older than `max_age` relative to now. Timestamps in the future
/// (clock skew) are treated as recent.
pub fn is_recent(when: SystemTime, max_age: Duration) -> bool {
    match SystemTime::now().duration_since(when) {
        Ok(age) => age <= max_age,
        Err(_) => true,
    }
}

/// Canonicalize a path for comparison, falling back to a trailing-slash-trimmed copy when
/// the path cannot be resolved (e.g. it no longer exists).
pub fn normalize(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| {
        PathBuf::from(path.to_string_lossy().trim_end_matches('/').to_string())
    })
}

/// Whether a filesystem `target` directory equals a `recorded` cwd string from a session store.
pub fn cwd_matches(target: &Path, recorded: &str) -> bool {
    let recorded = recorded.trim();
    !recorded.is_empty() && normalize(target) == normalize(Path::new(recorded))
}

/// Trim a title and strip a leading agent status glyph (e.g. Claude's `✳`/`✻`) plus
/// surrounding whitespace, so tiles show a clean title.
pub fn clean_title(raw: &str) -> String {
    let trimmed = raw.trim();
    let stripped = trimmed.trim_start_matches(|c: char| {
        matches!(c, '✳' | '✻' | '✶' | '✷' | '●' | '◐' | '◓' | '◑' | '◒' | '·' | '*')
            || c.is_whitespace()
    });
    let stripped = if stripped.is_empty() { trimmed } else { stripped };
    // Session titles are single-line; guard against multi-line message fallbacks.
    stripped.lines().next().unwrap_or("").trim().to_string()
}

/// Whether a message looks like a real user prompt rather than injected metadata (slash-command
/// caveats, hook output, `AGENTS.md` instructions), which agents log as leading user entries.
/// Used only for the fallback title when a store exposes no explicit title.
pub fn is_meaningful_prompt(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        // Command/caveat/hook wrappers are XML-ish tags: `<local-command-caveat>`, `<command-…>`.
        && !trimmed.starts_with('<')
        // Injected instruction preambles.
        && !trimmed.starts_with("# AGENTS")
        && !trimmed.starts_with("<!--")
}

/// The newest regular file in `dir` matching `predicate`, paired with its mtime.
pub fn newest_file_in<F>(dir: &Path, predicate: F) -> Option<(PathBuf, SystemTime)>
where
    F: Fn(&str) -> bool,
{
    let mut best: Option<(PathBuf, SystemTime)> = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !predicate(&name) {
            continue;
        }
        let path = entry.path();
        let Some(m) = entry.metadata().ok().and_then(|md| md.modified().ok()) else {
            continue;
        };
        if best.as_ref().is_none_or(|(_, best_m)| m > *best_m) {
            best = Some((path, m));
        }
    }
    best
}

/// Decode a percent-encoded string (e.g. Grok's directory names like `%2Fhome%2Fuser`).
pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse a limited ISO-8601 UTC timestamp (`YYYY-MM-DDTHH:MM:SS[.fff]Z`) into a `SystemTime`.
/// Used for stores (Copilot) that record timestamps as strings rather than file mtimes.
pub fn parse_iso8601_utc(s: &str) -> Option<SystemTime> {
    let s = s.trim();
    if s.len() < 19 {
        return None;
    }
    let field = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (year, month, day) = (field(0, 4)?, field(5, 7)?, field(8, 10)?);
    let (hour, min, sec) = (field(11, 13)?, field(14, 16)?, field(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Days from civil epoch (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let secs = days * 86400 + hour * 3600 + min * 60 + sec;
    if secs < 0 {
        return None;
    }
    Some(UNIX_EPOCH + Duration::from_secs(secs as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decode_reverses_slash_encoding() {
        assert_eq!(percent_decode("%2Fhome%2Fu%2Fp"), "/home/u/p");
        assert_eq!(percent_decode("plain-name"), "plain-name");
    }

    #[test]
    fn clean_title_strips_status_glyph_and_extra_lines() {
        assert_eq!(clean_title("✳ Build the thing"), "Build the thing");
        assert_eq!(clean_title("  Plain title  "), "Plain title");
        assert_eq!(clean_title("First line\nsecond"), "First line");
    }

    #[test]
    fn parse_iso8601_matches_known_epoch() {
        // 1970-01-01T00:00:01Z == 1s after epoch.
        assert_eq!(
            parse_iso8601_utc("1970-01-01T00:00:01Z"),
            Some(UNIX_EPOCH + Duration::from_secs(1))
        );
        // A real Copilot timestamp parses without panicking and is after epoch.
        assert!(parse_iso8601_utc("2026-07-09T18:08:35.681Z").unwrap() > UNIX_EPOCH);
    }
}
