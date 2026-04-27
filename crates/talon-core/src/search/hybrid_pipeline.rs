//! Full hybrid search pipeline orchestrator.
//!
//! Wires together a lexical probe, optional LLM expansion (US-001),
//! per-variant hybrid retrieval (US-003), cross-variant RRF fusion, and
//! cross-encoder reranking (US-004).
//!
//! Search progress hooks (US-018) are optional and default to no-op.
//!
//! Ports `services/talon/search/hybrid-pipeline.ts`.

use rusqlite::Connection;
use std::time::Instant;

use crate::expansion::client::ExpansionClient;
use crate::indexing::migrations::read_db_version;
use crate::inference::InferenceClient;

use super::bm25::search_bm25;
use super::cache::dedupe_query_variants;
use super::constants::{
    DEFAULT_SNIPPET_LENGTH, HYBRID_PROBE_LEXICAL_LIMIT, HYBRID_PROBE_TITLE_LIMIT, RERANK_TOP_K,
};
use super::fuse::{estimate_strong_signal, fuse_hybrid_result_lists};
use super::fuzzy_title::search_title_parts;
use super::hooks::SearchHooks;
use super::hybrid_single::{HybridSingleResult, run_hybrid_single};
use super::pool;
use super::rerank_pipeline::{IntentRerankOptions, rerank_candidates_with_intent};
use super::rrf::{RrfInputs, RrfList, RrfScoreAccumulator, normalize_and_merge_rrf_results};
use super::types::{HybridScoreData, RawSearchResult, SearchScores};

/// Default number of LLM expansion variants to request per query.
const EXPANSION_N_VARIANTS: u8 = 3;

/// Options for [`run_hybrid_pipeline`].
#[derive(Debug)]
pub struct HybridPipelineOptions {
    /// Maximum results to return.
    pub limit: u32,
    /// Candidate pool size for RRF/rerank over-fetch.
    pub candidate_limit: u32,
    /// Skip LLM expansion and cross-encoder reranking when true.
    pub fast: bool,
    /// Pre-supplied query variants (bypass LLM call when non-empty).
    pub queries: Vec<String>,
    /// Optional disambiguating context for expansion, rerank, and chunks.
    pub intent: Option<String>,
    /// Optional stage instrumentation callbacks.
    pub hooks: SearchHooks,
}

/// Results plus query-expansion metadata from the hybrid pipeline.
#[derive(Debug, Clone)]
pub struct HybridPipelineOutput {
    /// Ranked raw search results.
    pub results: Vec<RawSearchResult>,
    /// Expansion variants used for retrieval, excluding the original query.
    pub expanded_queries: Vec<String>,
}

/// Runs the full hybrid search pipeline:
///   probe → optional LLM expansion → per-variant retrieval → fusion → rerank.
///
/// **Short-circuit rules:**
/// - `fast=true` or a decisive BM25 probe (`estimate_strong_signal`) skips
///   both LLM expansion and reranking.
/// - An exact-alias hit during the title probe also skips LLM expansion
///   (the alias is already a confident match).
/// - `options.queries` non-empty bypasses the LLM and uses the supplied
///   variants directly.
///
/// **Graceful degradation:** embedding failures produce empty vector buckets;
/// expansion failures fall back to the original query; rerank failures return
/// hybrid-scored results unchanged.
#[must_use]
pub fn run_hybrid_pipeline(
    conn: &Connection,
    inference: &InferenceClient,
    expansion: Option<&ExpansionClient>,
    query: &str,
    options: &HybridPipelineOptions,
) -> Vec<RawSearchResult> {
    run_hybrid_pipeline_with_metadata(conn, inference, expansion, query, options).results
}

/// Runs the full hybrid search pipeline and returns expansion metadata.
#[must_use]
pub fn run_hybrid_pipeline_with_metadata(
    conn: &Connection,
    inference: &InferenceClient,
    expansion: Option<&ExpansionClient>,
    query: &str,
    options: &HybridPipelineOptions,
) -> HybridPipelineOutput {
    // Lexical-only probe to detect high-confidence matches before paying for
    // the embedding + expansion + rerank round-trips.
    let bm25_probe = search_bm25(
        conn,
        query,
        HYBRID_PROBE_LEXICAL_LIMIT,
        DEFAULT_SNIPPET_LENGTH,
    );
    let title_probe = search_title_parts(conn, query, HYBRID_PROBE_TITLE_LIMIT);

    let has_supplied = !options.queries.is_empty();
    let has_exact_alias = !title_probe.exact_alias.is_empty();
    // Algorithm ported verbatim from qmd — store.ts:4025-4034
    let probe_decisive = options.intent.is_none() && estimate_strong_signal(&bm25_probe);

    if probe_decisive && let Some(top) = bm25_probe.first() {
        options.hooks.emit_strong_signal(top.score);
    }

    // A decisive probe or fast mode skips both expansion and reranking.
    let skip_expensive = options.fast || probe_decisive;
    // An exact alias hit additionally skips LLM expansion (not reranking).
    let skip_llm = skip_expensive || has_exact_alias;

    // Resolve variants: supplied → deduped supplied; bypass → []; else → LLM.
    let variants: Vec<String> = if has_supplied {
        dedupe_query_variants(&options.queries)
    } else if skip_llm {
        vec![]
    } else if let Some(exp) = expansion {
        options.hooks.emit_expand_start();
        let started = Instant::now();
        let expanded = exp
            .expand_with_intent(query, options.intent.as_deref(), EXPANSION_N_VARIANTS)
            .unwrap_or_default();
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        options.hooks.emit_expand_end(elapsed_ms);
        expanded
    } else {
        vec![]
    };

    // Build the final query list.
    let queries_to_search: Vec<String> = if has_supplied {
        if variants.is_empty() {
            vec![query.to_owned()]
        } else {
            variants.clone()
        }
    } else if variants.is_empty() {
        vec![query.to_owned()]
    } else {
        let mut v = vec![query.to_owned()];
        v.extend(variants.iter().cloned());
        v
    };

    let rrf_size = pool::rrf_pool(options.limit, options.candidate_limit);

    // Per-variant: embed → retrieve (BM25 + fuzzy + vector) → intra-variant RRF.
    let per_variant: Vec<Vec<RawSearchResult>> = queries_to_search
        .iter()
        .map(|q| {
            options.hooks.emit_embed_batch(1);
            let embedding = inference
                .embed(std::slice::from_ref(q))
                .ok()
                .and_then(|mut vecs| vecs.pop());
            let single = run_hybrid_single(
                conn,
                q,
                embedding.as_deref(),
                options.limit,
                options.candidate_limit,
            );
            single_to_raw_list(&single, rrf_size as usize)
        })
        .collect();

    // Cross-variant RRF fusion.
    // Original query (index 0) gets 2× weight; expansion variants get 1.0.
    // Algorithm ported verbatim from qmd — store.ts:4122
    let list_refs: Vec<&[RawSearchResult]> = per_variant.iter().map(Vec::as_slice).collect();
    let variant_weights: Vec<f64> = (0..list_refs.len())
        .map(|i| if i == 0 { 2.0 } else { 1.0 })
        .collect();
    let fused = fuse_hybrid_result_lists(&list_refs, &variant_weights, rrf_size as usize);

    // Rerank unless the probe gave us high confidence or fast mode is active.
    let results = if skip_expensive {
        fused
    } else {
        rerank_candidates_with_intent(IntentRerankOptions {
            conn,
            inference,
            query,
            intent: options.intent.as_deref(),
            candidates: fused,
            top_k: RERANK_TOP_K,
            hooks: &options.hooks,
            db_version: read_db_version(conn),
        })
    };

    HybridPipelineOutput {
        results,
        expanded_queries: variants,
    }
}

/// Runs per-signal weighted RRF on the three [`HybridSingleResult`] buckets
/// and converts the output to [`RawSearchResult`] for cross-variant fusion.
fn single_to_raw_list(single: &HybridSingleResult, limit: usize) -> Vec<RawSearchResult> {
    let mut acc = RrfScoreAccumulator::new();
    acc.accumulate(&single.vector, RrfList::Semantic);
    acc.accumulate(&single.bm25, RrfList::Bm25);
    acc.accumulate(&single.fuzzy_title_parts.exact_alias, RrfList::ExactAlias);
    acc.accumulate(&single.fuzzy_title_parts.fuzzy, RrfList::Fuzzy);

    let inputs = RrfInputs {
        semantic: &single.vector,
        bm25: &single.bm25,
        exact_alias: &single.fuzzy_title_parts.exact_alias,
        fuzzy: &single.fuzzy_title_parts.fuzzy,
    };

    normalize_and_merge_rrf_results(&acc, &inputs, limit)
        .iter()
        .map(hybrid_data_to_raw)
        .collect()
}

/// Converts [`HybridScoreData`] (post-RRF) to [`RawSearchResult`].
fn hybrid_data_to_raw(h: &HybridScoreData) -> RawSearchResult {
    RawSearchResult {
        path: h.path.clone(),
        title: h.title.clone(),
        tags: h.tags.clone(),
        aliases: h.aliases.clone(),
        snippet: h.snippet.clone(),
        score: h.hybrid_before_norm.unwrap_or(0.0),
        scores: SearchScores {
            bm25: h.bm25,
            fuzzy_title: h.fuzzy_title,
            hybrid: h.hybrid_before_norm,
            semantic: h.semantic,
            rerank: None,
        },
        semantic_heading: h.semantic_heading.clone(),
        semantic_char_start: h.semantic_char_start,
        semantic_char_end: h.semantic_char_end,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod hooks_tests;
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod intent_tests;
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod test_support;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
