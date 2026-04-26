//! Ranking quality metrics and shared eval infrastructure.
//!
//! Metrics ported from obsidian-hybrid-search (MIT licensed) eval/metrics.ts
//! by flowing-abyss. <https://github.com/flowing-abyss/obsidian-hybrid-search>
//! Attribution: ndcg, mrr, `hit_at_k`, `recall_at_k` formulas match metrics.ts.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    dead_code,
    clippy::cast_precision_loss // all cast sites are over small collections (n ≤ 100)
)]

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;
use serde_json::json;
use wiremock::{Request, Respond, ResponseTemplate};

// ── Content-aware 5-dimensional vector mock ───────────────────────────────
//
// Maps chunk/query text to a 5D binary vector based on keyword presence.
// Ported from ranking-regression.test.ts::makeVector.
//
// Dimensions:
//   0: orchard / apple / harvest / cider  (fruit-orchard domain)
//   1: banana / grove                     (banana domain)
//   2: cafe / café / espresso             (cafe domain)
//   3: graph / link / hub / child         (graph/link domain)
//   4: lifecycle / delete / rename        (lifecycle domain)

fn bool_to_f32(b: bool) -> f32 {
    f32::from(u8::from(b))
}

pub fn make_vector(text: &str) -> Vec<f32> {
    let lower = text.to_lowercase();
    vec![
        bool_to_f32(
            lower.contains("orchard")
                || lower.contains("apple")
                || lower.contains("harvest")
                || lower.contains("cider"),
        ),
        bool_to_f32(lower.contains("banana") || lower.contains("grove")),
        bool_to_f32(lower.contains("cafe") || lower.contains("café") || lower.contains("espresso")),
        bool_to_f32(
            lower.contains("graph")
                || lower.contains("link")
                || lower.contains("hub")
                || lower.contains("child"),
        ),
        bool_to_f32(
            lower.contains("lifecycle") || lower.contains("delete") || lower.contains("rename"),
        ),
    ]
}

// ── Wiremock responders ───────────────────────────────────────────────────

/// Dynamic `/embed` responder — returns a content-aware 5D vector per input.
pub struct SemanticQueryEmbedResponder;

impl Respond for SemanticQueryEmbedResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value =
            serde_json::from_slice(&request.body).unwrap_or_else(|_| json!({"inputs": []}));
        let inputs = body["inputs"].as_array().cloned().unwrap_or_default();
        let vectors: Vec<Vec<f32>> = inputs
            .iter()
            .map(|v| make_vector(v.as_str().unwrap_or("")))
            .collect();
        ResponseTemplate::new(200).set_body_json(json!(vectors))
    }
}

/// Dynamic `/embed-chunked` responder — returns content-aware 5D vectors per chunk.
pub struct SemanticEmbedChunkedResponder;

impl Respond for SemanticEmbedChunkedResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value =
            serde_json::from_slice(&request.body).unwrap_or_else(|_| json!({"input": [[]]}));
        let groups = body["input"].as_array().cloned().unwrap_or_default();
        let data: Vec<serde_json::Value> = groups
            .iter()
            .enumerate()
            .map(|(i, group)| {
                let chunks = group.as_array().cloned().unwrap_or_default();
                let mut embeddings: Vec<Vec<f32>> = chunks
                    .iter()
                    .map(|c| make_vector(c.as_str().unwrap_or("")))
                    .collect();
                if embeddings.is_empty() {
                    embeddings.push(vec![0.0_f32; 5]);
                }
                json!({"embeddings": embeddings, "index": i})
            })
            .collect();
        ResponseTemplate::new(200).set_body_json(json!({"data": data, "model": "semantic-test"}))
    }
}

/// Dynamic `/rerank` responder — scores candidates by keyword overlap with query.
/// Ported from `ranking-regression.test.ts::makeSidecarLayer.rerank`.
pub struct SemanticRerankResponder;

impl Respond for SemanticRerankResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value = serde_json::from_slice(&request.body)
            .unwrap_or_else(|_| json!({"query": "", "texts": []}));
        let texts = body["texts"].as_array().cloned().unwrap_or_default();
        let results: Vec<serde_json::Value> = texts
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let lower = t.as_str().unwrap_or("").to_lowercase();
                let score: f32 = if lower.contains("banana") {
                    0.98
                } else if lower.contains("orchard")
                    || lower.contains("apple")
                    || lower.contains("harvest")
                {
                    0.85
                } else if lower.contains("cafe") || lower.contains("café") {
                    0.60
                } else if lower.contains("graph") || lower.contains("link") || lower.contains("hub")
                {
                    0.70
                } else {
                    0.20
                };
                json!({"index": i, "score": score})
            })
            .collect();
        ResponseTemplate::new(200).set_body_json(json!(results))
    }
}

// ── Shared test helpers ────────────────────────────────────────────────────

pub fn unique_path(label: &str) -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("talon-eval-{label}-{pid}-{n}"))
}

pub fn cleanup(p: &std::path::Path) {
    let _ = fs_err::remove_file(p.join("idx.sqlite"));
    let _ = fs_err::remove_file(p.join("idx.sqlite-wal"));
    let _ = fs_err::remove_file(p.join("idx.sqlite-shm"));
    let _ = fs_err::remove_dir_all(p);
}

pub fn seed_fixture_vault(vault: &std::path::Path) {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault");
    copy_dir_all(&fixtures, vault);
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) {
    fs_err::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let ty = entry.file_type().unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&from, &to);
        } else {
            fs_err::copy(&from, &to).unwrap();
        }
    }
}

// ── Golden-set types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct GoldenQuery {
    pub id: String,
    pub query: String,
    pub expected_paths: Vec<String>,
    #[serde(default)]
    pub partial_paths: Vec<String>,
    pub category: String,
}

pub fn load_golden_set() -> Vec<GoldenQuery> {
    let json = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/golden-set.json"
    ));
    serde_json::from_str(json).expect("golden-set.json must be valid JSON")
}

// ── Aggregated metric results ──────────────────────────────────────────────

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

// ── Core metric functions ──────────────────────────────────────────────────

/// Normalized Discounted Cumulative Gain at k positions.
///
/// Relevance grades: 1.0 for exact match (`expected_paths`), 0.5 for partial,
/// 0.0 otherwise. Discount formula: grade / log2(rank + 1).
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

/// Mean Reciprocal Rank — 1/rank of the first relevant result (0 if none).
pub fn mrr(results: &[&str], relevant: &[&str]) -> f64 {
    let rel: HashSet<&str> = relevant.iter().copied().collect();
    for (i, p) in results.iter().enumerate() {
        if rel.contains(*p) {
            return 1.0 / (i + 1) as f64;
        }
    }
    0.0
}

/// Binary: 1.0 if any relevant result appears in the top-k positions, else 0.0.
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

// ── Metric unit tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndcg_perfect_match_at_rank_1() {
        let score = ndcg(&["a.md", "b.md", "c.md"], &["a.md"], &[], 5);
        assert!((score - 1.0).abs() < 1e-9, "rank-1 perfect nDCG");
    }

    #[test]
    fn ndcg_no_relevant_in_results() {
        let score = ndcg(&["x.md", "y.md"], &["a.md"], &[], 5);
        assert!(score.abs() < 1e-9, "no relevant → nDCG = 0");
    }

    #[test]
    fn ndcg_relevant_at_rank_2() {
        // DCG = 1/log2(3); IDCG = 1/log2(2)
        let score = ndcg(&["x.md", "a.md", "b.md"], &["a.md"], &[], 5);
        let expected = 1.0_f64 / 3_f64.log2();
        assert!((score - expected).abs() < 1e-9);
    }

    #[test]
    fn ndcg_partial_only_scores_half() {
        // DCG = 0.5/log2(2) = 0.5; IDCG = 0.5/log2(2) = 0.5 → nDCG = 1.0
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
}
