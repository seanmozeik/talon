//! Reciprocal Rank Fusion (RRF) across signal lists.
//!
//! Ported from `services/talon/search/rrf.ts`. Each retrieval list (BM25,
//! exact-alias, fuzzy, semantic) contributes an RRF score per result:
//!
//! ```text
//! score(result, list) = WEIGHT[list] / (RRF_K + rank + 1)
//! ```
//!
//! Per-path scores are summed across all lists and then normalized against
//! the maximum theoretically possible total (the sum of `WEIGHT[list] /
//! (RRF_K + 1)` across all lists that returned at least one result).

use std::collections::BTreeMap;

use super::constants::{RRF_K, RRF_WEIGHTS};
use super::types::{HybridScoreData, RawSearchResult};

/// Logical RRF list identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RrfList {
    /// BM25 lexical retrieval list.
    Bm25,
    /// Exact-alias retrieval list.
    ExactAlias,
    /// Fuzzy title/alias retrieval list.
    Fuzzy,
    /// Semantic (vector) retrieval list.
    Semantic,
}

impl RrfList {
    const fn weight(self) -> f64 {
        match self {
            Self::Bm25 => RRF_WEIGHTS.bm25,
            Self::ExactAlias => RRF_WEIGHTS.exact_alias,
            Self::Fuzzy => RRF_WEIGHTS.fuzzy,
            Self::Semantic => RRF_WEIGHTS.semantic,
        }
    }
}

/// RRF score accumulator. Stores per-path, per-list contributions plus the
/// maximum possible contribution per list (used for normalization).
#[derive(Debug, Default, Clone)]
pub struct RrfScoreAccumulator {
    /// `path → list → score`. `BTreeMap` is used so iteration order is stable
    /// for ties (matches the deterministic behavior callers expect).
    pub scores: BTreeMap<String, BTreeMap<RrfList, f64>>,
    /// `list → max possible contribution`.
    pub max_score_by_list: BTreeMap<RrfList, f64>,
}

impl RrfScoreAccumulator {
    /// Creates an empty accumulator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Accumulates RRF contributions from `results` for `list`.
    ///
    /// Each result at position `rank` contributes `weight / (RRF_K + rank + 1)`
    /// to its path's per-list score. The max-possible contribution for the
    /// list is set to `weight / (RRF_K + 1)` (i.e. rank-0's contribution),
    /// or `0.0` if `results` is empty.
    pub fn accumulate(&mut self, results: &[RawSearchResult], list: RrfList) {
        let weight = list.weight();
        let mut rank_f = 0.0_f64;
        for result in results {
            let rrf = weight / (RRF_K + rank_f + 1.0);
            self.scores
                .entry(result.path.clone())
                .or_default()
                .insert(list, rrf);
            rank_f += 1.0;
        }
        let max = if results.is_empty() {
            0.0
        } else {
            weight / (RRF_K + 1.0)
        };
        self.max_score_by_list.insert(list, max);
    }
}

fn sum_list_caps(max_by_list: &BTreeMap<RrfList, f64>) -> f64 {
    max_by_list.values().copied().sum()
}

fn first_hit_by_path<'a>(
    lists: &'a [&'a [RawSearchResult]],
) -> BTreeMap<String, &'a RawSearchResult> {
    let mut out = BTreeMap::new();
    for list in lists {
        for r in *list {
            out.entry(r.path.clone()).or_insert(r);
        }
    }
    out
}

/// Bag of per-list result lists, ordered to match the TS reference's
/// "first hit wins" priority for selecting the base row that supplies
/// title/snippet metadata.
#[derive(Debug, Clone, Default)]
pub struct RrfInputs<'a> {
    /// Semantic results.
    pub semantic: &'a [RawSearchResult],
    /// BM25 results.
    pub bm25: &'a [RawSearchResult],
    /// Exact-alias results.
    pub exact_alias: &'a [RawSearchResult],
    /// Fuzzy title results.
    pub fuzzy: &'a [RawSearchResult],
}

/// Normalizes RRF scores and merges them with retrieval-list metadata.
///
/// Per-path scores are summed across all lists, divided by the per-list cap
/// (the score for hitting rank 0 in every contributing list), and clamped.
/// Each result's title/snippet metadata comes from the highest-priority
/// retrieval list it appeared in. The top `limit` results are returned
/// sorted by descending hybrid score.
#[must_use]
pub fn normalize_and_merge_rrf_results(
    acc: &RrfScoreAccumulator,
    inputs: &RrfInputs<'_>,
    limit: usize,
) -> Vec<HybridScoreData> {
    let max_possible = sum_list_caps(&acc.max_score_by_list);
    let priority: [&[RawSearchResult]; 4] = [
        inputs.semantic,
        inputs.bm25,
        inputs.exact_alias,
        inputs.fuzzy,
    ];
    let result_map = first_hit_by_path(&priority);

    let mut hybrid: Vec<HybridScoreData> = acc
        .scores
        .iter()
        .filter_map(|(path, path_scores)| {
            let base = result_map.get(path)?;
            let raw = path_scores.get(&RrfList::Semantic).copied().unwrap_or(0.0)
                + path_scores.get(&RrfList::Bm25).copied().unwrap_or(0.0)
                + path_scores
                    .get(&RrfList::ExactAlias)
                    .copied()
                    .unwrap_or(0.0)
                + path_scores.get(&RrfList::Fuzzy).copied().unwrap_or(0.0);
            let hybrid_before_norm = if max_possible == 0.0 {
                0.0
            } else {
                (raw / max_possible).clamp(0.0, 1.0)
            };
            Some(HybridScoreData {
                path: path.clone(),
                title: base.title.clone(),
                tags: base.tags.clone(),
                aliases: base.aliases.clone(),
                snippet: base.snippet.clone(),
                bm25: path_scores.get(&RrfList::Bm25).copied(),
                fuzzy_title: path_scores.get(&RrfList::Fuzzy).copied(),
                semantic: path_scores.get(&RrfList::Semantic).copied(),
                hybrid_before_norm: Some(hybrid_before_norm),
                semantic_heading: base.semantic_heading.clone(),
                semantic_char_start: base.semantic_char_start,
                semantic_char_end: base.semantic_char_end,
            })
        })
        .collect();

    hybrid.sort_by(|a, b| {
        b.hybrid_before_norm
            .unwrap_or(0.0)
            .partial_cmp(&a.hybrid_before_norm.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hybrid.truncate(limit);
    hybrid
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::search::types::SearchScores;

    fn r(path: &str, score: f64) -> RawSearchResult {
        RawSearchResult {
            path: path.into(),
            title: format!("Title {path}"),
            tags: vec![],
            aliases: vec![],
            snippet: format!("snip {path}"),
            score,
            scores: SearchScores::default(),
            semantic_heading: None,
            semantic_char_start: None,
            semantic_char_end: None,
        }
    }

    #[test]
    fn empty_accumulator_yields_no_results() {
        let acc = RrfScoreAccumulator::new();
        let inputs = RrfInputs::default();
        assert_eq!(normalize_and_merge_rrf_results(&acc, &inputs, 10).len(), 0);
    }

    #[test]
    fn single_list_normalizes_top_hit_to_one() {
        let bm25 = vec![r("a.md", 0.0), r("b.md", 0.0)];
        let mut acc = RrfScoreAccumulator::new();
        acc.accumulate(&bm25, RrfList::Bm25);
        let inputs = RrfInputs {
            bm25: &bm25,
            ..Default::default()
        };
        let results = normalize_and_merge_rrf_results(&acc, &inputs, 10);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].path, "a.md");
        // The rank-0 score equals the per-list cap, so it normalizes to 1.0.
        assert!((results[0].hybrid_before_norm.unwrap() - 1.0).abs() < 1e-9);
        assert!(results[1].hybrid_before_norm.unwrap() < 1.0);
    }

    #[test]
    fn multiple_lists_sum_contributions() {
        let bm25 = vec![r("a.md", 0.0), r("b.md", 0.0)];
        let semantic = vec![r("a.md", 0.0)];
        let mut acc = RrfScoreAccumulator::new();
        acc.accumulate(&bm25, RrfList::Bm25);
        acc.accumulate(&semantic, RrfList::Semantic);

        let inputs = RrfInputs {
            bm25: &bm25,
            semantic: &semantic,
            ..Default::default()
        };
        let results = normalize_and_merge_rrf_results(&acc, &inputs, 10);
        // a.md appears in both lists → wins.
        assert_eq!(results[0].path, "a.md");
        assert!(results[0].hybrid_before_norm.unwrap() > results[1].hybrid_before_norm.unwrap());
        // a.md got hybrid_before_norm = 1.0 (cap), b.md got bm25's rank-1 only.
        assert!((results[0].hybrid_before_norm.unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn weights_favor_bm25_and_alias_over_fuzzy() {
        // Same-rank single-list contributions: weight / (RRF_K + 1).
        // Bm25 (weight 1.5) vs Fuzzy (weight 0.25) → bm25 should win 6×.
        // Algorithm ported verbatim from obsidian-hybrid-search (MIT) — searcher.ts:1390-1392
        let bm25 = vec![r("bm.md", 0.0)];
        let fuzzy = vec![r("fz.md", 0.0)];
        let mut acc = RrfScoreAccumulator::new();
        acc.accumulate(&bm25, RrfList::Bm25);
        acc.accumulate(&fuzzy, RrfList::Fuzzy);

        let inputs = RrfInputs {
            bm25: &bm25,
            fuzzy: &fuzzy,
            ..Default::default()
        };
        let results = normalize_and_merge_rrf_results(&acc, &inputs, 10);
        let bm = results.iter().find(|r| r.path == "bm.md").unwrap();
        let fz = results.iter().find(|r| r.path == "fz.md").unwrap();
        // The cap is the sum of both per-list maxes (1.5/61 + 0.25/61 = 1.75/61).
        // bm.md raw = 1.5/61 → normalized 1.5/1.75 = 6/7.
        // fz.md raw = 0.25/61 → normalized 0.25/1.75 = 1/7.
        assert!(
            (bm.hybrid_before_norm.unwrap() - 6.0 / 7.0).abs() < 1e-9,
            "bm.md expected {}, got {}",
            6.0 / 7.0,
            bm.hybrid_before_norm.unwrap()
        );
        assert!(
            (fz.hybrid_before_norm.unwrap() - 1.0 / 7.0).abs() < 1e-9,
            "fz.md expected {}, got {}",
            1.0 / 7.0,
            fz.hybrid_before_norm.unwrap()
        );
    }

    #[test]
    fn first_hit_wins_for_metadata_priority() {
        // Same path appears in semantic with snippet "from semantic", and in
        // bm25 with snippet "from bm25". Semantic comes first in priority.
        let semantic = vec![RawSearchResult {
            snippet: "from semantic".into(),
            ..r("a.md", 0.0)
        }];
        let bm25 = vec![RawSearchResult {
            snippet: "from bm25".into(),
            ..r("a.md", 0.0)
        }];
        let mut acc = RrfScoreAccumulator::new();
        acc.accumulate(&semantic, RrfList::Semantic);
        acc.accumulate(&bm25, RrfList::Bm25);
        let inputs = RrfInputs {
            semantic: &semantic,
            bm25: &bm25,
            ..Default::default()
        };
        let results = normalize_and_merge_rrf_results(&acc, &inputs, 10);
        assert_eq!(results[0].snippet, "from semantic");
    }

    #[test]
    fn limit_truncates_output() {
        let bm25: Vec<_> = (0..5).map(|i| r(&format!("p{i}.md"), 0.0)).collect();
        let mut acc = RrfScoreAccumulator::new();
        acc.accumulate(&bm25, RrfList::Bm25);
        let inputs = RrfInputs {
            bm25: &bm25,
            ..Default::default()
        };
        assert_eq!(normalize_and_merge_rrf_results(&acc, &inputs, 3).len(), 3);
    }

    #[test]
    fn rrf_weights_match_ohs_benchmark_values() {
        // Algorithm ported verbatim from obsidian-hybrid-search (MIT) — searcher.ts:1390-1392
        assert!((RRF_WEIGHTS.bm25 - 1.5).abs() < f64::EPSILON);
        assert!((RRF_WEIGHTS.exact_alias - 2.0).abs() < f64::EPSILON);
        assert!((RRF_WEIGHTS.fuzzy - 0.25).abs() < f64::EPSILON);
        assert!((RRF_WEIGHTS.semantic - 1.5).abs() < f64::EPSILON);
    }
}
