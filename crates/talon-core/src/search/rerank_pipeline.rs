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
mod tests {
    use super::*;
    use crate::inference::InferenceClient;
    use crate::search::types::SearchScores;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn sigmoid_at_zero_is_one_half() {
        assert!((sigmoid(0.0) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn sigmoid_large_positive_approaches_one() {
        assert!((sigmoid(100.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn sigmoid_large_negative_approaches_zero() {
        assert!(sigmoid(-100.0).abs() < 1e-9);
    }

    #[test]
    fn position_weights_boundary_values() {
        assert_eq!(position_weights(0), (0.75, 0.25));
        assert_eq!(position_weights(9), (0.75, 0.25));
        assert_eq!(position_weights(10), (0.60, 0.40));
        assert_eq!(position_weights(19), (0.60, 0.40));
        assert_eq!(position_weights(20), (0.40, 0.60));
    }

    fn make_candidate(p: &str, score: f64) -> RawSearchResult {
        RawSearchResult {
            path: p.to_string(),
            title: format!("Title {p}"),
            tags: vec![],
            aliases: vec![],
            snippet: format!("snippet for {p}"),
            score,
            scores: SearchScores {
                hybrid: Some(score),
                ..SearchScores::default()
            },
            semantic_heading: None,
            semantic_char_start: None,
            semantic_char_end: None,
        }
    }

    fn start_inference(uri: String) -> InferenceClient {
        InferenceClient::new(uri).unwrap()
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn happy_path_reranks_and_blends_candidates() {
        let rt = runtime();
        let server = rt.block_on(MockServer::start());
        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/rerank"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                    {"index": 0, "score": 0.9},
                    {"index": 1, "score": 0.1},
                ])))
                .mount(&server),
        );
        let inference = start_inference(server.uri());
        let candidates = vec![make_candidate("a.md", 0.5), make_candidate("b.md", 0.4)];
        let result = rerank_candidates(
            &inference,
            "rust async",
            candidates,
            10,
            &SearchHooks::default(),
        );
        assert_eq!(result.len(), 2);
        // a.md had the higher rerank score — must remain first after blending.
        assert_eq!(result[0].path, "a.md");
        // scores.rerank must be populated for all blended candidates.
        assert!(result.iter().all(|r| r.scores.rerank.is_some()));
    }

    #[test]
    fn blend_math_matches_ts_expectations_within_1e4() {
        // Min-max normalization (US-005 / OHS searcher.ts:1299-1325):
        //   min_h=0.4, max_h=0.5, range_h=0.1
        // a (rank 0, w_h=0.75, w_r=0.25):
        //   hybrid01 = (0.5-0.4)/0.1 = 1.0
        //   rerank01 = sigmoid(0.9)
        //   score = 0.75*1.0 + 0.25*sigmoid(0.9)
        // b (rank 1, w_h=0.75, w_r=0.25):
        //   hybrid01 = (0.4-0.4)/0.1 = 0.0
        //   rerank01 = sigmoid(0.1)
        //   score = 0.75*0.0 + 0.25*sigmoid(0.1) = 0.25*sigmoid(0.1)
        let rt = runtime();
        let server = rt.block_on(MockServer::start());
        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/rerank"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                    {"index": 0, "score": 0.9},
                    {"index": 1, "score": 0.1},
                ])))
                .mount(&server),
        );
        let inference = start_inference(server.uri());
        let candidates = vec![make_candidate("a.md", 0.5), make_candidate("b.md", 0.4)];
        let result = rerank_candidates(
            &inference,
            "blend query",
            candidates,
            10,
            &SearchHooks::default(),
        );
        let a = result.iter().find(|r| r.path == "a.md").unwrap();
        let b = result.iter().find(|r| r.path == "b.md").unwrap();
        let expected_a = 0.25_f64.mul_add(sigmoid(0.9_f64), 0.75_f64);
        let expected_b = 0.25_f64 * sigmoid(0.1_f64);
        assert!(
            (a.score - expected_a).abs() < 1e-4,
            "a.score={} expected={}",
            a.score,
            expected_a
        );
        assert!(
            (b.score - expected_b).abs() < 1e-4,
            "b.score={} expected={}",
            b.score,
            expected_b
        );
    }

    #[test]
    fn http_5xx_returns_candidates_with_hybrid_scores_unchanged() {
        let rt = runtime();
        let server = rt.block_on(MockServer::start());
        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/rerank"))
                .respond_with(ResponseTemplate::new(500))
                .mount(&server),
        );
        let inference = start_inference(server.uri());
        let candidates = vec![make_candidate("a.md", 0.8), make_candidate("b.md", 0.3)];
        let result = rerank_candidates(
            &inference,
            "error query",
            candidates,
            10,
            &SearchHooks::default(),
        );
        assert_eq!(result.len(), 2);
        // No rerank scores — graceful degradation.
        assert!(result.iter().all(|r| r.scores.rerank.is_none()));
        // Scores are unchanged from hybrid.
        assert!((result[0].score - 0.8).abs() < 1e-9);
        assert!((result[1].score - 0.3).abs() < 1e-9);
    }

    #[test]
    fn empty_candidates_returns_empty_without_calling_sidecar() {
        // No mock registered — any HTTP call would panic/fail.
        let inference = InferenceClient::new("http://localhost:19999").unwrap();
        let result = rerank_candidates(&inference, "query", vec![], 10, &SearchHooks::default());
        assert!(result.is_empty());
    }

    #[test]
    fn top_k_truncates_candidates_sent_to_reranker() {
        let rt = runtime();
        let server = rt.block_on(MockServer::start());
        // Reranker returns one score for the single candidate it received.
        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/rerank"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                    {"index": 0, "score": 0.9},
                ])))
                .mount(&server),
        );
        let inference = start_inference(server.uri());
        let candidates = vec![
            make_candidate("a.md", 0.5),
            make_candidate("b.md", 0.4),
            make_candidate("c.md", 0.3),
        ];
        // top_k=1: only the first candidate goes to the reranker.
        let result = rerank_candidates(
            &inference,
            "top k query",
            candidates,
            1,
            &SearchHooks::default(),
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "a.md");
        assert!(result[0].scores.rerank.is_some());
    }

    #[test]
    fn repeated_rerank_uses_cache_for_same_query_and_chunk() {
        let rt = runtime();
        let server = rt.block_on(MockServer::start());
        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/rerank"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                    {"index": 0, "score": 0.9},
                ])))
                .mount(&server),
        );
        let inference = start_inference(server.uri());
        let candidates = vec![make_candidate("cached.md", 0.5)];

        let first = rerank_candidates(
            &inference,
            "cache query unique",
            candidates.clone(),
            10,
            &SearchHooks::default(),
        );
        let second = rerank_candidates(
            &inference,
            "cache query unique",
            candidates,
            10,
            &SearchHooks::default(),
        );

        let requests = rt.block_on(server.received_requests()).unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(first[0].scores.rerank, second[0].scores.rerank);
    }

    #[test]
    fn rerank_cache_misses_after_db_version_changes() {
        let rt = runtime();
        let server = rt.block_on(MockServer::start());
        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/rerank"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                    {"index": 0, "score": 0.9},
                ])))
                .mount(&server),
        );
        let inference = start_inference(server.uri());
        let candidates = vec![make_candidate("versioned.md", 0.5)];

        let _ = rerank_candidates_with_db_version(
            &inference,
            "versioned cache query",
            candidates.clone(),
            10,
            &SearchHooks::default(),
            10,
        );
        let _ = rerank_candidates_with_db_version(
            &inference,
            "versioned cache query",
            candidates,
            10,
            &SearchHooks::default(),
            11,
        );

        let requests = rt.block_on(server.received_requests()).unwrap();
        assert_eq!(requests.len(), 2);
    }
}
