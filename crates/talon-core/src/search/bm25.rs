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

use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

use crate::text::frontmatter::normalize_keyword;

use super::constants::{BM25_FTS_SCORES, BM25_MIN_TOKENS, BM25_TOKENS_PER_CHAR_DIV};
use super::pre_filter::PreFilter;
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
///
/// # CTE barrier — not needed at current vault scale
///
/// `SQLite`'s FTS5 planner correctly uses the FTS index when `notes_fts_bm25
/// MATCH ?` is combined with `JOIN notes WHERE n.active = 1`.  A `WITH
/// fts_matches AS (…)` barrier (ported from qmd `store.ts:3028-3046`) was
/// considered to force FTS materialisation before the JOIN in case the
/// planner fell back to a table scan on `notes`.
///
/// Criterion benchmarks at a 1 000-note fixture vault showed:
///   - unfiltered BM25 (`search_bm25` alone):  ~1.71 ms mean
///   - post-filtered via `--where status:active`: ~2.75 ms mean  (≈1.6× overhead)
///
/// The 1.6× overhead came from N per-hit `note_frontmatter_fields` lookups in
/// [`crate::query::where_filter`]. The filter is now a SQL EXISTS subquery via
/// [`PreFilter`], so that overhead is paid once per query rather than per result.
///
/// Re-examine if vault scale grows past ~10 000 notes and the EXISTS subqueries
/// prove slow: a CTE barrier (`WITH fts_matches AS (…)`) from qmd `store.ts:3028`
/// would force FTS materialisation before the joins.
#[must_use]
pub fn search_bm25(
    conn: &Connection,
    query: &str,
    limit: u32,
    snippet_length: u32,
    pre_filter: &PreFilter,
) -> Vec<RawSearchResult> {
    if pre_filter.is_impossible() {
        return Vec::new();
    }
    let num_tokens = BM25_MIN_TOKENS.max(snippet_length.div_ceil(BM25_TOKENS_PER_CHAR_DIV));
    let fts_query = to_fts_query(query, FtsOperator::Or);
    let (filter_sql, filter_params) = pre_filter.sql_fragment();
    let sql = format!(
        "SELECT n.vault_path, n.title, n.tags, n.aliases,
                snippet(notes_fts_bm25, 2, '', '', '...', ?) AS snippet,
                bm25(notes_fts_bm25, {title}, {alias}, {content}) AS rank
         FROM notes_fts_bm25
         JOIN notes n ON n.id = notes_fts_bm25.rowid
         WHERE notes_fts_bm25 MATCH ? AND n.active = 1{filter_sql}
         ORDER BY rank
         LIMIT ?",
        title = BM25_FTS_SCORES.title,
        alias = BM25_FTS_SCORES.alias,
        content = BM25_FTS_SCORES.content,
    );
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return Vec::new();
    };
    let mut params: Vec<Value> = Vec::with_capacity(3 + filter_params.len());
    params.push(Value::Integer(i64::from(num_tokens)));
    params.push(Value::Text(fts_query));
    params.extend(filter_params);
    params.push(Value::Integer(i64::from(limit)));
    let Ok(mapped) = stmt.query_map(params_from_iter(params), |row| {
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
            semantic_heading: None,
            semantic_char_start: None,
            semantic_char_end: None,
        })
    }) else {
        return Vec::new();
    };
    mapped.collect::<rusqlite::Result<_>>().unwrap_or_default()
}

/// Looks up notes whose normalized alias exactly matches `query`.
///
/// "Normalized" means NFD-decomposed lowercased form (per
/// [`crate::text::frontmatter::normalize_keyword`]). The alias normalization is
/// expected to already be present in `note_aliases.alias_norm` (written by
/// the indexer's alias upsert path).
///
/// Returns an empty vector on SQL errors.
#[must_use]
pub fn search_by_alias_exact(
    conn: &Connection,
    query: &str,
    limit: u32,
    pre_filter: &PreFilter,
) -> Vec<RawSearchResult> {
    if pre_filter.is_impossible() {
        return Vec::new();
    }
    let normalized = normalize_keyword(query);
    let (filter_sql, filter_params) = pre_filter.sql_fragment();
    let sql = format!(
        "SELECT DISTINCT n.vault_path, n.title, n.tags, n.aliases
         FROM note_aliases a
         JOIN notes n ON n.id = a.note_id
         WHERE a.alias_norm = ? AND n.active = 1{filter_sql}
         LIMIT ?"
    );
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return Vec::new();
    };
    let mut params: Vec<Value> = Vec::with_capacity(2 + filter_params.len());
    params.push(Value::Text(normalized));
    params.extend(filter_params);
    params.push(Value::Integer(i64::from(limit)));
    let Ok(mapped) = stmt.query_map(params_from_iter(params), |row| {
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
            semantic_heading: None,
            semantic_char_start: None,
            semantic_char_end: None,
        })
    }) else {
        return Vec::new();
    };
    mapped.collect::<rusqlite::Result<_>>().unwrap_or_default()
}

#[cfg(test)]
#[path = "bm25/tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
