//! Real search handler for the Talon CLI.
//!
//! Implements all four search modes (hybrid, semantic, fulltext, title),
//! post-filters (--where, --since), and scope priority multiplication.
//!
//! Ports the search command from `services/talon/search/command.ts`.

use rusqlite::Connection;

use crate::config::TalonConfig;
use crate::contracts::VaultPath;
use crate::expansion::client::ExpansionClient;
use crate::inference::InferenceClient;
use crate::numeric::count_u32;
use crate::search::anchor::{build_anchors, resolve_snippet_heading};
use crate::search::{
    MatchKind, SearchInput, SearchMode, SearchResponse, SearchResult, WhereClause,
};

use crate::search::bm25::search_bm25;
use crate::search::constants::DEFAULT_SNIPPET_LENGTH;
use crate::search::fuzzy_title::search_fuzzy_title;
use crate::search::hybrid_pipeline::{HybridPipelineOptions, run_hybrid_pipeline};
use crate::search::types::RawSearchResult;
use crate::search::vector::search_vector;

/// Runs a search query and returns a [`SearchResponse`].
///
/// Supports four modes:
/// - **hybrid** (default): full pipeline with expansion + RRF + rerank.
/// - **semantic**: vector-only search via the inference sidecar.
/// - **fulltext**: BM25 lexical search only.
/// - **title**: fuzzy title/alias search only.
///
/// Post-filters (`--where`, `--since`) and scope priority multiplication are
/// applied after retrieval.
///
/// # Errors
///
/// Returns an error if the query is empty or the mode is invalid.
#[allow(clippy::missing_errors_doc)]
pub fn run_search(
    conn: &Connection,
    input: &SearchInput,
    inference: Option<&InferenceClient>,
    expansion: Option<&ExpansionClient>,
    config: Option<&TalonConfig>,
) -> SearchResponse {
    let query = match &input.query {
        Some(q) if !q.trim().is_empty() => q.clone(),
        _ => return SearchResponse::empty_input(),
    };

    let limit = u32::from(input.limit.get());
    let fast = input.fast;

    let raw_results: Vec<RawSearchResult> = match input.mode {
        SearchMode::Hybrid if fast => search_bm25(conn, &query, limit, DEFAULT_SNIPPET_LENGTH),
        SearchMode::Hybrid => {
            let Some(inference) = inference else {
                return SearchResponse {
                    vault: None,
                    query: Some(query),
                    mode: input.mode,
                    fast,
                    expanded: false,
                    reranked: false,
                    index_version: "1".to_string(),
                    total: 0,
                    results: Vec::new(),
                };
            };
            let opts = HybridPipelineOptions {
                limit,
                fast,
                queries: input.queries.clone(),
            };
            run_hybrid_pipeline(conn, inference, expansion, &query, &opts)
        }
        SearchMode::Semantic => {
            let Some(inference) = inference else {
                return SearchResponse::empty_input();
            };
            let Ok(embeddings) = inference.embed(std::slice::from_ref(&query)) else {
                return SearchResponse::empty_input();
            };
            let embedding = embeddings.first().map_or(&[] as &[f32], Vec::as_slice);
            search_vector(conn, embedding, limit)
        }
        SearchMode::Fulltext => search_bm25(conn, &query, limit, DEFAULT_SNIPPET_LENGTH),
        SearchMode::Title => search_fuzzy_title(conn, &query, limit),
    };

    let filtered = apply_where_filter(raw_results, &input.where_, conn);
    let filtered = apply_since_filter(filtered, input.since.as_deref(), conn);

    let scored = apply_scope_priority(filtered, config);

    let total = count_u32(scored.len());
    let mut scored = scored;
    scored.truncate(limit as usize);

    let expanded = (expansion.is_some() || !input.queries.is_empty())
        && !input.fast
        && input.mode == SearchMode::Hybrid;
    let reranked = input.mode == SearchMode::Hybrid && !input.fast;

    let anchors_requested = input.anchors.unwrap_or(false);
    SearchResponse {
        vault: None,
        query: Some(query),
        mode: input.mode,
        fast,
        expanded,
        reranked,
        index_version: "1".to_string(),
        total,
        results: scored
            .into_iter()
            .filter_map(|r| raw_to_search_result(&r, input.mode, conn, anchors_requested))
            .collect(),
    }
}

/// Converts a [`RawSearchResult`] to a [`SearchResult`] for the response.
///
/// - Prepends a heading breadcrumb to the snippet unconditionally when one
///   can be resolved (ports searcher.ts:265-273).
/// - Populates `preview_anchors` when `anchors_requested` is true.
///
/// Returns `None` if the path stored in the database cannot be parsed (corrupt data).
fn raw_to_search_result(
    raw: &RawSearchResult,
    mode: SearchMode,
    conn: &Connection,
    anchors_requested: bool,
) -> Option<SearchResult> {
    let match_kind = match mode {
        SearchMode::Hybrid | SearchMode::Fulltext => MatchKind::Fulltext,
        SearchMode::Semantic => MatchKind::Semantic,
        SearchMode::Title => MatchKind::Title,
    };

    // Heading breadcrumb prepended unconditionally (independent of anchors flag).
    let heading = resolve_snippet_heading(conn, raw);
    let snippet = match heading {
        Some(ref h) if !h.is_empty() => format!("{h}\n{}", raw.snippet),
        _ => raw.snippet.clone(),
    };

    let preview_anchors = if anchors_requested {
        let anchors = build_anchors(conn, raw);
        if anchors.is_empty() {
            None
        } else {
            Some(anchors)
        }
    } else {
        None
    };

    Some(SearchResult {
        vault_path: VaultPath::parse(&raw.path).ok()?,
        title: raw.title.clone(),
        snippet,
        score: raw.score,
        raw_score: Some(raw.score),
        match_kind,
        scope: None,
        preview_anchors,
    })
}

/// Applies `--where` frontmatter filters to the result list.
///
/// For each result, queries `note_frontmatter_fields` to check if the
/// specified field matches the operator/value. All clauses are AND-composed.
fn apply_where_filter(
    results: Vec<RawSearchResult>,
    clauses: &[WhereClause],
    conn: &Connection,
) -> Vec<RawSearchResult> {
    if clauses.is_empty() {
        return results;
    }

    results
        .into_iter()
        .filter(|r| {
            let note_id = get_note_id_by_path(conn, &r.path);
            let Some(note_id) = note_id else {
                return false;
            };
            super::where_filter::passes_where_clauses(conn, note_id, clauses)
        })
        .collect()
}

/// Applies `--since` timestamp filter.
///
/// Returns only results whose note `mtime_ms` >= the parsed timestamp.
fn apply_since_filter(
    results: Vec<RawSearchResult>,
    since: Option<&str>,
    conn: &Connection,
) -> Vec<RawSearchResult> {
    let Some(since_str) = since else {
        return results;
    };

    // Invalid timestamp: pass results through unchanged.
    let Ok(timestamp) = crate::indexing::change_tracking::parse_since(since_str) else {
        return results;
    };

    results
        .into_iter()
        .filter(|r| {
            let note_id = get_note_id_by_path(conn, &r.path);
            let Some(note_id) = note_id else {
                return false;
            };
            get_note_mtime(conn, note_id).is_some_and(|mtime| mtime >= timestamp)
        })
        .collect()
}

/// Multiplies each result's score by the scope priority multiplier.
///
/// Uses the `TalonConfig` to resolve the scope for each vault path.
/// Falls back to normal (1.0x) when no config is provided.
fn apply_scope_priority(
    results: Vec<RawSearchResult>,
    config: Option<&TalonConfig>,
) -> Vec<RawSearchResult> {
    let Some(cfg) = config else {
        return results;
    };

    results
        .into_iter()
        .map(|mut r| {
            let resolution = cfg.resolve_scope(std::path::Path::new(&r.path));
            let multiplier = resolution.priority.multiplier();
            r.score *= multiplier;
            r
        })
        .collect()
}

/// Looks up a note's rowid by `vault_path`.
fn get_note_id_by_path(conn: &Connection, vault_path: &str) -> Option<i64> {
    conn.query_row(
        "SELECT id FROM notes WHERE vault_path = ? AND active = 1",
        [vault_path],
        |row| row.get(0),
    )
    .ok()
}

/// Looks up a note's `mtime_ms` by rowid.
fn get_note_mtime(conn: &Connection, note_id: i64) -> Option<u64> {
    conn.query_row(
        "SELECT mtime_ms FROM notes WHERE id = ? AND active = 1",
        [note_id],
        |row| row.get(0),
    )
    .ok()
}
