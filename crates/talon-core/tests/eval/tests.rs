use super::*;

#[test]
fn ndcg_perfect_match_at_rank_1() {
    let score = ndcg(&["a.md", "b.md", "c.md"], &["a.md"], &[], 5);
    assert!((score - 1.0).abs() < 1e-9, "rank-1 perfect nDCG");
}

#[test]
fn ndcg_no_relevant_in_results() {
    let score = ndcg(&["x.md", "y.md"], &["a.md"], &[], 5);
    assert!(score.abs() < 1e-9, "no relevant -> nDCG = 0");
}

#[test]
fn ndcg_relevant_at_rank_2() {
    let score = ndcg(&["x.md", "a.md", "b.md"], &["a.md"], &[], 5);
    let expected = 1.0_f64 / 3_f64.log2();
    assert!((score - expected).abs() < 1e-9);
}

#[test]
fn ndcg_partial_only_scores_half() {
    let score = ndcg(&["partial.md", "x.md"], &[], &["partial.md"], 5);
    assert!(
        (score - 1.0).abs() < 1e-9,
        "partial only = perfect within its class"
    );
}

#[test]
fn mrr_relevant_first() {
    assert!((mrr(&["a.md", "b.md"], &["a.md"]) - 1.0).abs() < 1e-9);
}

#[test]
fn mrr_relevant_second() {
    assert!((mrr(&["x.md", "a.md"], &["a.md"]) - 0.5).abs() < 1e-9);
}

#[test]
fn mrr_no_relevant() {
    assert!(mrr(&["x.md", "y.md"], &["a.md"]).abs() < 1e-9);
}

#[test]
fn hit_at_k_found_in_window() {
    assert!(hit_at_k(&["x.md", "a.md", "b.md"], &["a.md"], 5) > 0.5);
}

#[test]
fn hit_at_k_not_in_window() {
    assert!(hit_at_k(&["x.md", "a.md", "b.md"], &["a.md"], 1) < 0.5);
}

#[test]
fn recall_at_k_all_found() {
    let score = recall_at_k(&["a.md", "b.md", "c.md"], &["a.md", "b.md"], 5);
    assert!((score - 1.0).abs() < 1e-9);
}

#[test]
fn recall_at_k_half_found() {
    let score = recall_at_k(&["a.md", "x.md"], &["a.md", "b.md"], 5);
    assert!((score - 0.5).abs() < 1e-9);
}
