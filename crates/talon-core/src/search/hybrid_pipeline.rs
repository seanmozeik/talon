//! Full hybrid search pipeline orchestrator.
//!
//! Wires lexical probe, LLM expansion, hybrid retrieval, RRF, and rerank.
//! Ports `services/talon/search/hybrid-pipeline.ts`.

use rusqlite::Connection;
use std::time::Instant;

use crate::expansion::client::ExpansionClient;
use crate::indexing::migrations::read_db_version;
use crate::inference::{EmbeddingClient, RerankClient};

use super::bm25::search_bm25;
use super::cache::dedupe_query_variants;
use super::constants::{
    DEFAULT_SNIPPET_LENGTH, HYBRID_PROBE_LEXICAL_LIMIT, HYBRID_PROBE_TITLE_LIMIT, RERANK_TOP_K,
};
use super::fuse::estimate_strong_signal;
use super::fuse::fuse_hybrid_result_lists;
use super::fuzzy_title::search_title_parts;
use super::hooks::SearchHooks;
use super::hybrid_single::run_hybrid_single;
use super::pool;
use super::pre_filter::PreFilter;
use super::rerank_pipeline::{IntentRerankOptions, rerank_candidates_with_intent};
use super::types::RawSearchResult;

mod convert;
use convert::{lexical_probe_results, single_to_raw_list};

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
    /// Skip LLM expansion but keep vector retrieval and rerank.
    pub skip_expansion: bool,
    /// Pre-supplied query variants (bypass LLM call when non-empty).
    pub queries: Vec<String>,
    /// Optional disambiguating context for expansion, rerank, and chunks.
    pub intent: Option<String>,
    /// Optional stage instrumentation callbacks.
    pub hooks: SearchHooks,
    /// Pre-computed filters pushed into every retrieval SQL query.
    pub pre_filter: PreFilter,
    /// Wall-clock deadline for cooperative hook fallback.
    pub deadline_at: Option<Instant>,
}

/// Results plus query-expansion metadata from the hybrid pipeline.
#[derive(Debug, Clone, Default)]
pub struct HybridPipelineOutput {
    /// Ranked raw search results.
    pub results: Vec<RawSearchResult>,
    /// Expansion variants used for retrieval, excluding the original query.
    pub expanded_queries: Vec<String>,
    /// Wall-clock time spent on LLM expansion, when expansion ran.
    pub expansion_ms: Option<u64>,
    /// Top BM25 probe score that triggered the strong-signal bypass.
    pub strong_signal_score: Option<f64>,
    /// Candidates submitted to the reranker, when rerank ran.
    pub rerank_candidates: Option<u32>,
    /// Wall-clock time spent reranking, when rerank ran.
    pub rerank_ms: Option<u64>,
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
    embedding: &EmbeddingClient,
    rerank: &RerankClient,
    expansion: Option<&ExpansionClient>,
    query: &str,
    options: &HybridPipelineOptions,
) -> Vec<RawSearchResult> {
    run_hybrid_pipeline_with_metadata(conn, embedding, rerank, expansion, query, options).results
}

/// Runs the full hybrid search pipeline and returns expansion metadata.
#[must_use]
pub fn run_hybrid_pipeline_with_metadata(
    conn: &Connection,
    embedding: &EmbeddingClient,
    rerank: &RerankClient,
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
        &options.pre_filter,
    );
    let title_probe =
        search_title_parts(conn, query, HYBRID_PROBE_TITLE_LIMIT, &options.pre_filter);

    let has_supplied = !options.queries.is_empty();
    let has_exact_alias = !title_probe.exact_alias.is_empty();
    // Algorithm ported verbatim from qmd — store.ts:4025-4034
    let probe_decisive = options.intent.is_none() && estimate_strong_signal(&bm25_probe);

    let strong_signal_score = if probe_decisive {
        bm25_probe.first().map(|top| {
            options.hooks.emit_strong_signal(top.score);
            top.score
        })
    } else {
        None
    };

    if deadline_exceeded(options.deadline_at) {
        return HybridPipelineOutput {
            results: lexical_probe_results(&bm25_probe, &title_probe, options.limit),
            expanded_queries: Vec::new(),
            expansion_ms: None,
            strong_signal_score,
            rerank_candidates: None,
            rerank_ms: None,
        };
    }

    // A decisive probe or fast mode skips both expansion and reranking.
    let skip_expensive = options.fast || probe_decisive;
    // An exact alias hit additionally skips LLM expansion (not reranking).
    let skip_llm = skip_expensive || options.skip_expansion || has_exact_alias;

    let (variants, expansion_ms) =
        resolve_query_variants(expansion, query, options, has_supplied, skip_llm);

    let queries_to_search = build_query_list(query, has_supplied, &variants);

    let rrf_size = pool::rrf_pool(options.limit, options.candidate_limit);
    let per_variant =
        retrieve_query_variants(conn, embedding, &queries_to_search, options, rrf_size);

    // Cross-variant RRF fusion.
    // Original query (index 0) gets 2× weight; expansion variants get 1.0.
    // Algorithm ported verbatim from qmd — store.ts:4122
    let list_refs: Vec<&[RawSearchResult]> = per_variant.iter().map(Vec::as_slice).collect();
    let variant_weights: Vec<f64> = (0..list_refs.len())
        .map(|i| if i == 0 { 2.0 } else { 1.0 })
        .collect();
    let fused = fuse_hybrid_result_lists(&list_refs, &variant_weights, rrf_size as usize);

    // Rerank unless the probe gave us high confidence or fast mode is active.
    let (results, rerank_candidates, rerank_ms) =
        if skip_expensive || deadline_exceeded(options.deadline_at) {
            (fused, None, None)
        } else {
            run_rerank_stage(conn, rerank, query, options, fused)
        };

    HybridPipelineOutput {
        results,
        expanded_queries: variants,
        expansion_ms,
        strong_signal_score,
        rerank_candidates,
        rerank_ms,
    }
}

fn resolve_query_variants(
    expansion: Option<&ExpansionClient>,
    query: &str,
    options: &HybridPipelineOptions,
    has_supplied: bool,
    skip_llm: bool,
) -> (Vec<String>, Option<u64>) {
    if has_supplied {
        return (dedupe_query_variants(&options.queries), None);
    }
    if skip_llm || deadline_exceeded(options.deadline_at) {
        return (Vec::new(), None);
    }
    let Some(exp) = expansion else {
        return (Vec::new(), None);
    };
    options.hooks.emit_expand_start();
    let started = Instant::now();
    let expanded = exp
        .expand_with_intent(query, options.intent.as_deref(), EXPANSION_N_VARIANTS)
        .unwrap_or_default();
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    options.hooks.emit_expand_end(elapsed_ms);
    (expanded, Some(elapsed_ms))
}

fn build_query_list(query: &str, has_supplied: bool, variants: &[String]) -> Vec<String> {
    if has_supplied {
        if variants.is_empty() {
            vec![query.to_owned()]
        } else {
            variants.to_vec()
        }
    } else if variants.is_empty() {
        vec![query.to_owned()]
    } else {
        let mut queries = Vec::with_capacity(variants.len() + 1);
        queries.push(query.to_owned());
        queries.extend(variants.iter().cloned());
        queries
    }
}

fn retrieve_query_variants(
    conn: &Connection,
    embedding: &EmbeddingClient,
    queries_to_search: &[String],
    options: &HybridPipelineOptions,
    rrf_size: u32,
) -> Vec<Vec<RawSearchResult>> {
    queries_to_search
        .iter()
        .take_while(|_| !deadline_exceeded(options.deadline_at))
        .map(|q| {
            options.hooks.emit_embed_batch(1);
            let embedding_vec = embedding
                .embed(std::slice::from_ref(q))
                .ok()
                .and_then(|mut vecs| vecs.pop());
            let single = run_hybrid_single(
                conn,
                q,
                embedding_vec.as_deref(),
                options.limit,
                options.candidate_limit,
                &options.pre_filter,
            );
            single_to_raw_list(&single, rrf_size as usize)
        })
        .collect()
}

fn run_rerank_stage(
    conn: &Connection,
    rerank: &RerankClient,
    query: &str,
    options: &HybridPipelineOptions,
    fused: Vec<RawSearchResult>,
) -> (Vec<RawSearchResult>, Option<u32>, Option<u64>) {
    if deadline_exceeded(options.deadline_at) {
        return (fused, None, None);
    }
    let candidate_count =
        u32::try_from(fused.len().min(RERANK_TOP_K as usize)).unwrap_or(RERANK_TOP_K);
    let started = Instant::now();
    let reranked = rerank_candidates_with_intent(IntentRerankOptions {
        conn,
        rerank,
        query,
        intent: options.intent.as_deref(),
        candidates: fused,
        top_k: RERANK_TOP_K,
        hooks: &options.hooks,
        db_version: read_db_version(conn),
    });
    let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    (reranked, Some(candidate_count), Some(elapsed))
}

fn deadline_exceeded(deadline_at: Option<Instant>) -> bool {
    deadline_at.is_some_and(|deadline| Instant::now() >= deadline)
}

#[cfg(test)]
mod hooks_tests;
#[cfg(test)]
mod intent_tests;
#[cfg(test)]
mod skip_expansion_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
