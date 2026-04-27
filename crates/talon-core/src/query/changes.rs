//! Changes handler — returns added/modified/deleted notes from the `event_log`.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::config::{ScopeFilter, TalonConfig};
use crate::contracts::VaultPath;
use crate::indexing::change_tracking;
use crate::query::{ChangeEntry, ChangesInput, ChangesResponse, TombstoneEntry};

/// Returns notes that were added, modified, or deleted since `input.since`.
///
/// Classification:
/// - **added**: first `'index'` event for a path falls within the window.
/// - **modified**: an `'index'` event within the window but an earlier `'index'`
///   event exists before the window.
/// - **deleted**: a `'delete'` event within the window.
///
/// Returns empty lists when `since` cannot be parsed.
#[must_use]
pub fn query_changes(
    conn: &Connection,
    input: &ChangesInput,
    config: Option<&TalonConfig>,
) -> ChangesResponse {
    let empty = ChangesResponse {
        vault: None,
        added: Vec::new(),
        modified: Vec::new(),
        deleted: Vec::new(),
    };
    let Ok(since_ms) = change_tracking::parse_since(&input.since) else {
        return empty;
    };
    let Some(all_events) = fetch_events(conn) else {
        return empty;
    };
    let filter = config.map(|cfg| {
        ScopeFilter::from_args(cfg, &input.scope, &input.scope_only, input.scope_all)
            .unwrap_or_else(|_| ScopeFilter::default_for(cfg))
    });
    classify(
        &all_events,
        since_ms,
        filter.as_ref(),
        input.limit.get() as usize,
    )
}

/// Loads all `event_log` rows as `(action, path, timestamp_ms)` tuples.
fn fetch_events(conn: &Connection) -> Option<Vec<(String, String, u64)>> {
    let Ok(mut stmt) = conn.prepare("SELECT action, path, timestamp FROM event_log ORDER BY id")
    else {
        return None;
    };
    let Ok(rows) = stmt.query_map([], |row| {
        let action: String = row.get(0)?;
        let path: String = row.get(1)?;
        let ts: String = row.get(2)?;
        Ok((action, path, ts))
    }) else {
        return None;
    };
    let Ok(events): rusqlite::Result<Vec<_>> = rows.collect() else {
        return None;
    };
    Some(
        events
            .into_iter()
            .filter_map(|(action, path, ts_str)| {
                rfc3339_to_ms(&ts_str).map(|ms| (action, path, ms))
            })
            .collect(),
    )
}

/// Classifies raw events into `ChangesResponse` buckets.
fn classify(
    all_events: &[(String, String, u64)],
    since_ms: u64,
    filter: Option<&ScopeFilter<'_>>,
    limit: usize,
) -> ChangesResponse {
    let indexed_before: HashSet<String> = all_events
        .iter()
        .filter(|(action, _, ts)| action == "index" && *ts < since_ms)
        .map(|(_, path, _)| path.clone())
        .collect();

    let mut latest_index: HashMap<String, u64> = HashMap::new();
    let mut latest_delete: HashMap<String, u64> = HashMap::new();

    for (action, path, ts_ms) in all_events {
        if *ts_ms < since_ms {
            continue;
        }
        if let Some(f) = filter
            && !f.accepts(path)
        {
            continue;
        }
        let map = if action == "index" {
            &mut latest_index
        } else if action == "delete" {
            &mut latest_delete
        } else {
            continue;
        };
        let entry = map.entry(path.clone()).or_insert(0);
        if *ts_ms > *entry {
            *entry = *ts_ms;
        }
    }

    let mut added: Vec<ChangeEntry> = Vec::new();
    let mut modified: Vec<ChangeEntry> = Vec::new();

    for (path, ts_ms) in latest_index {
        let Ok(vault_path) = VaultPath::parse(&path) else {
            continue;
        };
        let entry = ChangeEntry {
            path: vault_path,
            indexed_at: ts_ms,
        };
        if indexed_before.contains(&path) {
            modified.push(entry);
        } else {
            added.push(entry);
        }
    }

    let mut deleted: Vec<TombstoneEntry> = latest_delete
        .iter()
        .filter_map(|(path, ts_ms)| {
            VaultPath::parse(path)
                .ok()
                .map(|vault_path| TombstoneEntry {
                    path: vault_path,
                    deleted_at: *ts_ms,
                })
        })
        .collect();

    added.sort_by_key(|e| e.indexed_at);
    modified.sort_by_key(|e| e.indexed_at);
    deleted.sort_by_key(|e| e.deleted_at);

    let mut remaining = limit;
    added.truncate(remaining);
    remaining = remaining.saturating_sub(added.len());
    modified.truncate(remaining);
    remaining = remaining.saturating_sub(modified.len());
    deleted.truncate(remaining);

    ChangesResponse {
        vault: None,
        added,
        modified,
        deleted,
    }
}

fn rfc3339_to_ms(s: &str) -> Option<u64> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .ok()
        .and_then(|dt| u64::try_from(dt.unix_timestamp_nanos() / 1_000_000).ok())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use rusqlite::Connection;

    use super::query_changes;
    use crate::contracts::PositiveCount;
    use crate::indexing::migrations::run_migrations;
    use crate::query::ChangesInput;

    fn fresh_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn insert_event(conn: &Connection, action: &str, path: &str, timestamp: &str) {
        conn.execute(
            "INSERT INTO event_log (action, path, timestamp) VALUES (?, ?, ?)",
            rusqlite::params![action, path, timestamp],
        )
        .unwrap();
    }

    fn changes_input(since: &str) -> ChangesInput {
        ChangesInput {
            since: since.to_string(),
            scope: Vec::new(),
            scope_only: Vec::new(),
            scope_all: false,
            limit: PositiveCount::new(100, "limit").unwrap(),
        }
    }

    #[test]
    fn new_index_event_classified_as_added() {
        let conn = fresh_db();
        insert_event(&conn, "index", "a.md", "2024-01-15T10:30:01Z");

        let result = query_changes(&conn, &changes_input("2024-01-15T10:30:00Z"), None);

        assert_eq!(result.added.len(), 1);
        assert_eq!(result.added[0].path.as_str(), "a.md");
        assert!(result.modified.is_empty());
        assert!(result.deleted.is_empty());
    }

    #[test]
    fn reindex_after_prior_index_classified_as_modified() {
        let conn = fresh_db();
        insert_event(&conn, "index", "a.md", "2024-01-15T09:00:00Z");
        insert_event(&conn, "index", "a.md", "2024-01-15T10:30:01Z");

        let result = query_changes(&conn, &changes_input("2024-01-15T10:30:00Z"), None);

        assert!(result.added.is_empty());
        assert_eq!(result.modified.len(), 1);
        assert_eq!(result.modified[0].path.as_str(), "a.md");
        assert!(result.deleted.is_empty());
    }

    #[test]
    fn delete_event_classified_as_deleted() {
        let conn = fresh_db();
        insert_event(&conn, "index", "a.md", "2024-01-15T09:00:00Z");
        insert_event(&conn, "delete", "a.md", "2024-01-15T10:30:01Z");

        let result = query_changes(&conn, &changes_input("2024-01-15T10:30:00Z"), None);

        assert!(result.added.is_empty());
        assert!(result.modified.is_empty());
        assert_eq!(result.deleted.len(), 1);
        assert_eq!(result.deleted[0].path.as_str(), "a.md");
    }

    #[test]
    fn events_before_since_are_excluded() {
        let conn = fresh_db();
        insert_event(&conn, "index", "a.md", "2024-01-15T09:00:00Z");

        let result = query_changes(&conn, &changes_input("2024-01-15T10:30:00Z"), None);

        assert!(result.added.is_empty());
        assert!(result.modified.is_empty());
        assert!(result.deleted.is_empty());
    }

    #[test]
    fn invalid_since_returns_empty() {
        let conn = fresh_db();
        insert_event(&conn, "index", "a.md", "2024-01-15T10:30:01Z");

        let result = query_changes(&conn, &changes_input("not-a-timestamp"), None);

        assert!(result.added.is_empty());
        assert!(result.modified.is_empty());
        assert!(result.deleted.is_empty());
    }
}
