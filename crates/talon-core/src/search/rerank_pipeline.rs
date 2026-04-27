//! Cross-encoder rerank pipeline.
//!
//! Thin orchestration layer that calls the inference sidecar's `/rerank`
//! endpoint and blends the cross-encoder scores into the hybrid scores using
//! [`super::fuse::blend_rerank_probabilities`].
//!
//! Ports `services/talon/search/rerank-pipeline.ts`. The TS reference uses
//! Effect and an LLM cache layer; this Rust port calls the sidecar directly
//! and delegates blending to the existing `fuse` module. Caching can be added
//! on top by the pipeline orchestrator (US-005).

use std::time::Instant;

use crate::cache::rerank as rerank_cache;
use crate::inference::InferenceClient;

use super::fuse::{blend_rerank_probabilities, sigmoid};
use super::hooks::SearchHooks;
use super::types::RawSearchResult;

/// Returns `(w_hybrid, w_rerank)` blend weights for a candidate at the given
/// pre-rerank rank index (0-indexed).
///
/// Top results trust hybrid more; deeper results trust rerank more.
/// - `0..=9`  → `(0.75, 0.25)`
/// - `10..=19` → `(0.60, 0.40)`
/// - `20..`   → `(0.40, 0.60)`
///
/// Mirrors OHS `searcher.ts:1320`.
#[cfg(test)]
const fn position_weights(rank_index: usize) -> (f64, f64) {
    if rank_index < 10 {
        (0.75, 0.25)
    } else if rank_index < 20 {
        (0.60, 0.40)
    } else {
        (0.40, 0.60)
    }
}

/// Builds the reranker text payload for a single candidate.
///
/// Matches the TS `rerankText` function: `"${title}\n\n${snippet}"`.
fn rerank_text(candidate: &RawSearchResult) -> String {
    format!("{}\n\n{}", candidate.title, candidate.snippet)
}

/// Calls the inference sidecar to rerank `candidates` and blends the
/// cross-encoder scores into the hybrid scores.
///
/// Only the first `top_k` candidates are sent to the reranker and returned;
/// pass `RERANK_TOP_K` from [`super::constants`] as the default.
///
/// On inference error the function degrades gracefully: the original (up to
/// `top_k`) candidates are returned with their hybrid scores unchanged and
/// no `scores.rerank` field set.
#[must_use]
pub fn rerank_candidates(
    inference: &InferenceClient,
    query: &str,
    candidates: Vec<RawSearchResult>,
    top_k: u32,
    hooks: &SearchHooks,
) -> Vec<RawSearchResult> {
    rerank_candidates_with_db_version(inference, query, candidates, top_k, hooks, 0)
}

/// Calls the inference sidecar with a `db_version`-scoped per-snippet cache.
#[must_use]
pub(crate) fn rerank_candidates_with_db_version(
    inference: &InferenceClient,
    query: &str,
    candidates: Vec<RawSearchResult>,
    top_k: u32,
    hooks: &SearchHooks,
    db_version: u64,
) -> Vec<RawSearchResult> {
    if candidates.is_empty() {
        return candidates;
    }

    let limit = (top_k as usize).min(candidates.len());
    let active: Vec<RawSearchResult> = candidates.into_iter().take(limit).collect();

    hooks.emit_rerank_start(active.len());
    let started = Instant::now();
    let texts: Vec<String> = active.iter().map(rerank_text).collect();
    let mut scores: Vec<Option<f64>> = vec![None; limit];
    let mut missing_indices = Vec::new();
    let mut missing_texts = Vec::new();

    for (index, text) in texts.iter().enumerate() {
        if let Some(score) = rerank_cache::lookup(text, query, db_version) {
            scores[index] = Some(score);
        } else {
            missing_indices.push(index);
            missing_texts.push(text.clone());
        }
    }

    if !missing_texts.is_empty() {
        let Ok(rerank_results) = inference.rerank(query, &missing_texts, false) else {
            let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            hooks.emit_rerank_end(elapsed_ms);
            return active;
        };

        for result in rerank_results {
            let Some(original_index) = missing_indices.get(result.index as usize).copied() else {
                continue;
            };
            let score = sigmoid(f64::from(result.score));
            if let Some(slot) = scores.get_mut(original_index) {
                *slot = Some(score);
            }
            if let Some(text) = texts.get(original_index) {
                rerank_cache::store(text, query, score, db_version);
            }
        }
    }

    let blended = blend_rerank_probabilities(&active, &scores);
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    hooks.emit_rerank_end(elapsed_ms);
    blended
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "rerank_pipeline_tests.rs"]
mod tests;
