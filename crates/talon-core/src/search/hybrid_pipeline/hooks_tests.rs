use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::test_support::{cleanup, dummy_embed_response, insert_note, runtime, unique_db_path};
use super::*;
use crate::expansion::client::ExpansionClient;
use crate::inference::InferenceClient;
use crate::store::open_database;

fn build_recording_hooks(
    events: &Arc<Mutex<Vec<(&'static str, u128)>>>,
    started: Instant,
) -> SearchHooks {
    SearchHooks {
        on_strong_signal: Some({
            let events = Arc::clone(events);
            Box::new(move |top_score| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let bucket = (top_score * 1000.0) as u128;
                events.lock().unwrap().push(("strong_signal", bucket));
            })
        }),
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
    let hooks = build_recording_hooks(&events, Instant::now());

    let inference = InferenceClient::new(server.uri()).unwrap();
    let expansion = ExpansionClient::new(server.uri(), "test-model").unwrap();
    let opts = HybridPipelineOptions {
        limit: 10,
        candidate_limit: 40,
        fast: false,
        queries: vec![],
        intent: None,
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
