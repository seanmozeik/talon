use super::super::search_hybrid::infer_hybrid_match_kind;
use super::*;
use crate::config::{
    ChunkerConfig, ExpansionConfig, InferenceConfig, InferenceModels, LintConfig, RerankConfig,
    Scope, ScopeGlob, ScopePriority, ScopesConfig, SearchConfig,
};
use crate::search::types::SearchScores;
use crate::store::open_database;
use rusqlite::{Connection, params};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_path() -> std::path::PathBuf {
    static C: AtomicU64 = AtomicU64::new(0);
    let n = C.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "talon-search-query-test-{}-{n}.sqlite",
        std::process::id()
    ))
}

fn cleanup(path: &std::path::Path) {
    let _ = fs_err::remove_file(path);
    let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
    let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < f64::EPSILON,
        "expected {expected}, got {actual}"
    );
}

fn insert_note_with_content(conn: &Connection, vault_path: &str, content: &str) -> i64 {
    assert!(
        conn.execute(
            "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active) VALUES (?, ?, '[]', '[]', ?, 0, 0, 'h', 'd', 1)",
            params![vault_path, "Title", content],
        )
        .is_ok(),
        "failed to insert test note"
    );
    conn.last_insert_rowid()
}

fn raw(path: &str, snippet: &str, bm25: bool, semantic_heading: Option<&str>) -> RawSearchResult {
    RawSearchResult {
        path: path.into(),
        title: "Title".into(),
        tags: vec![],
        aliases: vec![],
        snippet: snippet.into(),
        score: 0.9,
        scores: SearchScores {
            bm25: if bm25 { Some(0.9) } else { None },
            semantic: semantic_heading.map(|_| 0.8),
            ..Default::default()
        },
        semantic_heading: semantic_heading.map(ToOwned::to_owned),
        semantic_char_start: semantic_heading.map(|_| 10),
        semantic_char_end: semantic_heading.map(|_| 20),
    }
}

fn config_with_wiki_scope() -> TalonConfig {
    let mut scopes = ScopesConfig::new();
    scopes.insert(
        "wiki".to_string(),
        Scope {
            glob: ScopeGlob::Single("wiki/**".to_string()),
            priority: ScopePriority::Boosted,
            default: true,
            lint: true,
        },
    );

    TalonConfig {
        vault_path: PathBuf::from("/tmp/vault"),
        db_path: PathBuf::from("/tmp/vault/idx.sqlite"),
        config_file_path: None,
        include_patterns: Vec::new(),
        ignore_patterns: Vec::new(),
        inference: InferenceConfig {
            base_url: "http://localhost".to_string(),
            models: InferenceModels {
                query_embedding: "query".to_string(),
                document_embedding: "document".to_string(),
                chunk_embedding: "chunk".to_string(),
                reranker: "reranker".to_string(),
            },
            rerank: RerankConfig::default(),
        },
        expansion: ExpansionConfig {
            provider: "openai-compatible".to_string(),
            base_url: "http://localhost".to_string(),
            model: "expansion".to_string(),
            max_tokens: None,
        },
        scopes,
        search: SearchConfig::default(),
        lint: LintConfig::default(),
        chunker: ChunkerConfig::default(),
    }
}

#[test]
fn raw_to_search_result_uses_body_fallback_for_short_bm25_snippets() {
    let path = unique_path();
    let conn = match open_database(&path) {
        Ok(conn) => conn,
        Err(err) => panic!("failed to open temp db: {err}"),
    };
    insert_note_with_content(
        &conn,
        "notes/fallback.md",
        "## Intro\n\nA longer body excerpt with alpha in the middle gives the fallback query more context than the short BM25 snippet.",
    );

    let raw = raw("notes/fallback.md", "short alpha", true, None);
    let result = raw_to_search_result(
        &raw,
        SearchMode::Fulltext,
        &conn,
        false,
        "alpha",
        raw.score,
        None,
    )
    .unwrap_or_else(|| panic!("raw result should convert"));

    assert!(
        result.snippet.len() > raw.snippet.len(),
        "fallback retrieval should replace the short BM25 snippet"
    );
    assert!(
        result.snippet.contains("longer body excerpt"),
        "fallback retrieval should surface body text"
    );
    drop(conn);
    cleanup(&path);
}

#[test]
fn apply_scope_priority_preserves_pre_multiplier_raw_score() {
    let config = config_with_wiki_scope();
    let raw = raw("wiki/shout.md", "shout", false, None);
    let scored = apply_scope_priority(vec![raw], Some(&config), &[]);

    assert_eq!(scored.len(), 1);
    assert_close(scored[0].raw_score, 0.9);
    assert_close(scored[0].raw.score, 1.08);
}

#[test]
fn apply_scope_priority_gates_low_relevance_positive_boosts() {
    let config = config_with_wiki_scope();
    let mut raw = raw("wiki/shout.md", "shout", false, None);
    raw.score = 0.39;
    let scored = apply_scope_priority(vec![raw], Some(&config), &[]);

    assert_eq!(scored.len(), 1);
    assert_close(scored[0].raw_score, 0.39);
    assert_close(scored[0].raw.score, 0.39);
}

#[test]
fn additive_scope_request_neutralizes_requested_scope_penalty() {
    let mut config = config_with_wiki_scope();
    config.scopes.insert(
        "raw".to_string(),
        Scope {
            glob: ScopeGlob::Single("raw/**".to_string()),
            priority: ScopePriority::Muted,
            default: false,
            lint: true,
        },
    );
    let mut raw_email = raw("raw/email.md", "quote", false, None);
    raw_email.score = 0.8;

    let scored = apply_scope_priority(vec![raw_email], Some(&config), &["raw".to_string()]);

    assert_eq!(scored[0].raw.path, "raw/email.md");
    assert_close(scored[0].raw_score, 0.8);
    assert_close(scored[0].raw.score, 0.8);
}

#[test]
fn raw_to_search_result_uses_supplied_pre_multiplier_raw_score() {
    let path = unique_path();
    let conn = match open_database(&path) {
        Ok(conn) => conn,
        Err(err) => panic!("failed to open temp db: {err}"),
    };
    let mut raw = raw("wiki/shout.md", "shout", false, None);
    raw.score = 0.9;

    let result = raw_to_search_result(
        &raw,
        SearchMode::Semantic,
        &conn,
        false,
        "",
        0.9,
        Some(&config_with_wiki_scope()),
    )
    .unwrap_or_else(|| panic!("raw result should convert"));

    assert_close(result.score, 0.9);
    assert_eq!(result.raw_score, Some(0.9));
    assert_eq!(result.scope.as_deref(), Some("wiki"));
    drop(conn);
    cleanup(&path);
}

#[test]
fn infer_hybrid_match_kind_picks_dominant_signal() {
    let title_dominant = SearchScores {
        bm25: Some(0.4),
        fuzzy_title: Some(0.9),
        semantic: Some(0.5),
        ..Default::default()
    };
    assert_eq!(
        infer_hybrid_match_kind(&title_dominant),
        MatchKind::Title,
        "highest title contribution should win"
    );

    let semantic_over_bm25 = SearchScores {
        bm25: Some(0.3),
        semantic: Some(0.7),
        ..Default::default()
    };
    assert_eq!(
        infer_hybrid_match_kind(&semantic_over_bm25),
        MatchKind::Semantic,
        "stronger semantic should beat weaker bm25"
    );

    let bm25_over_semantic = SearchScores {
        bm25: Some(0.8),
        semantic: Some(0.2),
        ..Default::default()
    };
    assert_eq!(
        infer_hybrid_match_kind(&bm25_over_semantic),
        MatchKind::Fulltext,
        "stronger bm25 should beat weaker semantic"
    );

    let semantic_only = SearchScores {
        semantic: Some(0.4),
        ..Default::default()
    };
    assert_eq!(
        infer_hybrid_match_kind(&semantic_only),
        MatchKind::Semantic,
        "semantic-only candidate should map to Semantic"
    );

    let bm25_only = SearchScores {
        bm25: Some(0.4),
        ..Default::default()
    };
    assert_eq!(
        infer_hybrid_match_kind(&bm25_only),
        MatchKind::Fulltext,
        "bm25-only candidate should map to Fulltext"
    );

    let weak_title = SearchScores {
        bm25: Some(0.8),
        fuzzy_title: Some(0.1),
        semantic: None,
        ..Default::default()
    };
    assert_eq!(
        infer_hybrid_match_kind(&weak_title),
        MatchKind::Fulltext,
        "weak title should not steal credit from a dominant body signal"
    );

    let tied_with_bm25 = SearchScores {
        bm25: Some(0.5),
        semantic: Some(0.5),
        ..Default::default()
    };
    assert_eq!(
        infer_hybrid_match_kind(&tied_with_bm25),
        MatchKind::Fulltext,
        "exact tie keeps Fulltext (stable for deterministic ranking)"
    );

    assert_eq!(
        infer_hybrid_match_kind(&SearchScores::default()),
        MatchKind::Fulltext,
        "empty breakdown falls back to Fulltext"
    );
}

#[test]
fn raw_to_search_result_truncates_on_char_boundaries() {
    let path = unique_path();
    let conn = match open_database(&path) {
        Ok(conn) => conn,
        Err(err) => panic!("failed to open temp db: {err}"),
    };
    let raw = raw(
        "notes/emoji.md",
        &format!("{}🙂", "a".repeat(DEFAULT_SNIPPET_LENGTH as usize)),
        false,
        Some("Heading"),
    );

    let result = raw_to_search_result(
        &raw,
        SearchMode::Semantic,
        &conn,
        false,
        "",
        raw.score,
        None,
    )
    .unwrap_or_else(|| panic!("raw result should convert"));

    assert_eq!(
        result.snippet.chars().count(),
        DEFAULT_SNIPPET_LENGTH as usize
    );
    assert!(result.snippet.starts_with("Heading\n"));
    drop(conn);
    cleanup(&path);
}
