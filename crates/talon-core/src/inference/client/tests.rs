#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn no_sleep(_: Duration) {}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn test_client(server_uri: String) -> InferenceClient {
    InferenceClient {
        base_url: server_uri,
        http: HttpClient::builder().build().unwrap(),
        sleep: no_sleep,
        rerank_batch_size: RERANK_BATCH_SIZE,
        _rerank_max_tokens: crate::search::constants::RERANK_MAX_TOKENS,
    }
}

#[test]
fn build_succeeds_with_default_timeout() {
    let client = InferenceClient::new("http://localhost:8080");
    assert!(client.is_ok());
}

#[test]
fn build_succeeds_with_custom_timeout() {
    let client = InferenceClient::with_timeout("http://example", Duration::from_secs(5));
    assert!(client.is_ok());
}

#[test]
fn url_concat_strips_trailing_slash() {
    let a = InferenceClient::new("http://localhost:8080").unwrap();
    let b = InferenceClient::new("http://localhost:8080/").unwrap();
    assert_eq!(a.base_url.trim_end_matches('/'), "http://localhost:8080");
    assert_eq!(b.base_url.trim_end_matches('/'), "http://localhost:8080");
}

#[test]
fn rerank_batches_inputs_and_offsets_indices() {
    let runtime = runtime();
    let server = runtime.block_on(MockServer::start());
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/rerank"))
            .and(body_partial_json(json!({
                "query": "query",
                "texts": ["t0", "t1", "t2", "t3"],
                "raw_scores": false,
                "truncate": true,
                "return_text": false
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"index": 1, "score": 0.4},
                {"index": 0, "score": 0.9}
            ])))
            .mount(&server),
    );
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/rerank"))
            .and(body_partial_json(json!({
                "query": "query",
                "texts": ["t4", "t5"],
                "raw_scores": false,
                "truncate": true,
                "return_text": false
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"index": 0, "score": 0.2}
            ])))
            .mount(&server),
    );

    let client = InferenceClient::new(server.uri()).unwrap();
    let texts: Vec<String> = (0..6).map(|i| format!("t{i}")).collect();
    let result = client.rerank("query", &texts, false).unwrap();
    let got: Vec<(u32, f32)> = result.iter().map(|r| (r.index, r.score)).collect();
    assert_eq!(got, vec![(1, 0.4), (0, 0.9), (4, 0.2)]);

    let requests = runtime.block_on(server.received_requests()).unwrap();
    assert_eq!(requests.len(), 2);

    let first: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    let second: serde_json::Value = serde_json::from_slice(&requests[1].body).unwrap();
    assert_eq!(first["raw_scores"], false);
    assert_eq!(second["raw_scores"], false);
    assert_eq!(first["truncate"], true);
    assert_eq!(second["truncate"], true);
    assert_eq!(first["texts"].as_array().unwrap().len(), 4);
    assert_eq!(second["texts"].as_array().unwrap().len(), 2);
}

#[test]
fn rerank_reads_flat_score_payloads() {
    let runtime = runtime();
    let server = runtime.block_on(MockServer::start());
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/rerank"))
            .and(body_partial_json(json!({
                "query": "query",
                "texts": ["t0"],
                "raw_scores": false,
                "truncate": true,
                "return_text": false
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"index": 0, "score": 0.73}
            ])))
            .mount(&server),
    );

    let client = InferenceClient::new(server.uri()).unwrap();
    let result = client
        .rerank("query", &[String::from("t0")], false)
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].index, 0);
    assert!((result[0].score - 0.73).abs() < f32::EPSILON);
}

#[test]
fn embed_retries_transient_failures_and_succeeds() {
    let runtime = runtime();
    let server = runtime.block_on(MockServer::start());
    let attempts = AtomicUsize::new(0);
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .and(body_partial_json(json!({
                "inputs": ["query"]
            })))
            .respond_with(move |_request: &Request| {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    ResponseTemplate::new(503)
                } else {
                    ResponseTemplate::new(200).set_body_json(json!([[1.0, 2.0, 3.0]]))
                }
            })
            .mount(&server),
    );

    let client = test_client(server.uri());
    let result = client.embed(&["query".to_string()]).unwrap();
    assert_eq!(result, vec![vec![1.0, 2.0, 3.0]]);

    let requests = runtime.block_on(server.received_requests()).unwrap();
    assert_eq!(requests.len(), 2);
}

#[test]
fn embed_gives_up_after_three_consecutive_transient_failures() {
    let runtime = runtime();
    let server = runtime.block_on(MockServer::start());
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .and(body_partial_json(json!({
                "inputs": ["query"]
            })))
            .respond_with(ResponseTemplate::new(503))
            .expect(3)
            .mount(&server),
    );

    let client = test_client(server.uri());
    let err = client.embed(&["query".to_string()]).unwrap_err();
    assert!(matches!(
        err,
        InferenceError::Http {
            status: Some(503),
            ..
        }
    ));

    let requests = runtime.block_on(server.received_requests()).unwrap();
    assert_eq!(requests.len(), 3);
}

#[test]
fn embed_rejects_non_transient_errors_without_retry() {
    let runtime = runtime();
    let server = runtime.block_on(MockServer::start());
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .and(body_partial_json(json!({
                "inputs": ["query"]
            })))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
            .expect(1)
            .mount(&server),
    );

    let client = test_client(server.uri());
    let err = client.embed(&["query".to_string()]).unwrap_err();
    assert!(matches!(
        err,
        InferenceError::Http {
            status: Some(400),
            ..
        }
    ));

    let requests = runtime.block_on(server.received_requests()).unwrap();
    assert_eq!(requests.len(), 1);
}

#[test]
fn embed_chunked_falls_back_to_singleton_requests_after_batch_failure() {
    let runtime = runtime();
    let server = runtime.block_on(MockServer::start());
    let attempts = AtomicUsize::new(0);
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(move |request: &Request| {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                let group_count = body["input"].as_array().map_or(0, Vec::len);
                if attempt < 3 && group_count > 1 {
                    ResponseTemplate::new(503)
                } else {
                    let first = body["input"][0].as_array().unwrap()[0]
                        .as_str()
                        .unwrap()
                        .to_string();
                    let value = if first == "a" { 1.0 } else { 2.0 };
                    ResponseTemplate::new(200).set_body_json(json!({
                        "data": [{
                            "index": 0,
                            "embeddings": [[value, value + 0.5]]
                        }],
                        "model": "test-model"
                    }))
                }
            })
            .mount(&server),
    );

    let client = test_client(server.uri());
    let result = client
        .embed_chunked(&[vec!["a".to_string()], vec!["b".to_string()]])
        .unwrap();

    let got: Vec<(u32, Vec<Vec<f32>>)> = result
        .data
        .iter()
        .map(|row| (row.index, row.embeddings.clone()))
        .collect();
    assert_eq!(
        got,
        vec![(0, vec![vec![1.0, 1.5]]), (1, vec![vec![2.0, 2.5]])]
    );
    assert_eq!(result.model, "test-model");

    let requests = runtime.block_on(server.received_requests()).unwrap();
    assert_eq!(requests.len(), 5);
}

#[test]
fn embed_chunked_retries_batch_before_fallback() {
    let runtime = runtime();
    let server = runtime.block_on(MockServer::start());
    let attempts = AtomicUsize::new(0);
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(move |request: &Request| {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    return ResponseTemplate::new(503);
                }
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                let data: Vec<_> = body["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .enumerate()
                    .map(|(index, _)| {
                        let embedding_value = u8::try_from(index).map_or(0.0, f32::from);
                        json!({
                            "index": index,
                            "embeddings": [[embedding_value]]
                        })
                    })
                    .collect();
                ResponseTemplate::new(200).set_body_json(json!({
                    "data": data,
                    "model": "test-model"
                }))
            })
            .mount(&server),
    );

    let client = test_client(server.uri());
    let result = client
        .embed_chunked(&[vec!["a".to_string()], vec!["b".to_string()]])
        .unwrap();

    assert_eq!(result.data.len(), 2);
    let requests = runtime.block_on(server.received_requests()).unwrap();
    assert_eq!(requests.len(), 2);
}
