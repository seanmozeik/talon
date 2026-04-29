use super::*;
use crate::inference::InferenceClient;
use crate::search::types::SearchScores;
use crate::store::open_database;
use rusqlite::{Connection, params};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn make_candidate(path: &str, score: f64) -> RawSearchResult {
    RawSearchResult {
        path: path.to_owned(),
        title: format!("Title {path}"),
        tags: vec![],
        aliases: vec![],
        snippet: format!("snippet for {path}"),
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

fn unique_db_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "talon-rerank-{name}-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn cleanup(path: &std::path::Path) {
    let _ = fs_err::remove_file(path);
    let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
    let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
}

fn insert_note_with_chunks(conn: &Connection, path: &str, chunks: &[&str]) {
    conn.execute(
        "INSERT INTO notes
         (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
         VALUES (?, 'Chunked Note', '[]', '[]', '', 0, 0, 'h', 'd', 1)",
        params![path],
    )
    .unwrap();
    let note_id = conn.last_insert_rowid();
    for (index, text) in chunks.iter().enumerate() {
        conn.execute(
            "INSERT INTO chunks
             (note_id, chunk_index, text, embedding_text, heading_path, char_start, char_end,
              chunk_hash, token_estimate, embedding_status)
             VALUES (?, ?, ?, '', NULL, 0, 100, ?, 10, 'pending')",
            params![
                note_id,
                i64::try_from(index).unwrap(),
                text,
                format!("h{index}")
            ],
        )
        .unwrap();
    }
}

#[test]
fn intent_weighted_chunk_selection_prefers_intent_rich_chunk() {
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
    let db_path = unique_db_path("chunk-selection");
    let conn = open_database(&db_path).unwrap();
    insert_note_with_chunks(
        &conn,
        "chunked.md",
        &[
            "performance latency unrelated",
            "performance web page load paint metric",
        ],
    );

    let inference = start_inference(server.uri());
    let result = rerank_candidates_with_intent(IntentRerankOptions {
        conn: &conn,
        inference: &inference,
        query: "performance latency",
        intent: Some("web page load"),
        candidates: vec![make_candidate("chunked.md", 0.5)],
        top_k: 10,
        hooks: &SearchHooks::default(),
        db_version: 100,
    });

    assert_eq!(result[0].snippet, "performance web page load paint metric");
    drop(conn);
    cleanup(&db_path);
}

#[test]
fn chunk_selection_weights_intent_terms_above_query_terms() {
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
    let db_path = unique_db_path("chunk-selection-intent-weight");
    let conn = open_database(&db_path).unwrap();
    insert_note_with_chunks(
        &conn,
        "weighted.md",
        &[
            "performance latency throughput regression",
            "performance launch blockers current actions",
        ],
    );

    let inference = start_inference(server.uri());
    let result = rerank_candidates_with_intent(IntentRerankOptions {
        conn: &conn,
        inference: &inference,
        query: "performance latency throughput regression",
        intent: Some("launch blockers current actions"),
        candidates: vec![make_candidate("weighted.md", 0.5)],
        top_k: 10,
        hooks: &SearchHooks::default(),
        db_version: 102,
    });

    assert_eq!(
        result[0].snippet,
        "performance launch blockers current actions"
    );
    drop(conn);
    cleanup(&db_path);
}

#[test]
fn rerank_cache_key_includes_prefixed_intent_query() {
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
    let db_path = unique_db_path("intent-cache");
    let conn = open_database(&db_path).unwrap();
    let inference = start_inference(server.uri());
    let candidates = vec![make_candidate("intent-cache.md", 0.5)];

    let _ = rerank_candidates_with_intent(IntentRerankOptions {
        conn: &conn,
        inference: &inference,
        query: "cache query with intent",
        intent: Some("web page load"),
        candidates: candidates.clone(),
        top_k: 10,
        hooks: &SearchHooks::default(),
        db_version: 101,
    });
    let _ = rerank_candidates_with_intent(IntentRerankOptions {
        conn: &conn,
        inference: &inference,
        query: "cache query with intent",
        intent: Some("web page load"),
        candidates: candidates.clone(),
        top_k: 10,
        hooks: &SearchHooks::default(),
        db_version: 101,
    });
    let _ = rerank_candidates_with_intent(IntentRerankOptions {
        conn: &conn,
        inference: &inference,
        query: "cache query with intent",
        intent: Some("sports training"),
        candidates,
        top_k: 10,
        hooks: &SearchHooks::default(),
        db_version: 101,
    });

    let requests = rt.block_on(server.received_requests()).unwrap();
    assert_eq!(requests.len(), 2);
    drop(conn);
    cleanup(&db_path);
}
