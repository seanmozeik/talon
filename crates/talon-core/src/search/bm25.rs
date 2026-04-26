//! BM25 retrieval against `notes_fts_bm25`.
//!
//! Ports `services/talon/search/bm25.ts`. Two retrievers:
//!
//! - [`search_bm25`] — full-text BM25 with OHS column weights
//!   (title=10, alias=5, content=1) joined OR-wise so documents are ranked
//!   by how many query terms they match.
//! - [`search_by_alias_exact`] — exact alias lookup by NFD-normalized form,
//!   used so the hybrid pipeline can give exact alias hits the high
//!   exact-alias RRF weight rather than the lower fuzzy-title weight.

use rusqlite::{Connection, params};

use crate::frontmatter::normalize_keyword;

use super::constants::{BM25_FTS_SCORES, BM25_MIN_TOKENS, BM25_TOKENS_PER_CHAR_DIV};
use super::text_fts::{FtsOperator, build_bm25_score, to_fts_query};
use super::types::{RawSearchResult, SearchScores};

fn parse_string_array(raw: Option<String>) -> Vec<String> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default()
}

/// Searches the BM25 FTS index with OHS-weighted columns.
///
/// `snippet_length` controls the FTS5 `snippet()` token budget; values below
/// [`BM25_MIN_TOKENS`] characters worth of tokens are clamped up.
///
/// Returns an empty vector on FTS errors (e.g. malformed query that survives
/// sanitization).
#[must_use]
pub fn search_bm25(
    conn: &Connection,
    query: &str,
    limit: u32,
    snippet_length: u32,
) -> Vec<RawSearchResult> {
    let num_tokens = BM25_MIN_TOKENS.max(snippet_length.div_ceil(BM25_TOKENS_PER_CHAR_DIV));
    let fts_query = to_fts_query(query, FtsOperator::Or);
    let sql = format!(
        "SELECT n.vault_path, n.title, n.tags, n.aliases,
                snippet(notes_fts_bm25, 2, '', '', '...', ?) AS snippet,
                bm25(notes_fts_bm25, {title}, {alias}, {content}) AS rank
         FROM notes_fts_bm25
         JOIN notes n ON n.id = notes_fts_bm25.rowid
         WHERE notes_fts_bm25 MATCH ? AND n.active = 1
         ORDER BY rank
         LIMIT ?",
        title = BM25_FTS_SCORES.title,
        alias = BM25_FTS_SCORES.alias,
        content = BM25_FTS_SCORES.content,
    );
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return Vec::new();
    };
    let Ok(mapped) = stmt.query_map(
        params![num_tokens, fts_query, limit],
        |row| -> rusqlite::Result<RawSearchResult> {
            let path: String = row.get(0)?;
            let title: Option<String> = row.get(1)?;
            let tags: Option<String> = row.get(2)?;
            let aliases: Option<String> = row.get(3)?;
            let snippet: Option<String> = row.get(4)?;
            let rank: f64 = row.get(5)?;
            let score = build_bm25_score(rank);
            Ok(RawSearchResult {
                path,
                title: title.unwrap_or_default(),
                tags: parse_string_array(tags),
                aliases: parse_string_array(aliases),
                snippet: snippet.unwrap_or_default(),
                score,
                scores: SearchScores {
                    bm25: Some(score),
                    ..Default::default()
                },
            })
        },
    ) else {
        return Vec::new();
    };
    mapped.filter_map(Result::ok).collect()
}

/// Looks up notes whose normalized alias exactly matches `query`.
///
/// "Normalized" means NFD-decomposed lowercased form (per
/// [`crate::frontmatter::normalize_keyword`]). The alias normalization is
/// expected to already be present in `note_aliases.alias_norm` (written by
/// the indexer's alias upsert path).
///
/// Returns an empty vector on SQL errors.
#[must_use]
pub fn search_by_alias_exact(conn: &Connection, query: &str, limit: u32) -> Vec<RawSearchResult> {
    let normalized = normalize_keyword(query);
    let sql = "SELECT DISTINCT n.vault_path, n.title, n.tags, n.aliases
               FROM note_aliases a
               JOIN notes n ON n.id = a.note_id
               WHERE a.alias_norm = ? AND n.active = 1
               LIMIT ?";
    let Ok(mut stmt) = conn.prepare(sql) else {
        return Vec::new();
    };
    let Ok(mapped) = stmt.query_map(
        params![normalized, limit],
        |row| -> rusqlite::Result<RawSearchResult> {
            let path: String = row.get(0)?;
            let title: Option<String> = row.get(1)?;
            let tags: Option<String> = row.get(2)?;
            let aliases: Option<String> = row.get(3)?;
            Ok(RawSearchResult {
                path,
                title: title.unwrap_or_default(),
                tags: parse_string_array(tags),
                aliases: parse_string_array(aliases),
                snippet: String::new(),
                score: 1.0,
                scores: SearchScores {
                    fuzzy_title: Some(1.0),
                    ..Default::default()
                },
            })
        },
    ) else {
        return Vec::new();
    };
    mapped.filter_map(Result::ok).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::store::open_database;
    use std::env::temp_dir;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_path() -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        temp_dir().join(format!("talon-bm25-test-{pid}-{n}.sqlite"))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = fs_err::remove_file(path);
        let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
    }

    fn insert_note(
        conn: &Connection,
        vault_path: &str,
        title: &str,
        content: &str,
        aliases_json: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO notes
             (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
             VALUES (?, ?, '[]', ?, ?, 0, 0, 'h', 'd', 1)",
            params![vault_path, title, aliases_json, content],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn insert_alias(conn: &Connection, note_id: i64, alias: &str) {
        let norm = normalize_keyword(alias);
        conn.execute(
            "INSERT INTO note_aliases (note_id, alias, alias_norm) VALUES (?, ?, ?)",
            params![note_id, alias, norm],
        )
        .unwrap();
    }

    #[test]
    fn bm25_returns_matching_notes_and_score_in_unit_interval() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        insert_note(
            &conn,
            "a.md",
            "Atomic Notes",
            "atomic notes are small",
            "[]",
        );
        insert_note(&conn, "b.md", "Other", "completely different content", "[]");

        let results = search_bm25(&conn, "atomic notes", 10, 300);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "a.md");
        assert!(results[0].score > 0.0 && results[0].score <= 1.0);
        assert!(results[0].scores.bm25.is_some());
        assert!(!results[0].snippet.is_empty());
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn bm25_matches_or_so_more_terms_means_better_rank() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        insert_note(&conn, "both.md", "Foo Bar", "alpha beta", "[]");
        insert_note(&conn, "one.md", "Foo Only", "alpha gamma", "[]");

        let results = search_bm25(&conn, "alpha beta", 10, 300);
        let both = results.iter().position(|r| r.path == "both.md").unwrap();
        let one = results.iter().position(|r| r.path == "one.md").unwrap();
        assert!(both < one, "both.md should outrank one.md");
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn bm25_ignores_inactive_notes() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        insert_note(&conn, "a.md", "Atomic", "atomic notes", "[]");
        conn.execute("UPDATE notes SET active = 0 WHERE vault_path = 'a.md'", [])
            .unwrap();
        let results = search_bm25(&conn, "atomic", 10, 300);
        assert!(results.is_empty());
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn alias_exact_finds_normalized_match() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        let id = insert_note(
            &conn,
            "a.md",
            "Atomic Notes",
            "body",
            "[\"Atomic\",\"Zettel\"]",
        );
        insert_alias(&conn, id, "Atomic");
        insert_alias(&conn, id, "Zettel");

        // Lookup by lowercased input — should still hit via NFD+lower.
        let results = search_by_alias_exact(&conn, "atomic", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "a.md");
        assert!((results[0].score - 1.0).abs() < f64::EPSILON);
        // Aliases column was JSON-decoded.
        assert_eq!(results[0].aliases, vec!["Atomic", "Zettel"]);
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn alias_exact_misses_non_match() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        let id = insert_note(&conn, "a.md", "Atomic", "body", "[]");
        insert_alias(&conn, id, "Atomic");
        assert!(search_by_alias_exact(&conn, "completely-other", 10).is_empty());
        drop(conn);
        cleanup(&path);
    }
}
