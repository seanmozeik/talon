//! Real search handler for the Talon CLI.
//!
//! Implements all four search modes (hybrid, semantic, fulltext, title),
//! post-filters (--where, --since), and scope priority multiplication.
//!
//! Ports the search command from `services/talon/search/command.ts`.

use rusqlite::Connection;

use crate::cache::search as search_cache;
use crate::config::TalonConfig;
use crate::contracts::VaultPath;
use crate::expansion::client::ExpansionClient;
use crate::inference::InferenceClient;
use crate::numeric::count_u32;
use crate::search::anchor::{build_anchors, maybe_expand_bm25_snippet, resolve_snippet_heading};
use crate::search::{
    MatchKind, SearchInput, SearchMode, SearchResponse, SearchResult, WhereClause,
};

use crate::search::bm25::search_bm25;
use crate::search::constants::DEFAULT_SNIPPET_LENGTH;
use crate::search::fuzzy_title::search_fuzzy_title;
use crate::search::hybrid_pipeline::{HybridPipelineOptions, run_hybrid_pipeline_with_metadata};
use crate::search::pool;
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
#[allow(clippy::missing_errors_doc)]
pub fn run_search(
    conn: &Connection,
    input: &SearchInput,
    inference: Option<&InferenceClient>,
    expansion: Option<&ExpansionClient>,
    config: Option<&TalonConfig>,
) -> SearchResponse {
    run_search_inner(conn, input, inference, expansion, config, false)
}

#[allow(clippy::missing_errors_doc)]
pub fn run_search_with_expanded_queries(
    conn: &Connection,
    input: &SearchInput,
    inference: Option<&InferenceClient>,
    expansion: Option<&ExpansionClient>,
    config: Option<&TalonConfig>,
) -> SearchResponse {
    run_search_inner(conn, input, inference, expansion, config, true)
}

fn run_search_inner(
    conn: &Connection,
    input: &SearchInput,
    inference: Option<&InferenceClient>,
    expansion: Option<&ExpansionClient>,
    config: Option<&TalonConfig>,
    include_expanded_queries: bool,
) -> SearchResponse {
    let query = match &input.query {
        Some(q) if !q.trim().is_empty() => q.clone(),
        _ => return SearchResponse::empty_input(),
    };

    let use_cache = inference.is_some() && !include_expanded_queries;
    if use_cache && let Some(response) = search_cache::lookup(conn, input, config) {
        return response;
    }

    let limit = u32::from(input.limit.get());
    let candidate_floor = u32::from(input.candidate_limit.get());
    let fast = input.fast;

    // Step 1: retrieve wide pool.
    let mut expanded_queries = Vec::new();
    let raw_results: Vec<RawSearchResult> = match input.mode {
        SearchMode::Hybrid if fast => search_bm25(
            conn,
            &query,
            pool::bm25_pool(limit, candidate_floor),
            DEFAULT_SNIPPET_LENGTH,
        ),
        SearchMode::Hybrid => {
            let Some(inference) = inference else {
                return SearchResponse {
                    vault: None,
                    query: Some(query),
                    mode: input.mode,
                    fast,
                    expanded: false,
                    expanded_queries: Vec::new(),
                    reranked: false,
                    index_version: "1".to_string(),
                    total: 0,
                    results: Vec::new(),
                };
            };
            let opts = HybridPipelineOptions {
                limit,
                candidate_limit: candidate_floor,
                fast,
                queries: input.queries.clone(),
                intent: input.intent.clone(),
                hooks: crate::search::SearchHooks::default(),
            };
            let output =
                run_hybrid_pipeline_with_metadata(conn, inference, expansion, &query, &opts);
            if include_expanded_queries {
                expanded_queries = output.expanded_queries;
            }
            output.results
        }
        SearchMode::Semantic => {
            let Some(inference) = inference else {
                return SearchResponse::empty_input();
            };
            let Ok(embeddings) = inference.embed(std::slice::from_ref(&query)) else {
                return SearchResponse::empty_input();
            };
            let embedding = embeddings.first().map_or(&[] as &[f32], Vec::as_slice);
            search_vector(conn, embedding, pool::vector_pool(limit, candidate_floor))
        }
        SearchMode::Fulltext => search_bm25(
            conn,
            &query,
            pool::bm25_pool(limit, candidate_floor),
            DEFAULT_SNIPPET_LENGTH,
        ),
        SearchMode::Title => {
            search_fuzzy_title(conn, &query, pool::fuzzy_pool(limit, candidate_floor))
        }
    };

    // Step 2: apply --where filter (no truncation).
    let filtered = apply_where_filter(raw_results, &input.where_, conn);
    // Step 3: apply --since filter (no truncation).
    let filtered = apply_since_filter(filtered, input.since.as_deref(), conn);
    // Step 4: scope priority multiplication.
    let scored = apply_scope_priority(filtered, config);

    // Step 5: total is post-filter, pre-truncate.
    let total = count_u32(scored.len());
    // Step 6: final output trim.
    let mut scored = scored;
    scored.truncate(limit as usize);

    let expanded = (expansion.is_some() || !input.queries.is_empty())
        && !input.fast
        && input.mode == SearchMode::Hybrid;
    let reranked = input.mode == SearchMode::Hybrid && !input.fast;

    let anchors_requested = input.anchors.unwrap_or(false);
    let response = SearchResponse {
        vault: None,
        query: Some(query.clone()),
        mode: input.mode,
        fast,
        expanded,
        expanded_queries,
        reranked,
        index_version: "1".to_string(),
        total,
        results: scored
            .into_iter()
            .filter_map(|r| raw_to_search_result(&r, input.mode, conn, anchors_requested, &query))
            .collect(),
    };
    if use_cache {
        search_cache::store(conn, input, config, &response);
    }
    response
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
    query: &str,
) -> Option<SearchResult> {
    let match_kind = match mode {
        SearchMode::Hybrid | SearchMode::Fulltext => MatchKind::Fulltext,
        SearchMode::Semantic => MatchKind::Semantic,
        SearchMode::Title => MatchKind::Title,
    };

    let mut snippet = raw.snippet.clone();
    if matches!(mode, SearchMode::Hybrid | SearchMode::Fulltext)
        && raw.scores.bm25.is_some()
        && snippet.chars().count() * 2 < DEFAULT_SNIPPET_LENGTH as usize
        && let Some(note_id) = get_note_id_by_path(conn, &raw.path)
        && let Some(fallback) = maybe_expand_bm25_snippet(conn, note_id, query, &snippet)
    {
        snippet = fallback;
    }

    // Heading breadcrumb prepended unconditionally (independent of anchors flag).
    let heading = resolve_snippet_heading(conn, raw, &snippet);
    if let Some(ref h) = heading
        && !h.is_empty()
    {
        snippet = format!("{h}\n{snippet}");
    }

    // Algorithm ported verbatim from obsidian-hybrid-search (MIT) — searcher.ts:1209.
    let snippet = snippet
        .chars()
        .take(DEFAULT_SNIPPET_LENGTH as usize)
        .collect::<String>();

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

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
