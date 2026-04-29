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
    let result = rerank_candidates(
        &inference,
        "rust async",
        candidates,
        10,
        &SearchHooks::default(),
    );
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].path, "a.md");
    assert!(result.iter().all(|r| r.scores.rerank.is_some()));
}

#[test]
fn blend_math_matches_ts_expectations_within_1e4() {
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
    let expected_a = 0.25_f64.mul_add(0.9_f64, 0.75_f64);
    let expected_b = 0.25_f64 * 0.1_f64;
    assert!((a.score - expected_a).abs() < 1e-4);
    assert!((b.score - expected_b).abs() < 1e-4);
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
    assert!(result.iter().all(|r| r.scores.rerank.is_none()));
    assert!((result[0].score - 0.8).abs() < 1e-9);
    assert!((result[1].score - 0.3).abs() < 1e-9);
}

#[test]
fn empty_candidates_returns_empty_without_calling_sidecar() {
    let inference = InferenceClient::new("http://localhost:19999").unwrap();
    let result = rerank_candidates(&inference, "query", vec![], 10, &SearchHooks::default());
    assert!(result.is_empty());
}

#[test]
fn top_k_truncates_candidates_sent_to_reranker() {
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
    let candidates = vec![
        make_candidate("a.md", 0.5),
        make_candidate("b.md", 0.4),
        make_candidate("c.md", 0.3),
    ];
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
fn versioned_rerank_uses_cache_for_same_query_and_chunk() {
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

    let first = rerank_candidates_with_db_version(
        &inference,
        "cache query unique",
        candidates.clone(),
        10,
        &SearchHooks::default(),
        20,
    );
    let second = rerank_candidates_with_db_version(
        &inference,
        "cache query unique",
        candidates,
        10,
        &SearchHooks::default(),
        20,
    );

    let requests = rt.block_on(server.received_requests()).unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(first[0].scores.rerank, second[0].scores.rerank);
}

#[test]
fn public_rerank_wrapper_does_not_use_versionless_cache() {
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
    let candidates = vec![make_candidate("uncached.md", 0.5)];

    let _ = rerank_candidates(
        &inference,
        "uncached query unique",
        candidates.clone(),
        10,
        &SearchHooks::default(),
    );
    let _ = rerank_candidates(
        &inference,
        "uncached query unique",
        candidates,
        10,
        &SearchHooks::default(),
    );

    let requests = rt.block_on(server.received_requests()).unwrap();
    assert_eq!(requests.len(), 2);
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
