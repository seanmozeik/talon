//! Hybrid pipeline result conversion helpers.

use super::super::fuse::fuse_hybrid_result_lists;
use super::super::fuzzy_title::TitleSearchParts;
use super::super::hybrid_single::HybridSingleResult;
use super::super::rrf::{RrfInputs, RrfList, RrfScoreAccumulator, normalize_and_merge_rrf_results};
use super::super::types::{HybridScoreData, RawSearchResult, SearchScores};

/// Runs per-signal weighted RRF on the three [`HybridSingleResult`] buckets
/// and converts the output to [`RawSearchResult`] for cross-variant fusion.
pub(super) fn single_to_raw_list(
    single: &HybridSingleResult,
    limit: usize,
) -> Vec<RawSearchResult> {
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

pub(super) fn lexical_probe_results(
    bm25_probe: &[RawSearchResult],
    title_probe: &TitleSearchParts,
    limit: u32,
) -> Vec<RawSearchResult> {
    let mut title = title_probe.exact_alias.clone();
    title.extend(title_probe.fuzzy.clone());
    fuse_hybrid_result_lists(&[bm25_probe, title.as_slice()], &[1.0, 1.0], limit as usize)
}
