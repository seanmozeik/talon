use super::super::search_hybrid::infer_hybrid_match_kind;
use super::*;
use crate::search::types::SearchScores;
use crate::store::open_database;
use rusqlite::{Connection, params};
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
    let result = raw_to_search_result(&raw, SearchMode::Fulltext, &conn, false, "alpha", None)
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

    let result = raw_to_search_result(&raw, SearchMode::Semantic, &conn, false, "", None)
        .unwrap_or_else(|| panic!("raw result should convert"));

    assert_eq!(
        result.snippet.chars().count(),
        DEFAULT_SNIPPET_LENGTH as usize
    );
    assert!(result.snippet.starts_with("Heading\n"));
    drop(conn);
    cleanup(&path);
}
