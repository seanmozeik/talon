use std::collections::HashSet;

use rusqlite::{Connection, params};

use crate::contracts::VaultPath;
use crate::numeric::count_u32;
use crate::query::related::find_related;
use crate::query::{LinkedNote, NoteExcerpt, RecallInput, RelatedInput};
use crate::search::Direction;
use crate::search::types::RawSearchResult;

fn to_headline(snippet: &str) -> String {
    let first = snippet
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if first.len() <= 120 {
        return first.to_owned();
    }
    first[..120]
        .rfind(['.', '!', '?'])
        .map_or_else(|| format!("{}…", &first[..117]), |i| first[..=i].to_owned())
}

fn mtime_date(conn: &Connection, path: &str) -> String {
    conn.query_row(
        "SELECT strftime('%Y-%m-%d', mtime_ms / 1000, 'unixepoch') \
         FROM notes WHERE vault_path = ?1 AND active = 1",
        rusqlite::params![path],
        |row| row.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
    .unwrap_or_default()
}

pub(super) fn build_linked_context(
    conn: &Connection,
    pipeline_results: &[RawSearchResult],
    input: &RecallInput,
    excluded_set: &HashSet<String>,
) -> (Vec<LinkedNote>, u32) {
    let Some(top_path) = pipeline_results.first().map(|r| r.path.clone()) else {
        return (Vec::new(), 0);
    };
    let ri = RelatedInput {
        path: top_path,
        depth: input.depth.clamp(0, 3),
        direction: Direction::Both,
        scope: input.scope.clone(),
        scope_only: input.scope_only.clone(),
    };
    let rel = find_related(conn, &ri);
    let link_count = count_u32(rel.results.len());
    let notes: Vec<LinkedNote> = rel
        .results
        .into_iter()
        .filter(|r| !excluded_set.contains(r.vault_path.as_str()))
        .map(|r| LinkedNote {
            vault_path: r.vault_path,
            title: r.title,
            link_text: r.link_text,
            relation: r.relation,
            hops: 1,
        })
        .collect();
    (notes, link_count)
}

pub(super) fn to_note_excerpts(
    conn: &Connection,
    pipeline_results: &[RawSearchResult],
) -> Vec<NoteExcerpt> {
    pipeline_results
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let vault_path = VaultPath::parse(&r.path).ok()?;
            Some(NoteExcerpt {
                vault_path,
                title: r.title.clone(),
                snippet: to_headline(&r.snippet),
                score: r.score,
                rank: u32::try_from(i + 1).unwrap_or(u32::MAX),
                mtime: mtime_date(conn, &r.path),
            })
        })
        .collect()
}

/// Returns fractional days since a note's `mtime_ms`, or a large sentinel value.
pub(super) fn days_since_mtime(conn: &Connection, vault_path: &str) -> f64 {
    let mtime: Option<u64> = conn
        .query_row(
            "SELECT mtime_ms FROM notes WHERE vault_path = ? AND active = 1",
            params![vault_path],
            |row| row.get(0),
        )
        .ok();
    mtime.map_or(9999.0, |ms| {
        let diff_days =
            u32::try_from(now_millis().saturating_sub(ms) / 86_400_000).unwrap_or(u32::MAX);
        f64::from(diff_days)
    })
}

fn now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::indexing::migrations::run_migrations;
    use rusqlite::Connection;

    #[test]
    fn headline_takes_first_nonempty_line() {
        assert_eq!(to_headline("line one\nline two"), "line one");
    }

    #[test]
    fn headline_skips_blank_lines() {
        assert_eq!(to_headline("\n\n  content  \n"), "content");
    }

    #[test]
    fn headline_truncates_long_line_at_sentence() {
        // >120 chars, ends with a period — should cut at sentence boundary
        let s = "A ".repeat(40) + "end.";
        let result = to_headline(&s);
        assert!(
            result.ends_with('.'),
            "should end at sentence boundary: {result:?}"
        );
        assert!(result.len() <= 120, "too long: {}", result.len());
    }

    #[test]
    fn headline_hard_truncates_with_ellipsis() {
        let s = "x".repeat(200);
        let result = to_headline(&s);
        assert!(
            result.ends_with('…'),
            "should end with ellipsis: {result:?}"
        );
        // 117 ascii bytes + ellipsis (3 UTF-8 bytes) = 120 bytes max
        assert!(result.len() <= 120, "too long: {}", result.len());
    }

    #[test]
    fn mtime_date_returns_empty_for_unknown_path() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        assert_eq!(mtime_date(&conn, "does/not/exist.md"), "");
    }

    #[test]
    fn mtime_date_formats_unix_ms_as_yyyy_mm_dd() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        // 2026-04-15 00:00:00 UTC = 1776211200 seconds = 1776211200000 ms
        conn.execute(
            "INSERT INTO notes (vault_path, title, tags, aliases, content, frontmatter, \
             mtime_ms, size_bytes, hash, docid, active) \
             VALUES ('test.md', 'Test', '[]', '[]', '', '{}', 1776211200000, 0, 'h', 1, 1)",
            [],
        )
        .unwrap();
        assert_eq!(mtime_date(&conn, "test.md"), "2026-04-15");
    }
}
