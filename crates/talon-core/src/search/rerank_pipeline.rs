//! Cross-encoder rerank pipeline.
//!
//! Thin orchestration layer that calls the inference sidecar's `/rerank`
//! endpoint and blends the cross-encoder scores into the hybrid scores using
//! [`super::fuse::blend_rerank_candidates`].
//!
//! Ports `services/talon/search/rerank-pipeline.ts`. The TS reference uses
//! Effect and an LLM cache layer; this Rust port calls the sidecar directly
//! and delegates blending to the existing `fuse` module. Caching can be added
//! on top by the pipeline orchestrator (US-005).

use crate::inference::InferenceClient;

use super::fuse::blend_rerank_candidates;
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
) -> Vec<RawSearchResult> {
    if candidates.is_empty() {
        return candidates;
    }

    let limit = (top_k as usize).min(candidates.len());
    let active: Vec<RawSearchResult> = candidates.into_iter().take(limit).collect();

    let texts: Vec<String> = active.iter().map(rerank_text).collect();

    let Ok(rerank_results) = inference.rerank(query, &texts, false) else {
        return active;
    };

    let mut scores: Vec<Option<f64>> = vec![None; limit];
    for result in rerank_results {
        if let Some(slot) = scores.get_mut(result.index as usize) {
            *slot = Some(f64::from(result.score));
        }
    }

    blend_rerank_candidates(&active, &scores)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::inference::InferenceClient;
    use crate::search::fuse::sigmoid;
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
        let result = rerank_candidates(&inference, "rust async", candidates, 10);
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
        let result = rerank_candidates(&inference, "query", candidates, 10);
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
        let result = rerank_candidates(&inference, "query", candidates, 10);
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
        let result = rerank_candidates(&inference, "query", vec![], 10);
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
        let result = rerank_candidates(&inference, "query", candidates, 1);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "a.md");
        assert!(result[0].scores.rerank.is_some());
    }
}
