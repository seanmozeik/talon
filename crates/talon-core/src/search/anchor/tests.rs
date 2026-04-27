#![allow(clippy::unwrap_used)]

use super::*;
use crate::search::types::SearchScores;
use crate::store::open_database;
use rusqlite::params;
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_path() -> std::path::PathBuf {
    static C: AtomicU64 = AtomicU64::new(0);
    let n = C.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "talon-anchor-test-{}-{n}.sqlite",
        std::process::id()
    ))
}

fn cleanup(path: &std::path::Path) {
    let _ = fs_err::remove_file(path);
    let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
    let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
}

fn raw(path: &str, snippet: &str, bm25: bool, sem_heading: Option<&str>) -> RawSearchResult {
    RawSearchResult {
        path: path.into(),
        title: "Test".into(),
        tags: vec![],
        aliases: vec![],
        snippet: snippet.into(),
        score: 0.9,
        scores: SearchScores {
            bm25: if bm25 { Some(0.9) } else { None },
            semantic: if sem_heading.is_some() {
                Some(0.8)
            } else {
                None
            },
            ..Default::default()
        },
        semantic_heading: sem_heading.map(ToOwned::to_owned),
        semantic_char_start: sem_heading.map(|_| 100),
        semantic_char_end: sem_heading.map(|_| 200),
    }
}

fn insert_note_with_content(conn: &Connection, vault_path: &str, content: &str) -> i64 {
    conn.execute(
        "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active) VALUES (?, ?, '[]', '[]', ?, 0, 0, 'h', 'd', 1)",
        params![vault_path, "Title", content],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn insert_chunk(conn: &Connection, note_id: i64, text: &str, heading: &str) {
    conn.execute(
        "INSERT INTO chunks (note_id, chunk_index, text, embedding_text, heading_path, char_start, char_end, line_start, line_end, chunk_hash, token_estimate, embedding_status) VALUES (?, 0, ?, '', ?, 0, 100, 0, 5, 'h', 10, 'pending')",
        params![note_id, text, heading],
    )
    .unwrap();
}

fn insert_chunk_with_position(
    conn: &Connection,
    note_id: i64,
    text: &str,
    heading: Option<&str>,
    char_start: Option<i64>,
) {
    conn.execute(
        "INSERT INTO chunks (note_id, chunk_index, text, embedding_text, heading_path, char_start, char_end, line_start, line_end, chunk_hash, token_estimate, embedding_status) VALUES (?, 0, ?, '', ?, ?, 100, 0, 5, 'h', 10, 'pending')",
        params![note_id, text, heading, char_start],
    )
    .unwrap();
}

#[test]
fn bm25_anchor_resolved_via_strategy1_chunk_lookup() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    let note_id = insert_note_with_content(
        &conn,
        "notes/test.md",
        "## Results\n\nThis is a matching snippet for tests.",
    );
    insert_chunk(
        &conn,
        note_id,
        "This is a matching snippet for tests.",
        "Results",
    );
    let r = raw(
        "notes/test.md",
        "This is a matching snippet for tests.",
        true,
        None,
    );
    let anchors = build_anchors(&conn, &r);
    assert!(!anchors.is_empty());
    let bm25 = anchors.iter().find(|a| a.kind == AnchorKind::Bm25).unwrap();
    assert_eq!(bm25.heading_path.as_deref(), Some("Results"));
    drop(conn);
    cleanup(&path);
}

#[test]
fn semantic_anchor_built_from_chunk_metadata() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    let r = raw(
        "notes/sem.md",
        "semantic chunk text",
        false,
        Some("Methods > Setup"),
    );
    let anchors = build_anchors(&conn, &r);
    let sem = anchors
        .iter()
        .find(|a| a.kind == AnchorKind::Semantic)
        .unwrap();
    assert_eq!(sem.heading_path.as_deref(), Some("Methods > Setup"));
    assert_eq!(sem.char_start, Some(100));
    assert_eq!(sem.char_end, Some(200));
    drop(conn);
    cleanup(&path);
}

#[test]
fn dedup_suppresses_semantic_when_match_text_equals_bm25() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    let note_id =
        insert_note_with_content(&conn, "notes/both.md", "## Intro\n\nshared block text here");
    insert_chunk(&conn, note_id, "shared block text here", "Intro");
    let mut r = raw(
        "notes/both.md",
        "shared block text here",
        true,
        Some("Intro"),
    );
    r.semantic_char_start = Some(10);
    r.semantic_char_end = Some(30);
    let anchors = build_anchors(&conn, &r);
    let bm25_count = anchors
        .iter()
        .filter(|a| a.kind == AnchorKind::Bm25)
        .count();
    let sem_count = anchors
        .iter()
        .filter(|a| a.kind == AnchorKind::Semantic)
        .count();
    assert_eq!(bm25_count, 1);
    assert_eq!(
        sem_count, 0,
        "dedup should remove duplicate semantic anchor"
    );
    drop(conn);
    cleanup(&path);
}

#[test]
fn content_scan_fallback_finds_heading() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    insert_note_with_content(
        &conn,
        "notes/scan.md",
        "# Top Level\n\n## Sub Section\n\nThis fragment is scannable from context.",
    );
    let heading = scan_content_for_heading(
        &conn,
        "notes/scan.md",
        "This fragment is scannable from context.",
    );
    assert!(heading.is_some(), "strategy 2 should find the heading");
    drop(conn);
    cleanup(&path);
}

#[test]
fn resolve_snippet_heading_scans_from_chunk_start_when_heading_path_is_null() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    let content = "# A\n## B\n### C\nbody content for heading fallback";
    let note_id = insert_note_with_content(&conn, "notes/null-heading.md", content);
    let char_start = content.find("body").unwrap();
    insert_chunk_with_position(
        &conn,
        note_id,
        "body content for heading fallback",
        None,
        Some(i64::try_from(char_start).unwrap()),
    );

    let r = raw(
        "notes/null-heading.md",
        "body content for heading fallback",
        true,
        None,
    );
    let heading = resolve_snippet_heading(&conn, &r);

    assert_eq!(heading.as_deref(), Some("A > B > C"));
    drop(conn);
    cleanup(&path);
}

#[test]
fn resolve_snippet_heading_scans_from_chunk_start_for_short_snippet() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    let content = "# A\n## B\n### C\nbody";
    let note_id = insert_note_with_content(&conn, "notes/short-null-heading.md", content);
    let char_start = content.find("body").unwrap();
    insert_chunk_with_position(
        &conn,
        note_id,
        "body",
        None,
        Some(i64::try_from(char_start).unwrap()),
    );

    let r = raw("notes/short-null-heading.md", "body", true, None);
    let heading = resolve_snippet_heading(&conn, &r);

    assert_eq!(heading.as_deref(), Some("A > B > C"));
    drop(conn);
    cleanup(&path);
}

#[test]
fn resolve_snippet_heading_returns_none_when_heading_path_and_char_start_are_null() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    let content = "# A\n## B\n### C\nbody content for missing position";
    let note_id = insert_note_with_content(&conn, "notes/no-start.md", content);
    insert_chunk_with_position(
        &conn,
        note_id,
        "body content for missing position",
        None,
        None,
    );

    let r = raw(
        "notes/no-start.md",
        "body content for missing position",
        true,
        None,
    );
    let heading = resolve_snippet_heading(&conn, &r);

    assert_eq!(heading, None);
    drop(conn);
    cleanup(&path);
}
