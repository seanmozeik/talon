#![allow(clippy::unwrap_used)]

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::super::pre_filter::PreFilter;
use super::test_support::{cleanup, dummy_embed_response, insert_note, runtime, unique_db_path};
use super::*;
use crate::expansion::client::ExpansionClient;
use crate::inference::InferenceClient;
use crate::store::open_database;

#[test]
fn retrieval_only_keeps_embedding_but_skips_expansion_and_rerank() {
    let rt = runtime();
    let server = rt.block_on(MockServer::start());

    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .respond_with(ResponseTemplate::new(200).set_body_json(dummy_embed_response()))
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

    let inference = InferenceClient::new(server.uri()).unwrap();
    let expansion = ExpansionClient::new(server.uri(), "test-model").unwrap();

    let opts = HybridPipelineOptions {
        limit: 10,
        candidate_limit: 40,
        fast: false,
        retrieval_only: true,
        queries: Vec::new(),
        intent: None,
        hooks: crate::search::SearchHooks::default(),
        pre_filter: PreFilter::none(),
        deadline_at: None,
    };

    let results = run_hybrid_pipeline(
        &conn,
        &inference,
        Some(&expansion),
        "hybrid retrieval",
        &opts,
    );

    let received = rt.block_on(server.received_requests()).unwrap_or_default();
    assert!(
        received.iter().any(|r| r.url.path() == "/embed"),
        "retrieval-only mode must still call embedding"
    );
    assert!(
        !received.iter().any(|r| r.url.path() == "/chat/completions"),
        "retrieval-only mode must not call expansion"
    );
    assert!(
        !received.iter().any(|r| r.url.path() == "/rerank"),
        "retrieval-only mode must not call rerank"
    );
    assert!(
        !results.is_empty(),
        "retrieval-only mode must still return results"
    );

    drop(conn);
    cleanup(&db_path);
}
