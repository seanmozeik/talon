#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn start_client(uri: String) -> ExpansionClient {
    ExpansionClient::new(uri, "test-model").unwrap()
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn happy_path_returns_variants() {
    let runtime = runtime();
    let server = runtime.block_on(MockServer::start());
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {
                        "content": "{\"queries\":[\"rust async patterns\",\"tokio futures guide\",\"async await rust\"]}"
                    }
                }]
            })))
            .mount(&server),
    );
    let client = start_client(server.uri());
    let result = client.expand("async rust", 4).unwrap();
    assert_eq!(result.len(), 3);
    assert!(result.contains(&"rust async patterns".to_owned()));
    assert!(result.contains(&"tokio futures guide".to_owned()));
}

#[test]
fn request_does_not_cap_thinking_tokens() {
    let runtime = runtime();
    let server = runtime.block_on(MockServer::start());
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {
                        "content": "{\"queries\":[\"zettelkasten links\"]}"
                    }
                }]
            })))
            .mount(&server),
    );
    let client = start_client(server.uri());
    let _ = client.expand("zettelkasten", 2).unwrap();

    let requests = runtime.block_on(server.received_requests()).unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert!(body.get("max_tokens").is_none());
    assert!(body.get("max_completion_tokens").is_none());
}

#[test]
fn request_sends_configured_max_tokens_when_set() {
    let runtime = runtime();
    let server = runtime.block_on(MockServer::start());
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {
                        "content": "{\"queries\":[\"bounded expansion\"]}"
                    }
                }]
            })))
            .mount(&server),
    );
    let client = ExpansionClient::with_max_tokens(server.uri(), "test-model", Some(384)).unwrap();
    let _ = client.expand("bounded", 2).unwrap();

    let requests = runtime.block_on(server.received_requests()).unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["max_tokens"].as_u64(), Some(384));
}

#[test]
fn malformed_json_body_returns_empty_vec() {
    let runtime = runtime();
    let server = runtime.block_on(MockServer::start());
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json at all!!!"))
            .mount(&server),
    );
    let client = start_client(server.uri());
    let result = client.expand("anything", 4).unwrap();
    assert!(result.is_empty(), "malformed body must return empty Vec");
}

#[test]
fn http_5xx_maps_to_expansion_error() {
    let runtime = runtime();
    let server = runtime.block_on(MockServer::start());
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
            .mount(&server),
    );
    let client = start_client(server.uri());
    let err = client.expand("query", 2).unwrap_err();
    assert!(
        matches!(
            err,
            ExpansionError::Http {
                status: Some(500),
                ..
            }
        ),
        "expected Http(500), got {err}"
    );
}

#[test]
fn original_query_excluded_from_variants() {
    let queries = vec![
        "Async Rust".to_owned(),
        "rust async patterns".to_owned(),
        "tokio".to_owned(),
    ];
    let result = normalize_queries("async rust", queries, 4);
    assert!(!result.iter().any(|q| q.to_lowercase() == "async rust"));
    assert_eq!(result.len(), 2);
}

#[test]
fn n_variants_cap_respected() {
    let queries = vec![
        "a".to_owned(),
        "b".to_owned(),
        "c".to_owned(),
        "d".to_owned(),
        "e".to_owned(),
    ];
    let result = normalize_queries("original", queries, 3);
    assert_eq!(result.len(), 3);
}

#[test]
fn strip_code_fences_removes_markdown_wrapper() {
    let wrapped = "```json\n{\"queries\":[\"a\",\"b\"]}\n```";
    let cleaned = strip_code_fences(wrapped);
    assert_eq!(cleaned, "{\"queries\":[\"a\",\"b\"]}");
}
