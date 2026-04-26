//! Integration test: `talon search` returns real ranked results.
//!
//! Seeds a temp vault, runs `talon sync` (with mocked sidecar), then exercises
//! all four search modes and verifies results against expectations.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::json;
use std::env::temp_dir;
use std::sync::atomic::{AtomicU64, Ordering};
use talon_core::{
    SearchInput, SearchMode, WhereClause, WhereOperator, embed::EmbedPassOptions,
    indexer::IndexerConfig, inference::InferenceClient, open_database, run_search, run_sync,
    vec_ext::register_sqlite_vec,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn unique_path(label: &str) -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    temp_dir().join(format!("talon-search-integration-{label}-{pid}-{n}"))
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
        vault.join("zettelkasten.md"),
        "# Zettelkasten Method\n\nAtomic notes for thinking and learning.\n\nThe Zettelkasten method is a personal knowledge management system.",
    )
    .unwrap();
    fs_err::write(
        vault.join("spaced-repetition.md"),
        "# Spaced Repetition\n\nSpaced repetition system for memory retention.\n\nUse flashcards and review intervals.",
    )
    .unwrap();
    fs_err::write(
        vault.join("atomic-notes.md"),
        "# Atomic Notes\n\nSmall, focused notes that link together.\n\nEach note should be self-contained.",
    )
    .unwrap();
}

fn dummy_embed_response() -> serde_json::Value {
    json!([[0.1_f32, 0.2_f32, 0.3_f32]])
}

// ── Test 1: hybrid mode returns results ────────────────────────────────────

#[test]
fn search_hybrid_mode_returns_results() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("search-hybrid");
    seed_vault(&vault);
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

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
                        "content": "{\"queries\":[\"atomic notes\",\"knowledge management\"]}"
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

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let expansion = talon_core::ExpansionClient::new(server.uri(), "test-model").unwrap();
    let config = IndexerConfig::index_all();

    let (stats, _) = run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();
    assert_eq!(stats.indexed, 3);

    let input = SearchInput {
        query: Some("atomic notes".to_string()),
        queries: Vec::new(),
        mode: SearchMode::Hybrid,
        fast: false,
        limit: talon_core::PositiveCount::new(10, "limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: talon_core::Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        where_: Vec::new(),
        since: None,
        anchors: None,
    };

    let response = run_search(&conn, &input, Some(&client), Some(&expansion), None);

    assert!(
        !response.results.is_empty(),
        "hybrid search must return results"
    );
    assert!(
        response
            .results
            .iter()
            .any(|r| r.vault_path.as_str() == "atomic-notes.md"),
        "atomic-notes.md must appear in results"
    );

    drop(conn);
    cleanup(&vault);
}

// ── Test 2: fulltext mode returns results ──────────────────────────────────

#[test]
fn search_fulltext_mode_returns_results() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("search-fulltext");
    seed_vault(&vault);
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let server = rt.block_on(MockServer::start());
    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let config = IndexerConfig::index_all();

    let (stats, _) = run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();
    assert_eq!(stats.indexed, 3);

    let input = SearchInput {
        query: Some("spaced repetition".to_string()),
        queries: Vec::new(),
        mode: SearchMode::Fulltext,
        fast: true,
        limit: talon_core::PositiveCount::new(10, "limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: talon_core::Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        where_: Vec::new(),
        since: None,
        anchors: None,
    };

    let response = run_search(&conn, &input, None, None, None);

    assert!(
        !response.results.is_empty(),
        "fulltext search must return results"
    );
    assert!(
        response
            .results
            .iter()
            .any(|r| r.vault_path.as_str() == "spaced-repetition.md"),
        "spaced-repetition.md must appear in results"
    );

    drop(conn);
    cleanup(&vault);
}

// ── Test 3: title mode returns results ─────────────────────────────────────

#[test]
fn search_title_mode_returns_results() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("search-title");
    seed_vault(&vault);
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let server = rt.block_on(MockServer::start());
    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let config = IndexerConfig::index_all();

    let (stats, _) = run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();
    assert_eq!(stats.indexed, 3);

    let input = SearchInput {
        query: Some("zettelkasten".to_string()),
        queries: Vec::new(),
        mode: SearchMode::Title,
        fast: true,
        limit: talon_core::PositiveCount::new(10, "limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: talon_core::Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        where_: Vec::new(),
        since: None,
        anchors: None,
    };

    let response = run_search(&conn, &input, None, None, None);

    assert!(
        !response.results.is_empty(),
        "title search must return results"
    );
    assert!(
        response
            .results
            .iter()
            .any(|r| r.vault_path.as_str() == "zettelkasten.md"),
        "zettelkasten.md must appear in results"
    );

    drop(conn);
    cleanup(&vault);
}

// ── Test 4: where filter works ─────────────────────────────────────────────

#[test]
fn search_where_filter_applies() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("search-where");
    seed_vault(&vault);
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let server = rt.block_on(MockServer::start());
    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let config = IndexerConfig::index_all();

    let (stats, _) = run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();
    assert_eq!(stats.indexed, 3);

    // Search for all notes, then filter by where clause.
    let where_clause = WhereClause {
        key: "type".to_string(),
        op: WhereOperator::Equals,
        value: Some("method".to_string()),
    };

    let input = SearchInput {
        query: Some("notes".to_string()),
        queries: Vec::new(),
        mode: SearchMode::Fulltext,
        fast: true,
        limit: talon_core::PositiveCount::new(10, "limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: talon_core::Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        where_: vec![where_clause],
        since: None,
        anchors: None,
    };

    let response = run_search(&conn, &input, None, None, None);

    // No notes have frontmatter field "type" = "method", so results should be empty.
    assert!(
        response.results.is_empty(),
        "where filter should exclude all results when no match"
    );

    drop(conn);
    cleanup(&vault);
}

// ── Test 5: since filter works ─────────────────────────────────────────────

#[test]
fn search_since_filter_applies() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("search-since");
    seed_vault(&vault);
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let server = rt.block_on(MockServer::start());
    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let config = IndexerConfig::index_all();

    let (stats, _) = run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();
    assert_eq!(stats.indexed, 3);

    // Use a far-future timestamp — no notes should match.
    let input = SearchInput {
        query: Some("notes".to_string()),
        queries: Vec::new(),
        mode: SearchMode::Fulltext,
        fast: true,
        limit: talon_core::PositiveCount::new(10, "limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: talon_core::Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        where_: Vec::new(),
        since: Some("9999999999999".to_string()), // far future
        anchors: None,
    };

    let response = run_search(&conn, &input, None, None, None);

    assert!(
        response.results.is_empty(),
        "since filter with far-future timestamp should return no results"
    );

    drop(conn);
    cleanup(&vault);
}

// ── Test 6: empty query returns empty response ─────────────────────────────

#[test]
fn search_empty_query_returns_empty() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("search-empty");
    seed_vault(&vault);
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let server = rt.block_on(MockServer::start());
    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let config = IndexerConfig::index_all();

    let (stats, _) = run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();
    assert_eq!(stats.indexed, 3);

    let input = SearchInput {
        query: Some(String::new()),
        queries: Vec::new(),
        mode: SearchMode::Hybrid,
        fast: false,
        limit: talon_core::PositiveCount::new(10, "limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: talon_core::Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        where_: Vec::new(),
        since: None,
        anchors: None,
    };

    let response = run_search(&conn, &input, Some(&client), None, None);

    assert!(response.results.is_empty());
    assert_eq!(response.total, 0);

    drop(conn);
    cleanup(&vault);
}
