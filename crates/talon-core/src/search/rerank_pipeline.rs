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
use crate::text::nfd;
use rusqlite::Connection;

use super::fuse::{blend_rerank_probabilities, sigmoid};
use super::hooks::SearchHooks;
use super::intent;
use super::types::RawSearchResult;

pub(crate) struct IntentRerankOptions<'a> {
    pub(crate) conn: &'a Connection,
    pub(crate) inference: &'a InferenceClient,
    pub(crate) query: &'a str,
    pub(crate) intent: Option<&'a str>,
    pub(crate) candidates: Vec<RawSearchResult>,
    pub(crate) top_k: u32,
    pub(crate) hooks: &'a SearchHooks,
    pub(crate) db_version: u64,
}

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
    let options = RerankOptions {
        conn: None,
        inference,
        query,
        intent: None,
        candidates,
        top_k,
        hooks,
        db_version,
    };
    rerank_candidates_inner(options)
}

/// Calls the inference sidecar with an intent-aware query and chunk choice.
#[must_use]
pub(crate) fn rerank_candidates_with_intent(
    options: IntentRerankOptions<'_>,
) -> Vec<RawSearchResult> {
    rerank_candidates_inner(RerankOptions {
        conn: Some(options.conn),
        inference: options.inference,
        query: options.query,
        intent: options.intent,
        candidates: options.candidates,
        top_k: options.top_k,
        hooks: options.hooks,
        db_version: options.db_version,
    })
}

struct RerankOptions<'a> {
    conn: Option<&'a Connection>,
    inference: &'a InferenceClient,
    query: &'a str,
    intent: Option<&'a str>,
    candidates: Vec<RawSearchResult>,
    top_k: u32,
    hooks: &'a SearchHooks,
    db_version: u64,
}

fn rerank_candidates_inner(options: RerankOptions<'_>) -> Vec<RawSearchResult> {
    let RerankOptions {
        conn,
        inference,
        query,
        intent,
        candidates,
        top_k,
        hooks,
        db_version,
    } = options;

    if candidates.is_empty() {
        return candidates;
    }

    let limit = (top_k as usize).min(candidates.len());
    let mut active: Vec<RawSearchResult> = candidates.into_iter().take(limit).collect();
    if let Some(conn) = conn {
        select_best_chunks_for_rerank(conn, query, intent, &mut active);
    }
    let rerank_query = intent::prefix_query(intent, query);

    hooks.emit_rerank_start(active.len());
    let started = Instant::now();
    let texts: Vec<String> = active.iter().map(rerank_text).collect();
    let mut scores: Vec<Option<f64>> = vec![None; limit];
    let mut missing_indices = Vec::new();
    let mut missing_texts = Vec::new();

    for (index, text) in texts.iter().enumerate() {
        if let Some(score) = rerank_cache::lookup(text, &rerank_query, db_version) {
            scores[index] = Some(score);
        } else {
            missing_indices.push(index);
            missing_texts.push(text.clone());
        }
    }

    if !missing_texts.is_empty() {
        let Ok(rerank_results) = inference.rerank(&rerank_query, &missing_texts, false) else {
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
                rerank_cache::store(text, &rerank_query, score, db_version);
            }
        }
    }

    let blended = blend_rerank_probabilities(&active, &scores);
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    hooks.emit_rerank_end(elapsed_ms);
    blended
}

fn select_best_chunks_for_rerank(
    conn: &Connection,
    query: &str,
    intent: Option<&str>,
    candidates: &mut [RawSearchResult],
) {
    let query_terms = intent::extract_terms(query);
    let intent_terms = intent.map(intent::extract_terms).unwrap_or_default();
    if query_terms.is_empty() && intent_terms.is_empty() {
        return;
    }

    for candidate in candidates {
        let Some(chunk) =
            best_chunk_for_candidate(conn, &candidate.path, &query_terms, &intent_terms)
        else {
            continue;
        };
        candidate.snippet = chunk.text;
        candidate.semantic_heading = chunk.heading_path;
        candidate.semantic_char_start = chunk.char_start;
        candidate.semantic_char_end = chunk.char_end;
    }
}

struct RerankChunk {
    text: String,
    heading_path: Option<String>,
    char_start: Option<u32>,
    char_end: Option<u32>,
}

fn best_chunk_for_candidate(
    conn: &Connection,
    path: &str,
    query_terms: &[String],
    intent_terms: &[String],
) -> Option<RerankChunk> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT c.text, c.heading_path, c.char_start, c.char_end, c.chunk_index
             FROM chunks c
             JOIN notes n ON n.id = c.note_id
             WHERE n.vault_path = ?1
             ORDER BY c.chunk_index",
        )
        .ok()?;
    let rows = stmt
        .query_map([path], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .ok()?;

    let mut best: Option<(u32, i64, RerankChunk)> = None;
    for row in rows {
        let Ok((text, heading_path, char_start, char_end, chunk_index)) = row else {
            continue;
        };
        // Algorithm ported verbatim from qmd — store.ts:4140-4151
        let score = chunk_term_score_units(&text, query_terms, intent_terms);
        let chunk = RerankChunk {
            text,
            heading_path,
            char_start: char_start.and_then(|value| u32::try_from(value).ok()),
            char_end: char_end.and_then(|value| u32::try_from(value).ok()),
        };
        match &best {
            Some((best_score, best_index, _))
                if score < *best_score || (score == *best_score && chunk_index >= *best_index) => {}
            _ => best = Some((score, chunk_index, chunk)),
        }
    }
    best.map(|(_, _, chunk)| chunk)
}

fn chunk_term_score_units(text: &str, query_terms: &[String], intent_terms: &[String]) -> u32 {
    let chunk = nfd::normalize(text).to_lowercase();
    let query_hits = query_terms
        .iter()
        .filter(|term| chunk.contains(term.as_str()))
        .fold(0_u32, |count, _| count.saturating_add(1));
    let intent_hits = intent_terms
        .iter()
        .filter(|term| chunk.contains(term.as_str()))
        .fold(0_u32, |count, _| count.saturating_add(1));
    query_hits.saturating_mul(2).saturating_add(intent_hits)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "rerank_pipeline_intent_tests.rs"]
mod intent_tests;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "rerank_pipeline_tests.rs"]
mod tests;
