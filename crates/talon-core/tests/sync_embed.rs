//! Integration test: `talon sync` folds the embed pass into the same call.
//!
//! Per US-004b, `talon sync` runs the embed pass after reconciliation when
//! `--fast` is absent; `--fast` skips inference entirely.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::json;
use std::env::temp_dir;
use std::sync::atomic::{AtomicU64, Ordering};
use talon_core::{
    embed::EmbedPassOptions, indexer::IndexerConfig, inference::InferenceClient, open_database,
    run_sync, vec_ext::register_sqlite_vec,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn unique_path(label: &str) -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    temp_dir().join(format!("talon-sync-embed-{label}-{pid}-{n}"))
}

fn cleanup(p: &std::path::Path) {
    let _ = fs_err::remove_file(p.join("idx.sqlite"));
    let _ = fs_err::remove_file(p.join("idx.sqlite-wal"));
    let _ = fs_err::remove_file(p.join("idx.sqlite-shm"));
    let _ = fs_err::remove_dir_all(p);
}

fn seed_vault(vault: &std::path::Path) {
    fs_err::create_dir_all(vault).unwrap();
    fs_err::write(
        vault.join("note-a.md"),
        "# Note A\n\nThis is the content of note A.\n\nIt has some text for embedding.",
    )
    .unwrap();
    fs_err::write(
        vault.join("note-b.md"),
        "# Note B\n\nNote B content with different text.",
    )
    .unwrap();
}

#[test]
fn sync_with_embed_populates_vec_chunks() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("sync-embed");
    seed_vault(&vault);
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let server = runtime.block_on(MockServer::start());
    eprintln!("mock server at: {}", server.uri());
    // Mock single-chunk embed endpoint
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([[0.1, 0.2, 0.3, 0.4]])))
            .mount(&server),
    );

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let opts = EmbedPassOptions::defaults();
    let config = IndexerConfig::index_all();

    let (stats, embed_stats) =
        run_sync(&mut conn, &vault, &lock, &config, Some(opts), Some(&client)).unwrap();

    // Full scan indexes both notes
    assert_eq!(stats.indexed, 2);
    assert_eq!(stats.deleted, 0);

    // Check pending chunks
    let pending: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE embedding_status = 'pending'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    eprintln!("pending chunks after sync: {pending}");

    // Check chunk statuses
    let statuses: String = conn
        .query_row(
            "SELECT GROUP_CONCAT(embedding_status, ',') FROM chunks",
            [],
            |r| r.get(0),
        )
        .unwrap_or_default();
    eprintln!("chunk statuses: {statuses}");

    // Embed pass runs and succeeds
    let embed = embed_stats.expect("embed_stats should be Some when not --fast");
    eprintln!(
        "embed succeeded={}, failed={}, processed={}, diagnostics={:?}",
        embed.succeeded, embed.failed, embed.processed, embed.diagnostics
    );
    assert_eq!(embed.succeeded, 2, "both notes should be embedded");
    assert_eq!(embed.failed, 0);

    // vec_chunks should have entries
    let chunk_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
        .unwrap();
    eprintln!("vec_chunks count: {chunk_count}");
    assert!(chunk_count > 0, "vec_chunks should have embeddings");

    let active_notes: i64 = conn
        .query_row("SELECT COUNT(*) FROM notes WHERE active = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(active_notes, 2);

    drop(conn);
    cleanup(&vault);
}

#[test]
fn sync_fast_skips_embed_pass() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("sync-fast");
    seed_vault(&vault);
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    let mut conn = open_database(&db).unwrap();
    let config = IndexerConfig::index_all();

    // No embed config, no inference client — simulates --fast
    let (stats, embed_stats) = run_sync(&mut conn, &vault, &lock, &config, None, None).unwrap();

    // Full scan indexes both notes
    assert_eq!(stats.indexed, 2);

    // No embed pass ran
    assert!(
        embed_stats.is_none(),
        "embed_stats should be None in fast mode"
    );

    // vec_chunks should not exist (no embed pass ran, so ensure_vec_chunks was never called)
    let vec_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'vec_chunks'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        !vec_exists,
        "vec_chunks table should not exist in fast mode"
    );

    drop(conn);
    cleanup(&vault);
}

#[test]
fn sync_embed_http_error_marks_chunks_failed() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("sync-embed-err");
    seed_vault(&vault);
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let server = runtime.block_on(MockServer::start());
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .respond_with(ResponseTemplate::new(500).set_body_string("sidecar OOM"))
            .mount(&server),
    );

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let opts = EmbedPassOptions::defaults();
    let config = IndexerConfig::index_all();

    let (stats, embed_stats) =
        run_sync(&mut conn, &vault, &lock, &config, Some(opts), Some(&client)).unwrap();

    assert_eq!(stats.indexed, 2);

    let embed = embed_stats.expect("embed_stats should be Some");
    assert_eq!(embed.failed, 2, "both notes should fail embedding");
    assert_eq!(embed.succeeded, 0);
    assert!(!embed.diagnostics.is_empty(), "should have diagnostics");

    // Chunks should be marked failed
    let failed_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE embedding_status = 'failed'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(failed_count, 2);

    drop(conn);
    cleanup(&vault);
}
