//! Result fusion, strong-signal detection, and rerank blending.
//!
//! Ported from `services/talon/search/fuse.ts`. Two distinct fusion paths:
//!
//! - [`fuse_hybrid_result_lists`] folds RRF results across multiple pipelines
//!   with optional per-list weighting (used by `hybrid_pipeline.rs` to give the
//!   original query 2× weight relative to expansion variants).
//! - [`blend_rerank_candidates`] mixes the post-fusion hybrid score with a
//!   cross-encoder rerank score using a rank-tier-dependent weight.

use std::collections::BTreeMap;

use super::constants::{
    RERANK_MID_RANK_THRESHOLD, RERANK_TOP_RANK_THRESHOLD, RERANK_WEIGHT_LOW, RERANK_WEIGHT_MID,
    RERANK_WEIGHT_TOP, RRF_K, RRF_TOP_RANK_BONUS, STRONG_SIGNAL_MIN_GAP, STRONG_SIGNAL_MIN_SCORE,
};
use super::types::RawSearchResult;

fn compare_raw_results_by_score_then_path(
    a: &RawSearchResult,
    b: &RawSearchResult,
) -> std::cmp::Ordering {
    b.score
        .partial_cmp(&a.score)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| a.path.cmp(&b.path))
}

/// Clamps a value to the closed interval `[0, 1]`.
#[must_use]
pub const fn clamp01(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

/// Standard logistic. Used to map raw rerank scores (which can be unbounded
/// log-odds) into `[0, 1]` for blending.
#[must_use]
pub fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

/// Selects the rerank-blend weight for a candidate at `rank` (0-indexed).
#[must_use]
pub const fn rerank_weight_for_rank(rank: usize) -> f64 {
    if rank < RERANK_TOP_RANK_THRESHOLD {
        RERANK_WEIGHT_TOP
    } else if rank < RERANK_MID_RANK_THRESHOLD {
        RERANK_WEIGHT_MID
    } else {
        RERANK_WEIGHT_LOW
    }
}

/// Returns `true` if the top result is sufficiently above the runner-up to
/// be considered a confident match.
///
/// Definition: top score ≥ [`STRONG_SIGNAL_MIN_SCORE`] AND
/// `top - second ≥ ` [`STRONG_SIGNAL_MIN_GAP`].
///
/// Algorithm ported verbatim from qmd — store.ts:309-315.
#[must_use]
pub fn estimate_strong_signal(results: &[RawSearchResult]) -> bool {
    let Some(top) = results.first() else {
        return false;
    };
    let second = results.get(1).map_or(0.0, |r| r.score);
    top.score >= STRONG_SIGNAL_MIN_SCORE && top.score - second >= STRONG_SIGNAL_MIN_GAP
}

struct FuseAcc {
    base: RawSearchResult,
    score: f64,
    /// Preserved semantic chunk metadata from whichever strategy provided it.
    /// When BM25 wins the base (higher raw score), the semantic heading/offsets
    /// would otherwise be discarded — we stash them here so anchor building
    /// can still produce a Semantic anchor alongside the BM25 one.
    semantic_heading: Option<String>,
    semantic_char_start: Option<u32>,
    semantic_char_end: Option<u32>,
}

/// Fuses multiple ranked result lists with optionally weighted RRF, normalizes
/// by the theoretical maximum, applies a top-rank bonus, and returns the top
/// `limit` results.
///
/// `weights` must be the same length as `lists`.  Each list's RRF contribution
/// is multiplied by its corresponding weight.  Pass all-1.0s for uniform
/// weighting.  The original-query list should receive weight 2.0 and expansion
/// variants weight 1.0.
///
/// Algorithm ported verbatim from qmd — store.ts:4122
///
/// When called with one or zero non-empty lists, returns the first non-empty
/// list as-is (no fusion needed).
///
/// # Panics
/// Panics if `weights.len() != lists.len()`.
#[must_use]
pub fn fuse_hybrid_result_lists(
    lists: &[&[RawSearchResult]],
    weights: &[f64],
    limit: usize,
) -> Vec<RawSearchResult> {
    assert_eq!(
        lists.len(),
        weights.len(),
        "lists and weights must have the same length"
    );

    // Pair each list with its weight, filtering empty ones.
    let active: Vec<(&[RawSearchResult], f64)> = lists
        .iter()
        .copied()
        .zip(weights.iter().copied())
        .filter(|(l, _)| !l.is_empty())
        .collect();

    if active.len() <= 1 {
        return active.first().map_or(Vec::new(), |(l, _)| l.to_vec());
    }

    let mut acc: BTreeMap<String, FuseAcc> = BTreeMap::new();
    let mut max_possible = 0.0_f64;
    for (list, w) in &active {
        // The maximum contribution for this list is w / (RRF_K + 1).
        max_possible += w / (RRF_K + 1.0);
        let mut rank_f = 0.0_f64;
        for result in *list {
            let contribution = w / (RRF_K + rank_f + 1.0);
            acc.entry(result.path.clone())
                .and_modify(|entry| {
                    if result.score > entry.base.score {
                        entry.base = result.clone();
                    }
                    entry.score += contribution;
                    // Merge semantic chunk metadata: take the first non-None
                    // value so it survives even when BM25 wins the base slot.
                    if entry.semantic_heading.is_none() {
                        entry.semantic_heading.clone_from(&result.semantic_heading);
                        entry.semantic_char_start = result.semantic_char_start;
                        entry.semantic_char_end = result.semantic_char_end;
                    }
                })
                .or_insert_with(|| FuseAcc {
                    semantic_heading: result.semantic_heading.clone(),
                    semantic_char_start: result.semantic_char_start,
                    semantic_char_end: result.semantic_char_end,
                    base: result.clone(),
                    score: contribution,
                });
            rank_f += 1.0;
        }
    }

    let mut out: Vec<RawSearchResult> = acc
        .into_values()
        .map(
            |FuseAcc {
                 base,
                 score,
                 semantic_heading,
                 semantic_char_start,
                 semantic_char_end,
             }| {
                let norm = if max_possible == 0.0 {
                    0.0
                } else {
                    clamp01(score / max_possible)
                };
                let mut scores = base.scores.clone();
                scores.hybrid = Some(norm);
                RawSearchResult {
                    path: base.path,
                    title: base.title,
                    tags: base.tags,
                    aliases: base.aliases,
                    snippet: base.snippet,
                    score: norm,
                    scores,
                    semantic_heading,
                    semantic_char_start,
                    semantic_char_end,
                }
            },
        )
        .collect();
    out.sort_by(compare_raw_results_by_score_then_path);
    // Top-rank bonus applied after normalization; rank-0 can reach up to
    // 1.05 (no clamp — callers sort by score only).
    // Algorithm ported verbatim from qmd — store.ts:3377-3384
    for (rank, result) in out.iter_mut().enumerate() {
        if let Some(bonus) = RRF_TOP_RANK_BONUS.get(rank) {
            result.score += bonus;
            if let Some(h) = result.scores.hybrid.as_mut() {
                *h += bonus;
            }
        } else {
            break;
        }
    }
    out.truncate(limit);
    out
}

/// Blends each candidate's hybrid score with its rerank logit.
///
/// Uses a rank-tier-dependent weight (top: 0.75 hybrid / 0.25 rerank;
/// mid: 0.6/0.4; low: 0.4/0.6). Hybrid scores are min-max normalized
/// within the candidate batch; rerank logits are mapped to `[0, 1]` via
/// [`sigmoid`] before blending. See OHS `searcher.ts:1299-1325`.
///
/// `pre_rerank_rank` is the 0-indexed position of each candidate in the
/// input slice (i.e., before this function reorders by `final_score`).
#[must_use]
pub fn blend_rerank_candidates(
    candidates: &[RawSearchResult],
    rerank_scores: &[Option<f64>],
) -> Vec<RawSearchResult> {
    let rerank_probabilities: Vec<Option<f64>> = rerank_scores
        .iter()
        .map(|score| score.map(sigmoid))
        .collect();
    blend_rerank_probabilities(candidates, &rerank_probabilities)
}

/// Blends candidates with already sigmoid-normalized rerank scores.
#[must_use]
pub fn blend_rerank_probabilities(
    candidates: &[RawSearchResult],
    rerank_probabilities: &[Option<f64>],
) -> Vec<RawSearchResult> {
    let hybrid_values: Vec<f64> = candidates
        .iter()
        .map(|c| c.scores.hybrid.unwrap_or(c.score))
        .collect();
    let min_h = hybrid_values.iter().copied().fold(f64::INFINITY, f64::min);
    let max_h = hybrid_values
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    // Guard against single-candidate (or all-equal) edge case: use 1.0 so
    // normHybrid = (score - min) / 1.0 = 0.0 when all scores are equal.
    // Mirrors OHS `rangeH = maxH - minH || 1`. See searcher.ts:1315.
    let range_h = if max_h > min_h { max_h - min_h } else { 1.0 };

    let mut out: Vec<RawSearchResult> = candidates
        .iter()
        .enumerate()
        .map(|(rank, candidate)| {
            let Some(rerank01) = rerank_probabilities.get(rank).copied().flatten() else {
                return candidate.clone();
            };
            let base_hybrid = candidate.scores.hybrid.unwrap_or(candidate.score);
            let hybrid01 = clamp01((base_hybrid - min_h) / range_h);
            let w = rerank_weight_for_rank(rank);
            // `w * hybrid01 + (1-w) * rerank01`, written as an FMA.
            let final_score = clamp01(f64::mul_add(w, hybrid01 - rerank01, rerank01));

            let mut scores = candidate.scores.clone();
            scores.hybrid = Some(scores.hybrid.unwrap_or(candidate.score));
            // Sigmoid'd value, not raw logit — see US-005 / OHS searcher.ts:1319.
            scores.rerank = Some(rerank01);
            RawSearchResult {
                path: candidate.path.clone(),
                title: candidate.title.clone(),
                tags: candidate.tags.clone(),
                aliases: candidate.aliases.clone(),
                snippet: candidate.snippet.clone(),
                score: final_score,
                scores,
                semantic_heading: candidate.semantic_heading.clone(),
                semantic_char_start: candidate.semantic_char_start,
                semantic_char_end: candidate.semantic_char_end,
            }
        })
        .collect();
    out.sort_by(compare_raw_results_by_score_then_path);
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]
mod tests;
