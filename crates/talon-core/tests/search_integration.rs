//! Integration test: `talon search` returns real ranked results.
//!
//! Seeds a temp vault, runs `talon sync` (with mocked sidecar), then exercises
//! search modes and filters against real indexed data.

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

#[path = "search_integration/filters.rs"]
mod filters;
#[path = "search_integration/modes.rs"]
mod modes;

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
