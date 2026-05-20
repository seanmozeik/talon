//! Hybrid-mode helpers split out of `search.rs` to keep that file under the
//! repo's per-file line budget. These helpers wrap the call into the search
//! pipeline and project its diagnostics into the verbose-only response shape.

use rusqlite::Connection;

use crate::expansion::client::ExpansionClient;
use crate::inference::InferenceClient;
use crate::search::hybrid_pipeline::{HybridPipelineOptions, run_hybrid_pipeline_with_metadata};
use crate::search::pre_filter::PreFilter;
use crate::search::types::{RawSearchResult, SearchScores};
use crate::search::{MatchKind, SearchDiagnostics, SearchInput, SearchMode, SearchResponse};

pub(super) enum HybridOutcome {
    NoInference,
    Ok {
        results: Vec<RawSearchResult>,
        expanded_queries: Vec<String>,
        diagnostics: Option<SearchDiagnostics>,
    },
}

pub(super) struct HybridArgs<'a> {
    pub(super) conn: &'a Connection,
    pub(super) input: &'a SearchInput,
    pub(super) inference: Option<&'a InferenceClient>,
    pub(super) expansion: Option<&'a ExpansionClient>,
    pub(super) query: &'a str,
    pub(super) limit: u32,
    pub(super) candidate_floor: u32,
    pub(super) fast: bool,
    pub(super) include_expanded_queries: bool,
    pub(super) pre_filter: PreFilter,
}

pub(super) fn run_hybrid_mode(args: &HybridArgs<'_>) -> HybridOutcome {
    let Some(inference) = args.inference else {
        return HybridOutcome::NoInference;
    };
    let opts = HybridPipelineOptions {
        limit: args.limit,
        candidate_limit: args.candidate_floor,
        fast: args.fast,
        skip_expansion: false,
        queries: args.input.queries.clone(),
        intent: args.input.intent.clone(),
        hooks: crate::search::SearchHooks::default(),
        pre_filter: args.pre_filter.clone(),
        deadline_at: None,
    };
    let output =
        run_hybrid_pipeline_with_metadata(args.conn, inference, args.expansion, args.query, &opts);
    let (expanded_queries, diagnostics) = if args.include_expanded_queries {
        let diag = SearchDiagnostics {
            expansion_ms: output.expansion_ms,
            strong_signal_score: output.strong_signal_score,
            rerank_candidates: output.rerank_candidates,
            rerank_ms: output.rerank_ms,
            graph: None,
        };
        let diag = (!diag.is_empty()).then_some(diag);
        (output.expanded_queries, diag)
    } else {
        (Vec::new(), None)
    };
    HybridOutcome::Ok {
        results: output.results,
        expanded_queries,
        diagnostics,
    }
}

/// Picks a match kind for a hybrid result based on which retrieval signal
/// dominated for that note.
///
/// All three buckets (`bm25`, `fuzzy_title`, `semantic`) are normalized to
/// `[0, 1]` (see `types.rs` field docs), so a direct max comparison is fair.
/// A title hit wins when its score is the largest non-zero contribution —
/// title provenance is the most useful signal for the consumer ("the note's
/// *title* matched"). Otherwise the larger of semantic vs BM25 wins, ties to
/// Fulltext to keep ranking deterministic.
pub(super) fn infer_hybrid_match_kind(scores: &SearchScores) -> MatchKind {
    let title = scores.fuzzy_title.unwrap_or(0.0);
    let bm25 = scores.bm25.unwrap_or(0.0);
    let semantic = scores.semantic.unwrap_or(0.0);

    if title > 0.0 && title >= bm25 && title >= semantic {
        MatchKind::Title
    } else if semantic > bm25 {
        MatchKind::Semantic
    } else if bm25 > 0.0 {
        MatchKind::Fulltext
    } else if semantic > 0.0 {
        MatchKind::Semantic
    } else {
        // Post-fusion candidates always have at least one signal, but if the
        // breakdown is empty for any reason fall back to the safe default.
        MatchKind::Fulltext
    }
}

pub(super) fn empty_hybrid_response(query: String, mode: SearchMode, fast: bool) -> SearchResponse {
    SearchResponse {
        vault: None,
        query: Some(query),
        mode,
        fast,
        expanded: false,
        expanded_queries: Vec::new(),
        reranked: false,
        index_version: "1".to_string(),
        total: 0,
        results: Vec::new(),
        diagnostics: None,
    }
}
