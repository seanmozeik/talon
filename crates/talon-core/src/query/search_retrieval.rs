//! Wide-pool retrieval stage for the search command.
//!
//! Dispatches over the four `SearchMode` variants and produces raw candidates
//! before post-filters and scoring.

use rusqlite::Connection;

use crate::expansion::client::ExpansionClient;
use crate::inference::{EmbeddingClient, RerankClient};
use crate::search::bm25::search_bm25;
use crate::search::constants::DEFAULT_SNIPPET_LENGTH;
use crate::search::fuzzy_title::search_fuzzy_title;
use crate::search::pool;
use crate::search::pre_filter::PreFilter;
use crate::search::types::RawSearchResult;
use crate::search::vector::search_vector;
use crate::search::{SearchInput, SearchMode};

use super::search_hybrid::{HybridArgs, HybridOutcome, run_hybrid_mode};

/// Outcome of the wide-pool retrieval stage.
pub(super) enum RetrievalOutcome {
    /// Empty input or semantic without inference; caller returns `empty_input`.
    Empty,
    /// Hybrid without inference; caller returns `empty_hybrid_response`.
    EmptyHybrid,
    /// Ok with raw results plus optional expansion/diagnostic byproducts.
    Ok {
        results: Vec<RawSearchResult>,
        expanded_queries: Vec<String>,
        diagnostics: Option<crate::search::SearchDiagnostics>,
    },
}

#[allow(clippy::too_many_arguments)]
pub(super) fn retrieve_raw_results(
    conn: &Connection,
    input: &SearchInput,
    pre_filter: &PreFilter,
    embedding: Option<&EmbeddingClient>,
    rerank: Option<&RerankClient>,
    expansion: Option<&ExpansionClient>,
    query: &str,
    limit: u32,
    candidate_floor: u32,
    fast: bool,
    include_expanded_queries: bool,
) -> RetrievalOutcome {
    let raw = match input.mode {
        SearchMode::Hybrid if fast => search_bm25(
            conn,
            query,
            pool::bm25_pool(limit, candidate_floor),
            DEFAULT_SNIPPET_LENGTH,
            pre_filter,
        ),
        SearchMode::Hybrid => {
            return match run_hybrid_mode(&HybridArgs {
                conn,
                input,
                embedding,
                rerank,
                expansion,
                query,
                limit,
                candidate_floor,
                fast,
                include_expanded_queries,
                pre_filter: pre_filter.clone(),
            }) {
                HybridOutcome::NoInference => RetrievalOutcome::EmptyHybrid,
                HybridOutcome::Ok {
                    results,
                    expanded_queries,
                    diagnostics,
                } => RetrievalOutcome::Ok {
                    results,
                    expanded_queries,
                    diagnostics,
                },
            };
        }
        SearchMode::Semantic => {
            let Some(embedding) = embedding else {
                return RetrievalOutcome::Empty;
            };
            let Ok(embeddings) = embedding.embed(std::slice::from_ref(&query.to_string())) else {
                return RetrievalOutcome::Empty;
            };
            let embedding = embeddings.first().map_or(&[] as &[f32], Vec::as_slice);
            search_vector(
                conn,
                embedding,
                pool::vector_pool(limit, candidate_floor),
                pre_filter,
            )
        }
        SearchMode::Fulltext => search_bm25(
            conn,
            query,
            pool::bm25_pool(limit, candidate_floor),
            DEFAULT_SNIPPET_LENGTH,
            pre_filter,
        ),
        SearchMode::Title => search_fuzzy_title(
            conn,
            query,
            pool::fuzzy_pool(limit, candidate_floor),
            pre_filter,
        ),
    };
    RetrievalOutcome::Ok {
        results: raw,
        expanded_queries: Vec::new(),
        diagnostics: None,
    }
}
