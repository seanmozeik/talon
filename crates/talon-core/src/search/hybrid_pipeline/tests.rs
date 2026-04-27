use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::test_support::{cleanup, dummy_embed_response, insert_note, runtime, unique_db_path};
use super::*;
use crate::expansion::client::ExpansionClient;
use crate::inference::InferenceClient;
use crate::store::open_database;
use std::sync::{Arc, Mutex};
use std::time::Instant;

fn build_recording_hooks(
    events: &Arc<Mutex<Vec<(&'static str, u128)>>>,
    started: Instant,
) -> SearchHooks {
    SearchHooks {
        on_expand_start: Some({
            let events = Arc::clone(events);
            Box::new(move || {
                events
                    .lock()
                    .unwrap()
                    .push(("expand_start", started.elapsed().as_millis()));
            })
        }),
        on_expand_end: Some({
            let events = Arc::clone(events);
            Box::new(move |elapsed_ms| {
                events
                    .lock()
                    .unwrap()
                    .push(("expand_end", u128::from(elapsed_ms)));
            })
        }),
        on_embed_batch: Some({
            let events = Arc::clone(events);
            Box::new(move |batch_size| {
                events
                    .lock()
                    .unwrap()
                    .push(("embed_batch", batch_size as u128));
            })
        }),
        on_rerank_start: Some({
            let events = Arc::clone(events);
            Box::new(move |candidate_count| {
                events
                    .lock()
                    .unwrap()
                    .push(("rerank_start", candidate_count as u128));
            })
        }),
        on_rerank_end: Some({
            let events = Arc::clone(events);
            Box::new(move |elapsed_ms| {
                events
                    .lock()
                    .unwrap()
                    .push(("rerank_end", u128::from(elapsed_ms)));
            })
        }),
    }
}

// ── Test 1: full pipeline end-to-end ────────────────────────────────────

#[test]
fn full_pipeline_calls_embed_expand_and_rerank() {
    let rt = runtime();
    let server = rt.block_on(MockServer::start());

    // /embed: returns a dummy vector for each query call.
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .respond_with(ResponseTemplate::new(200).set_body_json(dummy_embed_response()))
            .mount(&server),
    );

    // /chat/completions: returns two expansion variants.
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

    // /rerank: boosts the target note to rank 0 with high score.
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

    // Seed: a few background notes + one target.
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

    let inference = InferenceClient::new(server.uri()).unwrap();
    let expansion = ExpansionClient::new(server.uri(), "test-model").unwrap();

    let opts = HybridPipelineOptions {
        limit: 10,
        candidate_limit: 40,
        fast: false,
        queries: vec![],
        hooks: SearchHooks::default(),
    };

    let results = run_hybrid_pipeline(&conn, &inference, Some(&expansion), "atomic notes", &opts);

    assert!(
        !results.is_empty(),
        "pipeline must return at least one result"
    );
    assert!(
        results.iter().any(|r| r.path == "target.md"),
        "target.md must appear in results"
    );

    drop(conn);
    cleanup(&db_path);
}

// ── Test 2: strong-signal probe skips expansion and rerank ───────────────

#[test]
fn strong_signal_probe_skips_expansion_and_rerank() {
    let rt = runtime();
    let server = rt.block_on(MockServer::start());

    // Only /embed is mocked; /chat/completions and /rerank are NOT registered.
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

    let inference = InferenceClient::new(server.uri()).unwrap();
    let expansion = ExpansionClient::new(server.uri(), "test-model").unwrap();

    let opts = HybridPipelineOptions {
        limit: 10,
        candidate_limit: 40,
        fast: false,
        queries: vec![],
        hooks: SearchHooks::default(),
    };

    let results = run_hybrid_pipeline(
        &conn,
        &inference,
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

// ── Test 3: fast flag skips expansion and rerank ─────────────────────────

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

    let inference = InferenceClient::new(server.uri()).unwrap();
    let expansion = ExpansionClient::new(server.uri(), "test-model").unwrap();

    let opts = HybridPipelineOptions {
        limit: 10,
        candidate_limit: 40,
        fast: true,
        queries: vec![],
        hooks: SearchHooks::default(),
    };

    let results = run_hybrid_pipeline(&conn, &inference, Some(&expansion), "fast", &opts);

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

// ── Test 4: no expansion client still returns results ────────────────────

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

    let inference = InferenceClient::new(server.uri()).unwrap();

    let opts = HybridPipelineOptions {
        limit: 10,
        candidate_limit: 40,
        fast: false,
        queries: vec![],
        hooks: SearchHooks::default(),
    };

    // expansion=None: pipeline must degrade gracefully (no LLM call).
    let results = run_hybrid_pipeline(&conn, &inference, None, "knowledge management", &opts);

    assert!(
        !results.is_empty(),
        "pipeline must return results without expansion client"
    );

    drop(conn);
    cleanup(&db_path);
}

// ── Test 5: pre-supplied queries bypass LLM ──────────────────────────────

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

    let inference = InferenceClient::new(server.uri()).unwrap();
    let expansion = ExpansionClient::new(server.uri(), "test-model").unwrap();

    let opts = HybridPipelineOptions {
        limit: 10,
        candidate_limit: 40,
        fast: false,
        queries: vec!["anki flashcards".to_owned()],
        hooks: SearchHooks::default(),
    };

    let results = run_hybrid_pipeline(&conn, &inference, Some(&expansion), "memory systems", &opts);

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

// ── Test 6: hooks fire in pipeline order ───────────────────────────────

#[test]
fn hooks_record_expand_before_rerank() {
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
                        "content": "{\"queries\":[\"atomic notes\",\"zettelkasten\"]}"
                    }
                }]
            })))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/rerank"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"index": 0, "score": 0.91}
            ])))
            .mount(&server),
    );

    let db_path = unique_db_path();
    let conn = open_database(&db_path).unwrap();
    insert_note(
        &conn,
        "target.md",
        "Zettelkasten Method",
        "atomic notes for thinking and learning",
    );
    insert_note(
        &conn,
        "related.md",
        "Atomic Notes",
        "small notes connected into a knowledge graph",
    );

    let events: Arc<Mutex<Vec<(&'static str, u128)>>> = Arc::new(Mutex::new(Vec::new()));
    let started = Instant::now();
    let hooks = build_recording_hooks(&events, started);

    let inference = InferenceClient::new(server.uri()).unwrap();
    let expansion = ExpansionClient::new(server.uri(), "test-model").unwrap();
    let opts = HybridPipelineOptions {
        limit: 10,
        candidate_limit: 40,
        fast: false,
        queries: vec![],
        hooks,
    };

    let results = run_hybrid_pipeline(&conn, &inference, Some(&expansion), "atomic notes", &opts);

    assert!(!results.is_empty(), "pipeline must still return results");

    let names: Vec<&str> = {
        let events = events.lock().unwrap();
        events.iter().map(|(name, _)| *name).collect()
    };
    let expand_end = names
        .iter()
        .position(|name| *name == "expand_end")
        .expect("expand_end should fire");
    let rerank_start = names
        .iter()
        .position(|name| *name == "rerank_start")
        .expect("rerank_start should fire");

    assert!(
        expand_end < rerank_start,
        "rerank_start must fire after expand_end; events={names:?}"
    );
    assert!(
        names.contains(&"expand_start")
            && names.contains(&"embed_batch")
            && names.contains(&"rerank_end"),
        "expected all hook stages to fire; events={names:?}"
    );

    drop(conn);
    cleanup(&db_path);
}
