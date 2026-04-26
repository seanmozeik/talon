//! Integration test: 21-note fixture vault exercises the full query layer.
//!
//! Ports the fixture vault from the TS reference and exercises search (fulltext,
//! title, hybrid), related-graph traversal, meta --where filtering, lint orphan
//! detection, and status counts end-to-end with a mocked sidecar.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::json;
use std::env::temp_dir;
use std::sync::atomic::{AtomicU64, Ordering};
use talon_core::{
    Direction, LintCheck, LintInput, MetaInput, PositiveCount, RelatedInput, SearchInput,
    SearchMode, WhereClause, WhereOperator,
    config::{ExpansionConfig, InferenceConfig, InferenceModels, ScopesConfig, TalonConfig},
    embed::EmbedPassOptions,
    indexer::IndexerConfig,
    inference::InferenceClient,
    open_database, query_lint, query_meta, query_status, run_search, run_sync,
    vec_ext::register_sqlite_vec,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

// ── Unique temp path per test ──────────────────────────────────────────────

fn unique_path(label: &str) -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    temp_dir().join(format!("talon-fixture-vault-{label}-{pid}-{n}"))
}

fn cleanup(p: &std::path::Path) {
    let _ = fs_err::remove_file(p.join("idx.sqlite"));
    let _ = fs_err::remove_file(p.join("idx.sqlite-wal"));
    let _ = fs_err::remove_file(p.join("idx.sqlite-shm"));
    let _ = fs_err::remove_dir_all(p);
}

// ── Copy fixture vault to a temp directory ─────────────────────────────────

fn seed_fixture_vault(vault: &std::path::Path) {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault");
    copy_dir_all(&fixtures, vault);
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) {
    fs_err::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let ty = entry.file_type().unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&from, &to);
        } else {
            fs_err::copy(&from, &to).unwrap();
        }
    }
}

// ── 5-dimensional mock embedding helpers ──────────────────────────────────

fn embed_response_5d() -> serde_json::Value {
    json!([[0.1_f32, 0.2_f32, 0.3_f32, 0.4_f32, 0.5_f32]])
}

/// Dynamic responder for `/embed-chunked` — returns one 5-dim vector per
/// chunk in the request so `persist_multi_chunk` length checks pass.
///
/// All notes with YAML frontmatter produce 2+ chunks (the frontmatter text
/// becomes a pre-heading section), so they use this endpoint rather than
/// the simpler `/embed` path.
struct EmbedChunkedResponder;

impl Respond for EmbedChunkedResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value =
            serde_json::from_slice(&request.body).unwrap_or_else(|_| json!({"input": [[]]}));
        let groups = body
            .get("input")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let data: Vec<serde_json::Value> = groups
            .iter()
            .enumerate()
            .map(|(i, group)| {
                let n = group.as_array().map_or(1, Vec::len).max(1);
                let embeddings: Vec<Vec<f32>> = (0..n)
                    .map(|_| vec![0.1_f32, 0.2_f32, 0.3_f32, 0.4_f32, 0.5_f32])
                    .collect();
                json!({"embeddings": embeddings, "index": i})
            })
            .collect();
        ResponseTemplate::new(200).set_body_json(json!({"data": data, "model": "embed_chunked"}))
    }
}

// ── Minimal TalonConfig for status queries ─────────────────────────────────

fn minimal_config(vault: &std::path::Path) -> TalonConfig {
    TalonConfig {
        vault_path: vault.to_path_buf(),
        db_path: vault.join("idx.sqlite"),
        include_patterns: Vec::new(),
        ignore_patterns: Vec::new(),
        inference: InferenceConfig {
            base_url: "http://localhost:1".to_string(),
            models: InferenceModels {
                query_embedding: "embed".to_string(),
                document_embedding: "embed".to_string(),
                chunk_embedding: "embed".to_string(),
                reranker: "rerank".to_string(),
            },
        },
        expansion: ExpansionConfig {
            provider: "openai-compatible".to_string(),
            base_url: "http://localhost:1".to_string(),
            model: "test".to_string(),
        },
        scopes: ScopesConfig::default(),
    }
}

// ── Test 1: sync indexes all 21 notes ─────────────────────────────────────

#[test]
fn fixture_vault_sync_indexes_all_notes() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("sync");
    seed_fixture_vault(&vault);
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
            .respond_with(ResponseTemplate::new(200).set_body_json(embed_response_5d()))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(EmbedChunkedResponder)
            .mount(&server),
    );

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let config = IndexerConfig::index_all();

    let (stats, embed_stats) = run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();

    assert_eq!(stats.indexed, 21, "all 21 fixture notes must be indexed");
    assert_eq!(stats.deleted, 0);

    let embed = embed_stats.expect("embed pass must run when not --fast");
    assert_eq!(embed.succeeded, 21, "all notes must embed successfully");
    assert_eq!(embed.failed, 0);

    let active: i64 = conn
        .query_row("SELECT COUNT(*) FROM notes WHERE active = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(active, 21);

    let vec_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
        .unwrap();
    assert!(
        vec_count >= 21,
        "vec_chunks must have at least one row per note"
    );

    drop(conn);
    cleanup(&vault);
}

// ── Test 2: fulltext search "orchard" returns Fruit Orchard first ──────────

#[test]
fn fixture_vault_fulltext_search_orchard() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("fulltext");
    seed_fixture_vault(&vault);
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
            .respond_with(ResponseTemplate::new(200).set_body_json(embed_response_5d()))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(EmbedChunkedResponder)
            .mount(&server),
    );

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let config = IndexerConfig::index_all();

    run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();

    let input = SearchInput {
        query: Some("orchard".to_string()),
        queries: Vec::new(),
        mode: SearchMode::Fulltext,
        fast: true,
        limit: PositiveCount::new(10, "limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        where_: Vec::new(),
        since: None,
    };

    let response = run_search(&conn, &input, None, None, None);
    assert!(
        !response.results.is_empty(),
        "fulltext 'orchard' must return results"
    );

    let first = response.results[0].vault_path.as_str();
    assert_eq!(
        first, "Search/Fruit Orchard.md",
        "Fruit Orchard must rank first for 'orchard'"
    );

    drop(conn);
    cleanup(&vault);
}

// ── Test 3: fulltext search "banana" returns Banana Grove first ────────────

#[test]
fn fixture_vault_fulltext_search_banana() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("banana");
    seed_fixture_vault(&vault);
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
            .respond_with(ResponseTemplate::new(200).set_body_json(embed_response_5d()))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(EmbedChunkedResponder)
            .mount(&server),
    );

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let config = IndexerConfig::index_all();

    run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();

    let input = SearchInput {
        query: Some("banana grove".to_string()),
        queries: Vec::new(),
        mode: SearchMode::Fulltext,
        fast: true,
        limit: PositiveCount::new(10, "limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        where_: Vec::new(),
        since: None,
    };

    let response = run_search(&conn, &input, None, None, None);
    assert!(
        !response.results.is_empty(),
        "fulltext 'banana grove' must return results"
    );

    assert!(
        response
            .results
            .iter()
            .any(|r| r.vault_path.as_str() == "Search/Banana Grove.md"),
        "Banana Grove must appear in results"
    );

    drop(conn);
    cleanup(&vault);
}

// ── Test 4: title search finds note by alias ───────────────────────────────

#[test]
fn fixture_vault_title_search_cafe_alias() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("title");
    seed_fixture_vault(&vault);
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
            .respond_with(ResponseTemplate::new(200).set_body_json(embed_response_5d()))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(EmbedChunkedResponder)
            .mount(&server),
    );

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let config = IndexerConfig::index_all();

    run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();

    let input = SearchInput {
        query: Some("Cafe del Sol".to_string()),
        queries: Vec::new(),
        mode: SearchMode::Title,
        fast: true,
        limit: PositiveCount::new(10, "limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        where_: Vec::new(),
        since: None,
    };

    let response = run_search(&conn, &input, None, None, None);
    assert!(
        !response.results.is_empty(),
        "title search 'Cafe del Sol' must return results"
    );

    let first = response.results[0].vault_path.as_str();
    assert_eq!(
        first, "Search/Cafe Note.md",
        "Cafe Note must rank first for alias 'Cafe del Sol'"
    );

    drop(conn);
    cleanup(&vault);
}

// ── Test 5: hybrid search returns multiple relevant results ────────────────

#[test]
fn fixture_vault_hybrid_search_returns_results() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("hybrid");
    seed_fixture_vault(&vault);
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
            .respond_with(ResponseTemplate::new(200).set_body_json(embed_response_5d()))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(EmbedChunkedResponder)
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": "{\"queries\":[\"orchard\",\"banana grove\"]}"}}]
            })))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/rerank"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"index": 0, "score": 0.9}
            ])))
            .mount(&server),
    );

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let expansion = talon_core::ExpansionClient::new(server.uri(), "test-model").unwrap();
    let config = IndexerConfig::index_all();

    run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();

    let input = SearchInput {
        query: Some("fruit harvest".to_string()),
        queries: Vec::new(),
        mode: SearchMode::Hybrid,
        fast: false,
        limit: PositiveCount::new(10, "limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        where_: Vec::new(),
        since: None,
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
            .any(|r| r.vault_path.as_str() == "Search/Fruit Orchard.md"),
        "Fruit Orchard must appear in hybrid search for 'fruit harvest'"
    );

    drop(conn);
    cleanup(&vault);
}

// ── Test 6: related graph traversal from Hub at depth=2 ───────────────────

#[test]
fn fixture_vault_related_hub_depth2() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("related");
    seed_fixture_vault(&vault);
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
            .respond_with(ResponseTemplate::new(200).set_body_json(embed_response_5d()))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(EmbedChunkedResponder)
            .mount(&server),
    );

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let config = IndexerConfig::index_all();

    run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();

    let input = RelatedInput {
        path: "Graph/Hub.md".to_string(),
        depth: 2,
        direction: Direction::Outgoing,
        scope: Vec::new(),
        scope_only: Vec::new(),
    };

    let response = talon_core::find_related(&conn, &input);

    let paths: Vec<&str> = response
        .results
        .iter()
        .map(|r| r.vault_path.as_str())
        .collect();

    // Depth 1: direct links from Hub
    assert!(paths.contains(&"Graph/Child.md"), "Hub must link to Child");
    assert!(paths.contains(&"Graph/Side.md"), "Hub must link to Side");
    assert!(
        paths.contains(&"Graph/Inbound.md"),
        "Hub must link to Inbound"
    );
    // Depth 2: transitive via Child
    assert!(
        paths.contains(&"Graph/Grandchild.md"),
        "Hub depth=2 must reach Grandchild via Child"
    );
    // Source itself must not appear
    assert!(
        !paths.contains(&"Graph/Hub.md"),
        "Hub must not appear in its own related results"
    );

    drop(conn);
    cleanup(&vault);
}

// ── Test 7: meta --where filters by frontmatter ────────────────────────────

#[test]
fn fixture_vault_meta_where_archived() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("meta");
    seed_fixture_vault(&vault);
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
            .respond_with(ResponseTemplate::new(200).set_body_json(embed_response_5d()))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(EmbedChunkedResponder)
            .mount(&server),
    );

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let config = IndexerConfig::index_all();

    run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();

    let input = MetaInput {
        where_: vec![WhereClause {
            key: "status".to_string(),
            op: WhereOperator::Equals,
            value: Some("archived".to_string()),
        }],
        since: None,
        scope: Vec::new(),
        scope_only: Vec::new(),
        select: Vec::new(),
        tag_counts: false,
        sources: None,
        limit: PositiveCount::new(50, "limit").unwrap(),
    };

    let response = query_meta(&conn, &input);

    assert_eq!(
        response.entries.len(),
        1,
        "exactly one note has status=archived"
    );
    assert_eq!(
        response.entries[0].path.as_str(),
        "Filters/Frontmatter.md",
        "only Frontmatter note has status=archived"
    );

    drop(conn);
    cleanup(&vault);
}

// ── Test 8: lint orphans finds notes with no incoming links ────────────────

#[test]
fn fixture_vault_lint_orphans() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("lint");
    seed_fixture_vault(&vault);
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
            .respond_with(ResponseTemplate::new(200).set_body_json(embed_response_5d()))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(EmbedChunkedResponder)
            .mount(&server),
    );

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let config = IndexerConfig::index_all();

    run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();

    let input = LintInput {
        check: LintCheck::Orphans,
        scope: Vec::new(),
        scope_only: Vec::new(),
    };

    let response = query_lint(&conn, &input);

    assert!(
        !response.findings.is_empty(),
        "orphan check must find at least one orphan"
    );

    let orphan_paths: Vec<&str> = response.findings.iter().map(|f| f.path.as_str()).collect();

    // Notes with no incoming links in the fixture vault
    assert!(
        orphan_paths.contains(&"Search/Banana Grove.md"),
        "Banana Grove has no incoming links and must be an orphan"
    );
    // Graph/Child is linked from Hub and Beta — must NOT be an orphan
    assert!(
        !orphan_paths.contains(&"Graph/Child.md"),
        "Graph/Child has incoming links and must not be an orphan"
    );
    // Graph/Grandchild is linked from Graph/Child — must NOT be an orphan
    assert!(
        !orphan_paths.contains(&"Graph/Grandchild.md"),
        "Graph/Grandchild has incoming links and must not be an orphan"
    );

    drop(conn);
    cleanup(&vault);
}

// ── Test 9: status returns correct active note count ──────────────────────

#[test]
fn fixture_vault_status_counts() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("status");
    seed_fixture_vault(&vault);
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
            .respond_with(ResponseTemplate::new(200).set_body_json(embed_response_5d()))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(EmbedChunkedResponder)
            .mount(&server),
    );

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let config = IndexerConfig::index_all();

    run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();

    let talon_config = minimal_config(&vault);
    let response = query_status(&conn, &talon_config);

    assert_eq!(
        response.index.active_notes, 21,
        "status must report 21 active notes"
    );
    assert!(
        response.index.chunk_count >= 21,
        "at least one chunk per note"
    );
    assert_eq!(response.index.failed_embeddings, 0, "no embedding failures");
    assert_eq!(
        response.index.vector_dimensions,
        Some(5),
        "vector dimensions must match the 5-dim mock"
    );

    drop(conn);
    cleanup(&vault);
}
