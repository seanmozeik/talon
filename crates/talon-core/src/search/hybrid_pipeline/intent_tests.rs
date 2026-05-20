#![allow(clippy::unwrap_used)]

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::super::pre_filter::PreFilter;
use super::test_support::{cleanup, dummy_embed_response, insert_note, runtime, unique_db_path};
use super::*;
use crate::expansion::client::ExpansionClient;
use crate::inference::InferenceClient;
use crate::store::open_database;

#[test]
fn intent_disables_strong_signal_probe_short_circuit() {
    let rt = runtime();
    let server = rt.block_on(MockServer::start());

    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .respond_with(ResponseTemplate::new(200).set_body_json(dummy_embed_response()))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {
                        "content": "{\"queries\":[\"crystallophosphene web performance\"]}"
                    }
                }]
            })))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/rerank"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"index": 0, "score": 0.7}
            ])))
            .mount(&server),
    );

    let db_path = unique_db_path();
    let conn = open_database(&db_path).unwrap();
    for i in 0..100 {
        insert_note(
            &conn,
            &format!("dummy-intent-{i}.md"),
            &format!("Unrelated Intent Topic {i}"),
            &format!("content about something completely different topic number {i}"),
        );
    }
    insert_note(
        &conn,
        "signal-intent.md",
        "crystallophosphene Research",
        "unique term found nowhere else",
    );

    let inference = InferenceClient::new(server.uri()).unwrap();
    let expansion = ExpansionClient::new(server.uri(), "test-model").unwrap();
    let opts = HybridPipelineOptions {
        limit: 10,
        candidate_limit: 40,
        fast: false,
        retrieval_only: false,
        queries: vec![],
        intent: Some("web page load".to_owned()),
        hooks: SearchHooks::default(),
        pre_filter: PreFilter::none(),
        deadline_at: None,
    };

    let _ = run_hybrid_pipeline(
        &conn,
        &inference,
        Some(&expansion),
        "crystallophosphene",
        &opts,
    );

    let received = rt.block_on(server.received_requests()).unwrap_or_default();
    assert!(received.iter().any(|r| r.url.path() == "/chat/completions"));
    assert!(received.iter().any(|r| r.url.path() == "/rerank"));

    drop(conn);
    cleanup(&db_path);
}
