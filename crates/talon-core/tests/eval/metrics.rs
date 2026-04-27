use std::collections::HashSet;

use super::GoldenQuery;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvalMetrics {
    pub ndcg_at_5: f64,
    pub ndcg_at_10: f64,
    pub mrr: f64,
    pub hit_at_5: f64,
    pub hit_at_10: f64,
    pub recall_at_10: f64,
    pub query_count: usize,
}

pub fn aggregate_metrics(queries: &[GoldenQuery], results: &[Vec<String>]) -> EvalMetrics {
    assert_eq!(queries.len(), results.len());
    let n = queries.len();
    if n == 0 {
        return EvalMetrics {
            ndcg_at_5: 0.0,
            ndcg_at_10: 0.0,
            mrr: 0.0,
            hit_at_5: 0.0,
            hit_at_10: 0.0,
            recall_at_10: 0.0,
            query_count: 0,
        };
    }
    let result_refs: Vec<Vec<&str>> = results
        .iter()
        .map(|v| v.iter().map(String::as_str).collect())
        .collect();
    let exp_refs: Vec<Vec<&str>> = queries
        .iter()
        .map(|q| q.expected_paths.iter().map(String::as_str).collect())
        .collect();
    let par_refs: Vec<Vec<&str>> = queries
        .iter()
        .map(|q| q.partial_paths.iter().map(String::as_str).collect())
        .collect();

    let sum_ndcg5: f64 = (0..n)
        .map(|i| ndcg(&result_refs[i], &exp_refs[i], &par_refs[i], 5))
        .sum();
    let sum_ndcg10: f64 = (0..n)
        .map(|i| ndcg(&result_refs[i], &exp_refs[i], &par_refs[i], 10))
        .sum();
    let sum_mrr: f64 = (0..n).map(|i| mrr(&result_refs[i], &exp_refs[i])).sum();
    let sum_hit5: f64 = (0..n)
        .map(|i| hit_at_k(&result_refs[i], &exp_refs[i], 5))
        .sum();
    let sum_hit10: f64 = (0..n)
        .map(|i| hit_at_k(&result_refs[i], &exp_refs[i], 10))
        .sum();
    let sum_recall10: f64 = (0..n)
        .map(|i| recall_at_k(&result_refs[i], &exp_refs[i], 10))
        .sum();

    EvalMetrics {
        ndcg_at_5: sum_ndcg5 / n as f64,
        ndcg_at_10: sum_ndcg10 / n as f64,
        mrr: sum_mrr / n as f64,
        hit_at_5: sum_hit5 / n as f64,
        hit_at_10: sum_hit10 / n as f64,
        recall_at_10: sum_recall10 / n as f64,
        query_count: n,
    }
}

/// Normalized Discounted Cumulative Gain at k positions.
pub fn ndcg(results: &[&str], relevant: &[&str], partial: &[&str], k: usize) -> f64 {
    let rel: HashSet<&str> = relevant.iter().copied().collect();
    let par: HashSet<&str> = partial.iter().copied().collect();

    let dcg: f64 = results
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, p)| {
            let grade = if rel.contains(*p) {
                1.0
            } else if par.contains(*p) {
                0.5
            } else {
                0.0
            };
            grade / (i as f64 + 2.0).log2()
        })
        .sum();

    let mut ideal_grades: Vec<f64> = relevant
        .iter()
        .map(|_| 1.0_f64)
        .chain(partial.iter().map(|_| 0.5_f64))
        .collect();
    ideal_grades.sort_by(|a, b| b.partial_cmp(a).unwrap());

    let idcg: f64 = ideal_grades
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, &g)| g / (i as f64 + 2.0).log2())
        .sum();

    if idcg < f64::EPSILON { 0.0 } else { dcg / idcg }
}

/// Mean Reciprocal Rank: 1/rank of the first relevant result, or 0 if none.
pub fn mrr(results: &[&str], relevant: &[&str]) -> f64 {
    let rel: HashSet<&str> = relevant.iter().copied().collect();
    for (i, p) in results.iter().enumerate() {
        if rel.contains(*p) {
            return 1.0 / (i + 1) as f64;
        }
    }
    0.0
}

/// Binary: 1.0 if any relevant result appears in the top-k positions.
pub fn hit_at_k(results: &[&str], relevant: &[&str], k: usize) -> f64 {
    let rel: HashSet<&str> = relevant.iter().copied().collect();
    if results.iter().take(k).any(|p| rel.contains(*p)) {
        1.0
    } else {
        0.0
    }
}

/// Fraction of relevant documents found in the top-k results.
pub fn recall_at_k(results: &[&str], relevant: &[&str], k: usize) -> f64 {
    if relevant.is_empty() {
        return 1.0;
    }
    let rel: HashSet<&str> = relevant.iter().copied().collect();
    let found = results.iter().take(k).filter(|p| rel.contains(**p)).count();
    found as f64 / relevant.len() as f64
}
