#![allow(clippy::unwrap_used)]

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::super::pre_filter::PreFilter;
use super::test_support::{cleanup, dummy_embed_response, insert_note, runtime, unique_db_path};
use super::*;
use crate::expansion::client::ExpansionClient;

use crate::store::open_database;

#[test]
fn skip_expansion_keeps_embedding_and_rerank() {
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
            .and(path("/rerank"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"index": 0, "score": 0.95}
            ])))
            .mount(&server),
    );

    let db_path = unique_db_path();
    let conn = open_database(&db_path).unwrap();
    insert_note(
        &conn,
        "note.md",
        "Hybrid Hook Note",
        "vector lexical hybrid retrieval content",
    );

    let (embedding, rerank) = test_support::test_clients(server.uri());
    let expansion = ExpansionClient::new(server.uri(), "test-model").unwrap();

    let opts = HybridPipelineOptions {
        limit: 10,
        candidate_limit: 40,
        fast: false,
        skip_expansion: true,
        queries: Vec::new(),
        intent: None,
        hooks: crate::search::SearchHooks::default(),
        pre_filter: PreFilter::none(),
        deadline_at: None,
    };

    let results = run_hybrid_pipeline(
        &conn,
        &embedding,
        &rerank,
        Some(&expansion),
        "hybrid retrieval",
        &opts,
    );

    let received = rt.block_on(server.received_requests()).unwrap_or_default();
    assert!(received.iter().any(|r| r.url.path() == "/embed"));
    assert!(!received.iter().any(|r| r.url.path() == "/chat/completions"));
    assert!(received.iter().any(|r| r.url.path() == "/rerank"));
    assert!(!results.is_empty());

    drop(conn);
    cleanup(&db_path);
}
