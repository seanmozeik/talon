//! Result fusion, strong-signal detection, and rerank blending.
//!
//! Ported from `services/talon/search/fuse.ts`. Two distinct fusion paths:
//!
//! - [`fuse_hybrid_result_lists`] is a per-pipeline RRF fold without per-list
//!   weights, used to merge multi-query expansions into a single ranked list.
//! - [`blend_rerank_candidates`] mixes the post-fusion hybrid score with a
//!   cross-encoder rerank score using a rank-tier-dependent weight.

use std::collections::BTreeMap;

use super::constants::{
    RERANK_MID_RANK_THRESHOLD, RERANK_TOP_RANK_THRESHOLD, RERANK_WEIGHT_LOW, RERANK_WEIGHT_MID,
    RERANK_WEIGHT_TOP, RRF_K, STRONG_SIGNAL_MIN_GAP, STRONG_SIGNAL_MIN_SCORE,
};
use super::types::RawSearchResult;

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

/// Fuses multiple ranked result lists with unweighted RRF, normalizes by the
/// theoretical maximum, and returns the top `limit` results.
///
/// When called with one or zero non-empty lists, returns the first non-empty
/// list as-is (no fusion needed).
#[must_use]
pub fn fuse_hybrid_result_lists(
    lists: &[&[RawSearchResult]],
    limit: usize,
) -> Vec<RawSearchResult> {
    let active: Vec<&[RawSearchResult]> = lists.iter().copied().filter(|l| !l.is_empty()).collect();
    if active.len() <= 1 {
        return active.first().map_or(Vec::new(), |l| l.to_vec());
    }

    let mut acc: BTreeMap<String, FuseAcc> = BTreeMap::new();
    let mut active_count = 0.0_f64;
    for list in &active {
        active_count += 1.0;
        let mut rank_f = 0.0_f64;
        for result in *list {
            let contribution = 1.0 / (RRF_K + rank_f + 1.0);
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

    let max_possible = active_count * (1.0 / (RRF_K + 1.0));
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
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(limit);
    out
}

fn normalize_within_candidate_batch(score: f64, max_score: f64) -> f64 {
    if max_score <= 0.0 {
        0.0
    } else {
        clamp01(score / max_score)
    }
}

/// Blends each candidate's hybrid score with its rerank score.
///
/// Uses a rank-tier-dependent weight (top: 0.75 hybrid / 0.25 rerank;
/// mid: 0.6/0.4; low: 0.4/0.6). Rerank scores already in `[0, 1]` are used
/// as-is; values outside that range are passed through [`sigmoid`].
#[must_use]
pub fn blend_rerank_candidates(
    candidates: &[RawSearchResult],
    rerank_scores: &[Option<f64>],
) -> Vec<RawSearchResult> {
    let max_hybrid = candidates
        .iter()
        .map(|c| c.scores.hybrid.unwrap_or(c.score))
        .fold(0.0_f64, f64::max);

    let mut out: Vec<RawSearchResult> = candidates
        .iter()
        .enumerate()
        .map(|(rank, candidate)| {
            let Some(rerank) = rerank_scores.get(rank).copied().flatten() else {
                return candidate.clone();
            };
            let base_hybrid = candidate.scores.hybrid.unwrap_or(candidate.score);
            let hybrid01 = normalize_within_candidate_batch(base_hybrid, max_hybrid);
            let rerank01 = if (0.0..=1.0).contains(&rerank) {
                rerank
            } else {
                sigmoid(rerank)
            };
            let w = rerank_weight_for_rank(rank);
            // Algebraic rewrite of `w*h + (1-w)*r` into a single fused-multiply-add.
            let final_score = clamp01(f64::mul_add(w, hybrid01 - rerank01, rerank01));

            let mut scores = candidate.scores.clone();
            scores.hybrid = Some(scores.hybrid.unwrap_or(candidate.score));
            scores.rerank = Some(rerank);
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
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]
mod tests;
