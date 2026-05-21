#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::super::pre_filter::PreFilter;
use super::test_support::{cleanup, dummy_embed_response, insert_note, runtime, unique_db_path};
use super::*;
use crate::expansion::client::ExpansionClient;

use crate::store::open_database;

fn test_opts(fast: bool, queries: Vec<String>, intent: Option<String>) -> HybridPipelineOptions {
    HybridPipelineOptions {
        limit: 10,
        candidate_limit: 40,
        fast,
        skip_expansion: false,
        queries,
        intent,
        hooks: SearchHooks::default(),
        pre_filter: PreFilter::none(),
        deadline_at: None,
    }
}

#[test]
fn full_pipeline_calls_embed_expand_and_rerank() {
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
                        "content": "{\"queries\":[\"atomic ideas\",\"note taking systems\"]}"
                    }
                }]
            })))
            .mount(&server),
    );

    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/rerank"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"index": 0, "score": 0.95}
            ])))
            .mount(&server),
    );

    let db_path = unique_db_path();
    let conn = open_database(&db_path).unwrap();

    insert_note(
        &conn,
        "unrelated-a.md",
        "Chemistry Notes",
        "periodic table elements",
    );
    insert_note(
        &conn,
        "unrelated-b.md",
        "History Notes",
        "ancient civilizations events",
    );
    insert_note(
        &conn,
        "target.md",
        "Zettelkasten Method",
        "atomic notes for thinking and learning",
    );

    let (embedding, rerank) = test_support::test_clients(server.uri());
    let expansion = ExpansionClient::new(server.uri(), "test-model").unwrap();

    let opts = test_opts(false, vec![], None);

    let output = run_hybrid_pipeline_with_metadata(
        &conn,
        &embedding,
        &rerank,
        Some(&expansion),
        "atomic notes",
        &opts,
    );
    let results = output.results;

    assert!(
        !results.is_empty(),
        "pipeline must return at least one result"
    );
    assert_eq!(
        output.expanded_queries,
        vec!["atomic ideas", "note taking systems"]
    );
    assert!(
        results.iter().any(|r| r.path == "target.md"),
        "target.md must appear in results"
    );

    drop(conn);
    cleanup(&db_path);
}

#[test]
fn strong_signal_probe_skips_expansion_and_rerank() {
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

    // Insert 100 dummy notes to raise IDF for the unique query term,
    // then the one target note with "crystallophosphene" in its title.
    // High IDF + title weight=10 → BM25 score >= 0.85 → strong signal.
    for i in 0..100 {
        insert_note(
            &conn,
            &format!("dummy-{i}.md"),
            &format!("Unrelated Topic {i}"),
            &format!("content about something completely different topic number {i}"),
        );
    }
    insert_note(
        &conn,
        "signal.md",
        "crystallophosphene Research",
        "unique term found nowhere else",
    );

    let (embedding, rerank) = test_support::test_clients(server.uri());
    let expansion = ExpansionClient::new(server.uri(), "test-model").unwrap();

    let opts = test_opts(false, vec![], None);

    let results = run_hybrid_pipeline(
        &conn,
        &embedding,
        &rerank,
        Some(&expansion),
        "crystallophosphene",
        &opts,
    );

    // The probe should detect a strong signal and skip expansion + rerank.
    let received = rt.block_on(server.received_requests()).unwrap_or_default();
    let expansion_count = received
        .iter()
        .filter(|r| r.url.path() == "/chat/completions")
        .count();
    let rerank_count = received
        .iter()
        .filter(|r| r.url.path() == "/rerank")
        .count();

    assert!(
        expansion_count == 0,
        "expansion must not be called when probe is decisive; \
             got {expansion_count} calls to /chat/completions"
    );
    assert!(
        rerank_count == 0,
        "rerank must not be called when probe is decisive; \
             got {rerank_count} calls to /rerank"
    );

    assert!(
        results.iter().any(|r| r.path == "signal.md"),
        "signal.md must appear in results even when short-circuited"
    );

    drop(conn);
    cleanup(&db_path);
}

#[test]
fn fast_flag_skips_expansion_and_rerank() {
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
        "Fast Search Note",
        "fast lexical search content",
    );

    let (embedding, rerank) = test_support::test_clients(server.uri());
    let expansion = ExpansionClient::new(server.uri(), "test-model").unwrap();

    let opts = test_opts(true, vec![], None);

    let results = run_hybrid_pipeline(&conn, &embedding, &rerank, Some(&expansion), "fast", &opts);

    let received = rt.block_on(server.received_requests()).unwrap_or_default();
    assert!(
        !received.iter().any(|r| r.url.path() == "/chat/completions"),
        "fast mode must not call expansion"
    );
    assert!(
        !received.iter().any(|r| r.url.path() == "/rerank"),
        "fast mode must not call rerank"
    );
    assert!(!results.is_empty(), "fast mode must still return results");

    drop(conn);
    cleanup(&db_path);
}

#[test]
fn no_expansion_client_returns_results() {
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
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server),
    );

    let db_path = unique_db_path();
    let conn = open_database(&db_path).unwrap();
    insert_note(
        &conn,
        "note.md",
        "Knowledge Base",
        "knowledge management and note taking",
    );

    let (embedding, rerank) = test_support::test_clients(server.uri());

    let opts = test_opts(false, vec![], None);

    // expansion=None: pipeline must degrade gracefully (no LLM call).
    let results = run_hybrid_pipeline(
        &conn,
        &embedding,
        &rerank,
        None,
        "knowledge management",
        &opts,
    );

    assert!(
        !results.is_empty(),
        "pipeline must return results without expansion client"
    );

    drop(conn);
    cleanup(&db_path);
}

#[test]
fn pre_supplied_queries_bypass_llm_expansion() {
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
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server),
    );
    // /chat/completions deliberately NOT mocked.

    let db_path = unique_db_path();
    let conn = open_database(&db_path).unwrap();
    insert_note(
        &conn,
        "spaced.md",
        "Spaced Repetition",
        "spaced repetition system for memory",
    );
    insert_note(
        &conn,
        "anki.md",
        "Anki Flashcards",
        "flashcard review system anki",
    );

    let (embedding, rerank) = test_support::test_clients(server.uri());
    let expansion = ExpansionClient::new(server.uri(), "test-model").unwrap();

    let opts = test_opts(false, vec!["anki flashcards".to_owned()], None);

    let results = run_hybrid_pipeline(
        &conn,
        &embedding,
        &rerank,
        Some(&expansion),
        "memory systems",
        &opts,
    );

    let received = rt.block_on(server.received_requests()).unwrap_or_default();
    assert!(
        !received.iter().any(|r| r.url.path() == "/chat/completions"),
        "pre-supplied queries must bypass LLM expansion"
    );
    assert!(
        !results.is_empty(),
        "must return results with pre-supplied queries"
    );

    drop(conn);
    cleanup(&db_path);
}
