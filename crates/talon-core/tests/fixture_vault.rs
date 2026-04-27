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
    AnchorKind, ChunkerConfig, Direction, LintCheck, LintInput, MetaInput, PositiveCount,
    RecallInput, RelatedInput, SearchInput, SearchMode, WhereClause, WhereOperator,
    config::{ExpansionConfig, InferenceConfig, InferenceModels, ScopesConfig, TalonConfig},
    embed::EmbedPassOptions,
    indexer::IndexerConfig,
    inference::InferenceClient,
    open_database, query_lint, query_meta, query_status, run_recall, run_search,
    run_sync_with_chunker,
    vec_ext::register_sqlite_vec,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

#[path = "fixture_vault/anchors.rs"]
mod anchors;
#[path = "fixture_vault/hybrid.rs"]
mod hybrid;
#[path = "fixture_vault/query.rs"]
mod query;
#[path = "fixture_vault/recall.rs"]
mod recall;
#[path = "fixture_vault/search.rs"]
mod search;
#[path = "fixture_vault/sync.rs"]
mod sync;

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

fn fixture_chunker() -> ChunkerConfig {
    ChunkerConfig {
        chunk_min_tokens: 1,
        ..ChunkerConfig::default()
    }
}

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

fn embed_response_5d() -> serde_json::Value {
    json!([[0.1_f32, 0.2_f32, 0.3_f32, 0.4_f32, 0.5_f32]])
}

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

fn minimal_config(vault: &std::path::Path) -> TalonConfig {
    TalonConfig {
        vault_path: vault.to_path_buf(),
        db_path: vault.join("idx.sqlite"),
        config_file_path: None,
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
            max_tokens: None,
        },
        scopes: ScopesConfig::default(),
        chunker: talon_core::ChunkerConfig::default(),
    }
}
