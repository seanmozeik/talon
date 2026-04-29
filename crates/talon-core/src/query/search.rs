//! Real search handler for the Talon CLI.
//!
//! Implements all four search modes (hybrid, semantic, fulltext, title),
//! pre-filters (--where, --since, --scope) applied at retrieval time, and
//! scope priority multiplication.
//!
//! Ports the search command from `services/talon/search/command.ts`.

use rusqlite::Connection;

use crate::cache::search as search_cache;
use crate::config::{ScopeFilter, TalonConfig};
use crate::contracts::VaultPath;
use crate::expansion::client::ExpansionClient;
use crate::inference::InferenceClient;
use crate::numeric::count_u32;
use crate::search::anchor::{build_anchors, maybe_expand_bm25_snippet, resolve_snippet_heading};
use crate::search::pre_filter::{PreFilter, scope_to_note_ids};
use crate::search::{MatchKind, SearchInput, SearchMode, SearchResponse, SearchResult};

use super::search_hybrid::{empty_hybrid_response, infer_hybrid_match_kind};
use super::search_retrieval::{RetrievalOutcome, retrieve_raw_results};
use crate::search::constants::DEFAULT_SNIPPET_LENGTH;
use crate::search::types::RawSearchResult;

struct ScoredRawSearchResult {
    raw: RawSearchResult,
    raw_score: f64,
}

/// Runs a search query and returns a [`SearchResponse`].
///
/// Supports four modes:
/// - **hybrid** (default): full pipeline with expansion + RRF + rerank.
/// - **semantic**: vector-only search via the inference sidecar.
/// - **fulltext**: BM25 lexical search only.
/// - **title**: fuzzy title/alias search only.
///
/// `--where`, `--since`, and scope filters are pre-computed into a
/// [`PreFilter`] and pushed into retrieval SQL. Scope priority multiplication
/// is applied post-retrieval as a score modifier.
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

    // Build pre-filter: resolve --since, scope, and --where into SQL constraints
    // pushed into every retrieval query so the candidate pool is already scoped.
    let since_ms = input
        .since
        .as_deref()
        .and_then(|s| crate::indexing::change_tracking::parse_since(s).ok());
    let accepted_note_ids = config.and_then(|cfg| {
        let filter = ScopeFilter::from_args(cfg, &input.scope, &input.scope_only, input.scope_all)
            .unwrap_or_else(|_| ScopeFilter::default_for(cfg));
        scope_to_note_ids(conn, &filter)
    });
    let pre_filter = PreFilter {
        since_ms,
        accepted_note_ids,
        where_clauses: input.where_.clone(),
    };

    // Step 1: retrieve wide pool with pre-filters applied at SQL level.
    let (raw_results, expanded_queries, diagnostics) = match retrieve_raw_results(
        conn,
        input,
        &pre_filter,
        inference,
        expansion,
        &query,
        limit,
        candidate_floor,
        fast,
        include_expanded_queries,
    ) {
        RetrievalOutcome::Empty => return SearchResponse::empty_input(),
        RetrievalOutcome::EmptyHybrid => return empty_hybrid_response(query, input.mode, fast),
        RetrievalOutcome::Ok {
            results,
            expanded_queries,
            diagnostics,
        } => (results, expanded_queries, diagnostics),
    };

    // Step 2: scope priority multiplication (score modifier, not filter).
    let scored = apply_scope_priority(raw_results, config);

    // Step 6: total is post-filter, pre-truncate.
    let total = count_u32(scored.len());
    // Step 7: final output trim.
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
            .filter_map(|r| {
                raw_to_search_result(
                    &r.raw,
                    input.mode,
                    conn,
                    anchors_requested,
                    &query,
                    r.raw_score,
                    config,
                )
            })
            .collect(),
        diagnostics,
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
    raw_score: f64,
    config: Option<&TalonConfig>,
) -> Option<SearchResult> {
    let match_kind = match mode {
        SearchMode::Hybrid => infer_hybrid_match_kind(&raw.scores),
        SearchMode::Fulltext => MatchKind::Fulltext,
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

    let scope = config
        .and_then(|cfg| cfg.resolve_scope_name(std::path::Path::new(&raw.path)))
        .map(str::to_string);
    let mtime = super::mtime::local_mtime_for_path(conn, &raw.path);

    Some(SearchResult {
        vault_path: VaultPath::parse(&raw.path).ok()?,
        title: raw.title.clone(),
        snippet,
        score: raw.score,
        raw_score: Some(raw_score),
        match_kind,
        scope,
        mtime,
        preview_anchors,
    })
}

/// Multiplies each result's score by the scope priority multiplier.
///
/// Uses the `TalonConfig` to resolve the scope for each vault path.
/// Falls back to normal (1.0x) when no config is provided.
fn apply_scope_priority(
    results: Vec<RawSearchResult>,
    config: Option<&TalonConfig>,
) -> Vec<ScoredRawSearchResult> {
    let Some(cfg) = config else {
        return results
            .into_iter()
            .map(|raw| {
                let raw_score = raw.score;
                ScoredRawSearchResult { raw, raw_score }
            })
            .collect();
    };

    results
        .into_iter()
        .map(|mut raw| {
            let raw_score = raw.score;
            let resolution = cfg.resolve_scope(std::path::Path::new(&raw.path));
            let multiplier = resolution.priority.multiplier();
            raw.score *= multiplier;
            ScoredRawSearchResult { raw, raw_score }
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

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
