//! End-to-end runner tests against an in-process mock sidecar.

use std::env::temp_dir;
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::{Connection, params};
use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::inference::EmbeddingClient;
use crate::store::open_database;
use crate::vec_ext::register_sqlite_vec;

fn unique_path(label: &str) -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    temp_dir().join(format!("talon-runner-test-{label}-{pid}-{n}.sqlite"))
}

fn cleanup(p: &std::path::Path) {
    let _ = fs_err::remove_file(p);
    let _ = fs_err::remove_file(p.with_extension("sqlite-wal"));
    let _ = fs_err::remove_file(p.with_extension("sqlite-shm"));
}

fn seed_note(conn: &Connection, vault_path: &str, chunks: &[&str]) {
    conn.execute(
            "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
             VALUES (?, ?, '[]', '[]', '', 0, 0, 'h', 'd', 1)",
            params![vault_path, vault_path],
        ).unwrap();
    let note_id = conn.last_insert_rowid();
    for (i, text) in chunks.iter().enumerate() {
        conn.execute(
                "INSERT INTO chunks (note_id, chunk_index, text, embedding_text, heading_path, char_start, char_end, chunk_hash, token_estimate, embedding_status)
                 VALUES (?, ?, ?, ?, '', 0, 0, ?, 1, 'pending')",
                params![note_id, i64::try_from(i).unwrap(), text, text, format!("h{i}")],
            ).unwrap();
    }
}

#[test]
fn single_chunk_path_persists_vector_and_marks_ok() {
    register_sqlite_vec().unwrap();
    let db = unique_path("single");
    let conn = open_database(&db).unwrap();
    seed_note(&conn, "single.md", &["hello world"]);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = runtime.block_on(MockServer::start());
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .and(body_partial_json(json!({"inputs": ["hello world"]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([[0.1, 0.2, 0.3, 0.4]])))
            .mount(&server),
    );

    let client = EmbeddingClient::tei_for_tests(server.uri(), "embed").unwrap();
    let stats = run_embed_pass(&conn, &client, &EmbedPassOptions::defaults()).unwrap();
    assert_eq!(stats.processed, 1);
    assert_eq!(stats.succeeded, 1);
    assert_eq!(stats.failed, 0);
    assert!(!stats.dimension_mismatch);

    let dims: i64 = conn
        .query_row("SELECT dimensions FROM vector_metadata LIMIT 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(dims, 4);
    let chunk_status: String = conn
        .query_row(
            "SELECT embedding_status FROM chunks WHERE chunk_index = 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(chunk_status, "ok");

    drop(conn);
    cleanup(&db);
}

#[test]
fn multi_chunk_path_persists_each_chunk() {
    register_sqlite_vec().unwrap();
    let db = unique_path("multi");
    let conn = open_database(&db).unwrap();
    seed_note(&conn, "multi.md", &["alpha", "beta", "gamma"]);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = runtime.block_on(MockServer::start());
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{
                    "embeddings": [
                        [0.1, 0.2, 0.3],
                        [0.4, 0.5, 0.6],
                        [0.7, 0.8, 0.9],
                    ],
                    "index": 0,
                }],
                "model": "embed_chunked",
            })))
            .mount(&server),
    );

    let client = EmbeddingClient::tei_for_tests(server.uri(), "embed").unwrap();
    let stats = run_embed_pass(&conn, &client, &EmbedPassOptions::defaults()).unwrap();
    assert_eq!(stats.succeeded, 1);
    assert_eq!(stats.failed, 0);
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM vector_metadata", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 3);
    drop(conn);
    cleanup(&db);
}

#[test]
fn http_error_marks_note_failed_and_records_diagnostic() {
    register_sqlite_vec().unwrap();
    let db = unique_path("err");
    let conn = open_database(&db).unwrap();
    seed_note(&conn, "bad.md", &["one"]);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = runtime.block_on(MockServer::start());
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .respond_with(ResponseTemplate::new(500).set_body_string("upstream model OOM"))
            .mount(&server),
    );

    let client = EmbeddingClient::tei_for_tests(server.uri(), "embed").unwrap();
    let stats = run_embed_pass(&conn, &client, &EmbedPassOptions::defaults()).unwrap();
    assert_eq!(stats.processed, 1);
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.succeeded, 0);
    assert_eq!(stats.diagnostics.len(), 1);
    let chunk_status: String = conn
        .query_row(
            "SELECT embedding_status FROM chunks WHERE chunk_index = 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(chunk_status, "failed");

    drop(conn);
    cleanup(&db);
}

#[test]
fn missing_single_chunk_endpoint_aborts_embed_pass() {
    register_sqlite_vec().unwrap();
    let db = unique_path("missing-single");
    let conn = open_database(&db).unwrap();
    seed_note(&conn, "bad.md", &["one"]);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = runtime.block_on(MockServer::start());

    let client = EmbeddingClient::tei_for_tests(server.uri(), "embed").unwrap();
    let err = run_embed_pass(&conn, &client, &EmbedPassOptions::defaults()).unwrap_err();
    assert!(
        err.to_string().contains("embedding endpoint unavailable"),
        "{err}"
    );
    let chunk_status: String = conn
        .query_row(
            "SELECT embedding_status FROM chunks WHERE chunk_index = 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(chunk_status, "pending");

    drop(conn);
    cleanup(&db);
}

#[test]
fn missing_chunked_endpoint_aborts_embed_pass() {
    register_sqlite_vec().unwrap();
    let db = unique_path("missing-chunked");
    let conn = open_database(&db).unwrap();
    seed_note(&conn, "bad.md", &["one", "two"]);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = runtime.block_on(MockServer::start());

    let client = EmbeddingClient::tei_for_tests(server.uri(), "embed").unwrap();
    let err = run_embed_pass(&conn, &client, &EmbedPassOptions::defaults()).unwrap_err();
    assert!(
        err.to_string().contains("embedding endpoint unavailable"),
        "{err}"
    );
    let pending_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE embedding_status = 'pending'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pending_count, 2);

    drop(conn);
    cleanup(&db);
}

#[test]
fn dimension_mismatch_is_reported() {
    register_sqlite_vec().unwrap();
    let db = unique_path("dim");
    let conn = open_database(&db).unwrap();
    seed_note(&conn, "first.md", &["a"]);
    seed_note(&conn, "second.md", &["b"]);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = runtime.block_on(MockServer::start());
    // Two requests, two different shapes — second one trips the mismatch.
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .and(body_partial_json(json!({"inputs": ["a"]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([[0.1, 0.2, 0.3, 0.4]])))
            .mount(&server),
    );
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .and(body_partial_json(json!({"inputs": ["b"]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([[0.1, 0.2]])))
            .mount(&server),
    );

    let client = EmbeddingClient::tei_for_tests(server.uri(), "embed").unwrap();
    let stats = run_embed_pass(&conn, &client, &EmbedPassOptions::defaults()).unwrap();
    assert!(stats.dimension_mismatch);
    assert_eq!(stats.succeeded, 1);
    assert_eq!(stats.failed, 1);

    drop(conn);
    cleanup(&db);
}
