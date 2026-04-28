//! Shared helpers for formatting `notes.mtime_ms` into agent-friendly strings.
//!
//! Two formats live here for two consumers:
//!
//! - [`local_mtime_for_path`] / [`format_local_mtime`] return `"HH:MM"` when
//!   the file was modified within the last 24 hours and `"YYYY-MM-DD"`
//!   otherwise. Used for per-result `mtime` fields where the agent only
//!   needs a freshness hint — recent edits get instantly-readable wall-clock
//!   time, older edits get the date. The 24h window beats calendar-today
//!   because a note edited at 23:00 last night should still read as recent
//!   when queried at 04:00 the next morning.
//! - [`format_iso8601`] returns full RFC 3339 in UTC (`"2026-04-25T10:23:00Z"`).
//!   Used for `changes` events where sub-day precision matters because
//!   `--since` consumers compare exact timestamps.

use rusqlite::Connection;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

/// Formats a unix-epoch millisecond timestamp as RFC 3339 / ISO 8601 in UTC.
///
/// Output uses seconds precision (`"2026-04-25T10:23:00Z"`). Returns `None`
/// when the timestamp is out of range or formatting fails.
#[must_use]
pub fn format_iso8601(mtime_ms: u64) -> Option<String> {
    let secs = i128::from(mtime_ms) * 1_000_000;
    let dt = OffsetDateTime::from_unix_timestamp_nanos(secs).ok()?;
    let dt = dt.replace_nanosecond(0).ok()?;
    dt.format(&Rfc3339).ok()
}

/// SQL fragment selecting `"HH:MM"` for files modified within the last 24
/// hours and `"YYYY-MM-DD"` otherwise, in the system local timezone.
///
/// The placeholder `MS_EXPR` is substituted with whichever expression yields
/// the millisecond timestamp for the row at hand (`?1`, `mtime_ms`, etc.).
const MTIME_CASE_SQL: &str = "CASE \
    WHEN MS_EXPR / 1000 >= CAST(strftime('%s', 'now') AS INTEGER) - 86400 \
    THEN strftime('%H:%M', MS_EXPR / 1000, 'unixepoch', 'localtime') \
    ELSE strftime('%Y-%m-%d', MS_EXPR / 1000, 'unixepoch', 'localtime') \
END";

fn mtime_case(ms_expr: &str) -> String {
    MTIME_CASE_SQL.replace("MS_EXPR", ms_expr)
}

/// Formats a unix-epoch millisecond timestamp as `"HH:MM"` (within 24h) or
/// `"YYYY-MM-DD"` (older), in the system local timezone.
///
/// `SQLite` handles the timezone conversion via its `'localtime'` modifier so
/// we don't need a runtime local-offset lookup (which is fallible on
/// multi-threaded processes via the `time` crate). Returns `None` when the
/// query fails.
#[must_use]
pub fn format_local_mtime(conn: &Connection, mtime_ms: i64) -> Option<String> {
    let sql = format!("SELECT {}", mtime_case("?1"));
    conn.query_row(&sql, [mtime_ms], |row| row.get::<_, Option<String>>(0))
        .ok()
        .flatten()
}

/// Looks up `notes.mtime_ms` for a vault path and returns the formatted
/// freshness hint (`"HH:MM"` for the last 24h, `"YYYY-MM-DD"` otherwise).
#[must_use]
pub fn local_mtime_for_path(conn: &Connection, vault_path: &str) -> Option<String> {
    let sql = format!(
        "SELECT {} FROM notes WHERE vault_path = ?1 AND active = 1",
        mtime_case("mtime_ms")
    );
    conn.query_row(&sql, [vault_path], |row| row.get::<_, Option<String>>(0))
        .ok()
        .flatten()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::indexing::migrations::run_migrations;

    fn fresh_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    #[test]
    fn formats_unix_ms_as_rfc3339_zulu() {
        // 2026-04-15 00:00:00 UTC = 1776211200000 ms
        assert_eq!(
            format_iso8601(1_776_211_200_000),
            Some("2026-04-15T00:00:00Z".to_string())
        );
    }

    #[test]
    fn truncates_subsecond_precision() {
        // 2026-04-15 00:00:00.789 UTC
        assert_eq!(
            format_iso8601(1_776_211_200_789),
            Some("2026-04-15T00:00:00Z".to_string())
        );
    }

    #[test]
    fn out_of_range_returns_none() {
        assert!(format_iso8601(u64::MAX).is_none());
    }

    #[test]
    fn format_local_mtime_old_timestamp_returns_date() {
        let conn = fresh_db();
        // 2020-01-15 — definitely older than 24h.
        let s = format_local_mtime(&conn, 1_579_046_400_000).expect("should format");
        assert_eq!(s.len(), 10, "expected YYYY-MM-DD, got {s:?}");
        assert_eq!(s.chars().nth(4), Some('-'));
        assert_eq!(s.chars().nth(7), Some('-'));
    }

    #[test]
    fn format_local_mtime_recent_timestamp_returns_hh_mm() {
        let conn = fresh_db();
        // Current time — within the 24h window by definition.
        let now_ms = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap();
        let s = format_local_mtime(&conn, now_ms).expect("should format");
        assert_eq!(s.len(), 5, "expected HH:MM, got {s:?}");
        assert_eq!(s.chars().nth(2), Some(':'));
    }

    #[test]
    fn local_mtime_for_missing_path_returns_none() {
        let conn = fresh_db();
        assert!(local_mtime_for_path(&conn, "does/not/exist.md").is_none());
    }
}
