//! Trigram fuzzy title/alias retrieval against `notes_fts_fuzzy`.
//!
//! Ports `services/talon/search/fuzzy-title.ts`. Two outputs:
//!
//! - exact-alias hits (separated so the hybrid pipeline can give them the
//!   high `exactAlias` RRF weight),
//! - fuzzy hits scored by `bm25 × overlap²`, where `overlap` is the trigram
//!   intersection ratio between the query and the title or any alias.

use std::collections::HashSet;

use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

use super::bm25::search_by_alias_exact;
use super::constants::FUZZY_ALIAS_MIN_LEN;
use super::pre_filter::PreFilter;
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
pub fn search_title_parts(
    conn: &Connection,
    query: &str,
    limit: u32,
    pre_filter: &PreFilter,
) -> TitleSearchParts {
    if pre_filter.is_impossible() {
        return TitleSearchParts::default();
    }
    let exact_alias = search_by_alias_exact(conn, query, limit, pre_filter);
    let exact_alias_paths: HashSet<String> = exact_alias.iter().map(|r| r.path.clone()).collect();

    let fts_query = build_trigram_or_query(query);
    let (filter_sql, filter_params) = pre_filter.sql_fragment();
    let sql = format!(
        "SELECT n.vault_path, n.title, n.tags, n.aliases,
                bm25(notes_fts_fuzzy) AS rank
         FROM notes_fts_fuzzy
         JOIN notes n ON n.id = notes_fts_fuzzy.rowid
         WHERE notes_fts_fuzzy MATCH ? AND n.active = 1{filter_sql}
         ORDER BY rank
         LIMIT ?"
    );
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return TitleSearchParts {
            exact_alias,
            fuzzy: Vec::new(),
        };
    };
    let mut params: Vec<Value> = Vec::with_capacity(2 + filter_params.len());
    params.push(Value::Text(fts_query));
    params.extend(filter_params);
    params.push(Value::Integer(i64::from(limit)));
    let Ok(mapped) = stmt.query_map(params_from_iter(params), |row| {
        Ok(FuzzyRow {
            path: row.get(0)?,
            title: row.get(1)?,
            tags: row.get(2)?,
            aliases: row.get(3)?,
            rank: row.get(4)?,
        })
    }) else {
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
pub fn search_fuzzy_title(
    conn: &Connection,
    query: &str,
    limit: u32,
    pre_filter: &PreFilter,
) -> Vec<RawSearchResult> {
    let parts = search_title_parts(conn, query, limit, pre_filter);
    let mut out = parts.exact_alias;
    out.extend(parts.fuzzy);
    out.truncate(limit as usize);
    out
}

#[cfg(test)]
#[path = "fuzzy_title/tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]
mod tests;
