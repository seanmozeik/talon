#![allow(clippy::unwrap_used)]

use std::env::temp_dir;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::inference::{EmbeddingClient, RerankClient};
use rusqlite::{Connection, params};
use serde_json::json;

static COUNTER: AtomicU64 = AtomicU64::new(0);
pub(super) fn unique_db_path() -> std::path::PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    temp_dir().join(format!("talon-hybrid-pipeline-test-{pid}-{n}.sqlite"))
}

pub(super) fn cleanup(path: &std::path::Path) {
    let _ = fs_err::remove_file(path);
    let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
    let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
}

pub(super) fn insert_note(conn: &Connection, vault_path: &str, title: &str, content: &str) {
    conn.execute(
            "INSERT INTO notes \
             (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active) \
             VALUES (?, ?, '[]', '[]', ?, 0, 0, 'h', 'd', 1)",
            params![vault_path, title, content],
        )
        .unwrap();
}

pub(super) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

pub(super) fn dummy_embed_response() -> serde_json::Value {
    // 3-dim vector — search_vector returns empty on empty vec_chunks regardless.
    json!([[0.1_f32, 0.2_f32, 0.3_f32]])
}

pub(super) fn test_clients(uri: impl Into<String>) -> (EmbeddingClient, RerankClient) {
    let uri = uri.into();
    (
        EmbeddingClient::tei_for_tests(uri.clone(), "embed").unwrap(),
        RerankClient::tei_for_tests(uri, 32).unwrap(),
    )
}
