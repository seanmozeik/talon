//! Trigram fuzzy title/alias retrieval against `notes_fts_fuzzy`.
//!
//! Ports `services/talon/search/fuzzy-title.ts`. Two outputs:
//!
//! - exact-alias hits (separated so the hybrid pipeline can give them the
//!   high `exactAlias` RRF weight),
//! - fuzzy hits scored by `bm25 × overlap²`, where `overlap` is the trigram
//!   intersection ratio between the query and the title or any alias.

use std::collections::HashSet;

use rusqlite::{Connection, params};

use super::bm25::search_by_alias_exact;
use super::constants::FUZZY_ALIAS_MIN_LEN;
use super::text_fts::{build_bm25_score, build_trigram_or_query, calculate_trigram_overlap};
use super::types::{RawSearchResult, SearchScores};

/// Returns the maximum trigram overlap of `query` against any alias in
/// `aliases` whose length is ≥ [`FUZZY_ALIAS_MIN_LEN`].
fn max_alias_overlap(query: &str, aliases: &[String]) -> f64 {
    aliases
        .iter()
        .filter(|a| a.chars().count() >= FUZZY_ALIAS_MIN_LEN)
        .map(|a| calculate_trigram_overlap(query, a))
        .fold(0.0_f64, f64::max)
}

fn parse_string_array(raw: Option<String>) -> Vec<String> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default()
}

struct FuzzyRow {
    path: String,
    title: Option<String>,
    tags: Option<String>,
    aliases: Option<String>,
    rank: f64,
}

/// Bundle returned by [`search_title_parts`]: exact alias hits and fuzzy hits
/// kept separate so the hybrid pipeline can weight them independently.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TitleSearchParts {
    /// Exact alias matches (highest-confidence title signal).
    pub exact_alias: Vec<RawSearchResult>,
    /// Fuzzy trigram hits, with exact-alias paths excluded.
    pub fuzzy: Vec<RawSearchResult>,
}

/// Performs both retrieval steps and returns them split.
#[must_use]
pub fn search_title_parts(conn: &Connection, query: &str, limit: u32) -> TitleSearchParts {
    let exact_alias = search_by_alias_exact(conn, query, limit);
    let exact_alias_paths: HashSet<String> = exact_alias.iter().map(|r| r.path.clone()).collect();

    let fts_query = build_trigram_or_query(query);
    let sql = "SELECT n.vault_path, n.title, n.tags, n.aliases,
                      bm25(notes_fts_fuzzy) AS rank
               FROM notes_fts_fuzzy
               JOIN notes n ON n.id = notes_fts_fuzzy.rowid
               WHERE notes_fts_fuzzy MATCH ? AND n.active = 1
               ORDER BY rank
               LIMIT ?";
    let Ok(mut stmt) = conn.prepare_cached(sql) else {
        return TitleSearchParts {
            exact_alias,
            fuzzy: Vec::new(),
        };
    };
    let Ok(mapped) = stmt.query_map(
        params![fts_query, limit],
        |row| -> rusqlite::Result<FuzzyRow> {
            Ok(FuzzyRow {
                path: row.get(0)?,
                title: row.get(1)?,
                tags: row.get(2)?,
                aliases: row.get(3)?,
                rank: row.get(4)?,
            })
        },
    ) else {
        return TitleSearchParts {
            exact_alias,
            fuzzy: Vec::new(),
        };
    };

    let Ok(fuzzy_rows): rusqlite::Result<Vec<_>> = mapped.collect() else {
        return TitleSearchParts {
            exact_alias,
            fuzzy: Vec::new(),
        };
    };

    let mut fuzzy: Vec<RawSearchResult> = fuzzy_rows
        .into_iter()
        .filter(|row| !exact_alias_paths.contains(&row.path))
        .map(
            |FuzzyRow {
                 path,
                 title,
                 tags,
                 aliases,
                 rank,
             }| {
                let title = title.unwrap_or_default();
                let aliases_vec = parse_string_array(aliases);
                let title_overlap = calculate_trigram_overlap(query, &title);
                let alias_overlap = max_alias_overlap(query, &aliases_vec);
                let overlap = title_overlap.max(alias_overlap);
                let base = build_bm25_score(rank);
                let score = base * overlap * overlap;
                RawSearchResult {
                    path,
                    title,
                    tags: parse_string_array(tags),
                    aliases: aliases_vec,
                    snippet: String::new(),
                    score,
                    scores: SearchScores {
                        fuzzy_title: Some(score),
                        ..Default::default()
                    },
                    semantic_heading: None,
                    semantic_char_start: None,
                    semantic_char_end: None,
                }
            },
        )
        .filter(|r| r.score > 0.0)
        .collect();
    fuzzy.truncate(limit as usize);

    TitleSearchParts { exact_alias, fuzzy }
}

/// Convenience wrapper that returns the union of exact-alias and fuzzy
/// results, capped at `limit`. Use [`search_title_parts`] when the caller
/// needs to weight them independently.
#[must_use]
pub fn search_fuzzy_title(conn: &Connection, query: &str, limit: u32) -> Vec<RawSearchResult> {
    let parts = search_title_parts(conn, query, limit);
    let mut out = parts.exact_alias;
    out.extend(parts.fuzzy);
    out.truncate(limit as usize);
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::store::open_database;
    use crate::text::frontmatter::normalize_keyword;
    use rusqlite::params;
    use std::env::temp_dir;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_path() -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        temp_dir().join(format!("talon-fuzzy-test-{pid}-{n}.sqlite"))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = fs_err::remove_file(path);
        let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
    }

    fn insert_note(conn: &Connection, vault_path: &str, title: &str, aliases_json: &str) -> i64 {
        conn.execute(
            "INSERT INTO notes
             (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
             VALUES (?, ?, '[]', ?, ?, 0, 0, 'h', 'd', 1)",
            params![vault_path, title, aliases_json, title],
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
    fn max_alias_overlap_filters_short_aliases() {
        // "ab" is below FUZZY_ALIAS_MIN_LEN (3), so it should not contribute
        // even if it would have full overlap.
        let aliases = vec!["ab".into(), "atomic".into()];
        let overlap = max_alias_overlap("atomic", &aliases);
        assert_eq!(overlap, 1.0);
        let only_short = max_alias_overlap("ab", &["ab".to_string()]);
        assert_eq!(only_short, 0.0);
    }

    #[test]
    fn fuzzy_title_finds_close_match() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        insert_note(&conn, "a.md", "Zettelkasten", "[]");
        insert_note(&conn, "b.md", "Unrelated Topic", "[]");

        let parts = search_title_parts(&conn, "zettelkasten", 10);
        assert!(parts.fuzzy.iter().any(|r| r.path == "a.md"));
        // No exact-alias entry was inserted, so exact_alias is empty.
        assert!(parts.exact_alias.is_empty());
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn fuzzy_separates_exact_alias_from_fuzzy() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        let id = insert_note(&conn, "a.md", "Atomic Notes", "[\"Atomic\"]");
        insert_alias(&conn, id, "Atomic");

        let parts = search_title_parts(&conn, "atomic", 10);
        // Exact alias hit goes to exact_alias.
        assert_eq!(parts.exact_alias.len(), 1);
        assert_eq!(parts.exact_alias[0].path, "a.md");
        // a.md is excluded from fuzzy because it already matched exactly.
        assert!(parts.fuzzy.iter().all(|r| r.path != "a.md"));
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn search_fuzzy_title_unions_both_buckets() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        let id = insert_note(&conn, "a.md", "Atomic", "[\"Atomic\"]");
        insert_alias(&conn, id, "Atomic");
        insert_note(&conn, "b.md", "Atomically Inclined", "[]");

        let out = search_fuzzy_title(&conn, "atomic", 10);
        let paths: Vec<&str> = out.iter().map(|r| r.path.as_str()).collect();
        assert!(paths.contains(&"a.md"));
        assert!(paths.contains(&"b.md"));
        // Exact alias result keeps score=1.0; the fuzzy result is < 1.0.
        let a = out.iter().find(|r| r.path == "a.md").unwrap();
        let b = out.iter().find(|r| r.path == "b.md").unwrap();
        assert_eq!(a.score, 1.0);
        assert!(b.score > 0.0 && b.score < 1.0);
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn trigram_matches_accented_title_without_accent_in_query() {
        // "Cafe" (no accent) should fuzzy-match "Café del Sol" via trigram
        // overlap — the trigram tokenizer decomposes Unicode characters so 'e'
        // trigrams overlap with the composed 'é' form.
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        insert_note(&conn, "cafe.md", "Café del Sol", "[]");

        let parts = search_title_parts(&conn, "cafe", 10);
        assert!(
            parts.fuzzy.iter().any(|r| r.path == "cafe.md"),
            "trigram search should match accented title with unaccented query"
        );
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn trigram_cyrillic_substring_search() {
        // Cyrillic characters should be indexable and searchable without any
        // external collation — SQLite's trigram tokenizer is byte-aware and
        // treats each UTF-8 sequence as a token boundary.
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        insert_note(&conn, "ru.md", "Концепция zettelkasten", "[]");

        let parts = search_title_parts(&conn, "Концепция", 10);
        assert!(
            parts.fuzzy.iter().any(|r| r.path == "ru.md"),
            "trigram search should find Cyrillic title substring"
        );
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn trigram_overlap_squared_shorter_title_higher_score() {
        // "Atomic Notes" vs "atom" scores higher than "Notes on Atomic Habits"
        // vs "atom" because the shorter title has higher BM25 frequency and both
        // have full trigram overlap. The overlap² multiplier = 1.0 for both.
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        insert_note(&conn, "a.md", "Atomic Notes", "[]");
        insert_note(&conn, "b.md", "Notes on Atomic Habits", "[]");

        let parts = search_title_parts(&conn, "atom", 10);
        let a = parts.fuzzy.iter().find(|r| r.path == "a.md").unwrap();
        let b = parts.fuzzy.iter().find(|r| r.path == "b.md").unwrap();
        assert!(a.score > b.score);
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn trigram_overlap_squared_penalty_with_typo() {
        // Typo query "atomik" loses trigrams not in title, reducing overlap
        // and multiplying score by overlap². Perfect query "atomic" has
        // overlap=1.0, multiplier=1.0. Typo query should score lower.
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        insert_note(&conn, "a.md", "Atomic Notes", "[]");

        let perfect_parts = search_title_parts(&conn, "atomic", 10);
        let typo_parts = search_title_parts(&conn, "atomik", 10);

        let perfect = perfect_parts
            .fuzzy
            .iter()
            .find(|r| r.path == "a.md")
            .unwrap()
            .score;
        let typo = typo_parts
            .fuzzy
            .iter()
            .find(|r| r.path == "a.md")
            .unwrap()
            .score;
        assert!(typo < perfect);
        drop(conn);
        cleanup(&path);
    }
}
