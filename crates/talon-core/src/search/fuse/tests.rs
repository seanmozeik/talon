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

fn r_with_hybrid(path: &str, hybrid: f64) -> RawSearchResult {
    let mut x = r(path, hybrid);
    x.scores.hybrid = Some(hybrid);
    x
}

#[test]
fn clamp01_clamps_below_zero_and_above_one() {
    assert_eq!(clamp01(-0.5), 0.0);
    assert_eq!(clamp01(0.3), 0.3);
    assert_eq!(clamp01(2.5), 1.0);
}

#[test]
fn sigmoid_zero_is_one_half() {
    assert!((sigmoid(0.0) - 0.5).abs() < 1e-9);
}

#[test]
fn rerank_weight_partitions_into_three_tiers() {
    assert_eq!(rerank_weight_for_rank(0), RERANK_WEIGHT_TOP);
    assert_eq!(rerank_weight_for_rank(9), RERANK_WEIGHT_TOP);
    assert_eq!(rerank_weight_for_rank(10), RERANK_WEIGHT_MID);
    assert_eq!(rerank_weight_for_rank(19), RERANK_WEIGHT_MID);
    assert_eq!(rerank_weight_for_rank(20), RERANK_WEIGHT_LOW);
    assert_eq!(rerank_weight_for_rank(100), RERANK_WEIGHT_LOW);
}

#[test]
fn strong_signal_requires_high_score_and_gap() {
    // Both conditions met: score 0.9 >= 0.85, gap 0.2 >= 0.15.
    assert!(estimate_strong_signal(&[r("a", 0.9), r("b", 0.7)]));

    // Score too low: 0.8 < 0.85.
    assert!(!estimate_strong_signal(&[r("a", 0.8), r("b", 0.6)]));

    // Gap too small: 0.9 - 0.85 = 0.05 < 0.15.
    assert!(!estimate_strong_signal(&[r("a", 0.9), r("b", 0.85)]));

    // Single result with high score: gap = 0.9 - 0 = 0.9 >= 0.15, satisfies both.
    assert!(estimate_strong_signal(&[r("a", 0.9)]));

    // Empty probe returns false.
    assert!(!estimate_strong_signal(&[]));

    // Borderline score exactly at threshold: 0.85 >= 0.85 ✓, gap 0.7 >= 0.15 ✓.
    assert!(estimate_strong_signal(&[r("a", 0.85), r("b", 0.7)]));

    // Score below threshold by epsilon: 0.84999 < 0.85.
    assert!(!estimate_strong_signal(&[r("a", 0.84999), r("b", 0.69999)]));

    // Borderline gap exactly at threshold: 0.85 - 0.7 = 0.15 >= 0.15 ✓.
    assert!(estimate_strong_signal(&[r("a", 0.85), r("b", 0.7)]));

    // Gap below threshold by epsilon: 0.9 - 0.75001 = 0.14999 < 0.15.
    assert!(!estimate_strong_signal(&[r("a", 0.9), r("b", 0.75001)]));
}

#[test]
fn fuse_single_list_passes_through_unchanged() {
    let list = vec![r("a.md", 0.4), r("b.md", 0.3)];
    let lists: Vec<&[RawSearchResult]> = vec![&list];
    let out = fuse_hybrid_result_lists(&lists, 10);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].path, "a.md");
}

#[test]
fn fuse_two_lists_top_intersection_wins() {
    let l1 = vec![r("a.md", 0.0), r("b.md", 0.0)];
    let l2 = vec![r("a.md", 0.0), r("c.md", 0.0)];
    let lists: Vec<&[RawSearchResult]> = vec![&l1, &l2];
    let out = fuse_hybrid_result_lists(&lists, 10);
    assert_eq!(out[0].path, "a.md");
    assert!(out[0].score >= out[1].score);
    // Hybrid score is recorded.
    assert!(out[0].scores.hybrid.is_some());
}

#[test]
fn fuse_normalizes_top_intersection_to_one() {
    // Two lists, same path at rank 0 in both → hits the per-list cap on
    // both sides → normalized to 1.0.
    let l1 = vec![r("a.md", 0.0)];
    let l2 = vec![r("a.md", 0.0)];
    let lists: Vec<&[RawSearchResult]> = vec![&l1, &l2];
    let out = fuse_hybrid_result_lists(&lists, 10);
    assert!((out[0].score - 1.0).abs() < 1e-9);
}

#[test]
fn blend_rerank_uses_rerank_when_provided() {
    let candidates = vec![r_with_hybrid("a.md", 0.5), r_with_hybrid("b.md", 0.4)];
    let scores = vec![Some(0.9_f64), Some(0.1_f64)];
    let out = blend_rerank_candidates(&candidates, &scores);
    // min-max normalization: min=0.4, max=0.5, range=0.1.
    // sigmoid is always applied to rerank logits (US-005 / OHS searcher.ts:1319).
    // a (rank 0, w_h=0.75, w_r=0.25):
    //   hybrid01 = (0.5 - 0.4) / 0.1 = 1.0
    //   rerank01 = sigmoid(0.9) ≈ 0.71095
    //   score = 0.75 * 1.0 + 0.25 * sigmoid(0.9)
    // b (rank 1, w_h=0.75, w_r=0.25):
    //   hybrid01 = (0.4 - 0.4) / 0.1 = 0.0
    //   rerank01 = sigmoid(0.1) ≈ 0.52498
    //   score = 0.75 * 0.0 + 0.25 * sigmoid(0.1)
    let a = out.iter().find(|r| r.path == "a.md").unwrap();
    let b = out.iter().find(|r| r.path == "b.md").unwrap();
    let expected_a = 0.25_f64.mul_add(sigmoid(0.9_f64), 0.75_f64);
    let expected_b = 0.25_f64 * sigmoid(0.1_f64);
    assert!(
        (a.score - expected_a).abs() < 1e-9,
        "a.score={} expected={}",
        a.score,
        expected_a
    );
    assert!(
        (b.score - expected_b).abs() < 1e-9,
        "b.score={} expected={}",
        b.score,
        expected_b
    );
    // scores.rerank stores the sigmoid'd value, not the raw logit.
    assert!((a.scores.rerank.unwrap() - sigmoid(0.9_f64)).abs() < 1e-9);
}

#[test]
fn blend_rerank_passes_through_when_no_rerank_score() {
    let candidates = vec![r_with_hybrid("a.md", 0.5)];
    let scores = vec![None];
    let out = blend_rerank_candidates(&candidates, &scores);
    assert_eq!(out[0].score, 0.5);
    assert_eq!(out[0].scores.rerank, None);
}

#[test]
fn blend_rerank_always_applies_sigmoid() {
    let candidates = vec![r_with_hybrid("a.md", 0.5)];
    // Sigmoid is always applied to logits (US-005). With a single candidate,
    // min == max so range_h = 1.0 → hybrid01 = (0.5 - 0.5) / 1.0 = 0.0.
    // score = 0.75 * 0.0 + 0.25 * sigmoid(100) ≈ 0.25.
    let scores = vec![Some(100.0_f64)];
    let out = blend_rerank_candidates(&candidates, &scores);
    assert!((out[0].score - 0.25).abs() < 1e-3);
    // scores.rerank holds the sigmoid'd value (~1.0), not the raw logit (100.0).
    assert!((out[0].scores.rerank.unwrap() - sigmoid(100.0_f64)).abs() < 1e-9);
}

#[test]
fn rrf_hybrid_score_never_exceeds_one() {
    // RRF normalization divides by Σweights/(k+1) so the theoretical
    // maximum is 1.0.  Verify this holds with many lists and many results.
    // Reference: obsidian-hybrid-search searcher.ts:748-759.
    let l1: Vec<RawSearchResult> = (0..20).map(|i| r(&format!("{i}.md"), 0.0)).collect();
    let l2: Vec<RawSearchResult> = (0..20).map(|i| r(&format!("{i}.md"), 0.0)).collect();
    let l3: Vec<RawSearchResult> = (0..20).map(|i| r(&format!("{i}.md"), 0.0)).collect();
    let lists: Vec<&[RawSearchResult]> = vec![&l1, &l2, &l3];
    let out = fuse_hybrid_result_lists(&lists, 20);
    for result in &out {
        assert!(
            result.score <= 1.0 + f64::EPSILON,
            "RRF hybrid score must be ≤ 1.0, got {} for {}",
            result.score,
            result.path
        );
    }
}

#[test]
fn blend_rerank_min_max_normalizes_hybrid_scores() {
    // Min-max normalization: min=0.1, max=0.9, range=0.8.
    // Both signals use sigmoid (US-005 / OHS searcher.ts:1299-1325).
    // high.md (rank 0, w_h=0.75): hybrid01 = (0.9-0.1)/0.8 = 1.0
    //   rerank01 = sigmoid(0.0) = 0.5
    //   score = 0.75 * 1.0 + 0.25 * 0.5 = 0.875  ← proves min-max fired
    // Without min-max: hybrid01 = 0.9/0.9 = 1.0 (same result here, but
    //   differs for the low candidate).
    // low.md (rank 1, w_h=0.75): hybrid01 = (0.1-0.1)/0.8 = 0.0
    //   rerank01 = sigmoid(100) ≈ 1.0
    //   score = 0.75 * 0.0 + 0.25 * 1.0 = 0.25
    let candidates = vec![
        r_with_hybrid("high.md", 0.9_f64),
        r_with_hybrid("low.md", 0.1_f64),
    ];
    let scores = vec![Some(0.0_f64), Some(100.0_f64)];
    let out = blend_rerank_candidates(&candidates, &scores);

    let high = out.iter().find(|r| r.path == "high.md").unwrap();
    let low = out.iter().find(|r| r.path == "low.md").unwrap();

    // high.md: 0.75 * 1.0 + 0.25 * sigmoid(0.0) = 0.75 + 0.125 = 0.875
    let expected_high = 0.25_f64.mul_add(sigmoid(0.0_f64), 0.75_f64);
    assert!(
        (high.score - expected_high).abs() < 1e-9,
        "high.md score: expected {expected_high:.4}, got {}",
        high.score
    );

    // low.md: 0.75 * 0.0 + 0.25 * sigmoid(100) ≈ 0.25
    let expected_low = 0.25_f64 * sigmoid(100.0_f64);
    assert!(
        (low.score - expected_low).abs() < 1e-9,
        "low.md score: expected {expected_low:.4}, got {}",
        low.score
    );
}
