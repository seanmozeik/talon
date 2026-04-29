use super::*;
use crate::search::SearchMode;
use crate::search::types::RawSearchResult;
use crate::search::types::SearchScores;
use crate::store::open_database;
use rusqlite::{Connection, params};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_path() -> PathBuf {
    static C: AtomicU64 = AtomicU64::new(0);
    let n = C.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "talon-search-affordance-test-{}-{n}.sqlite",
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
            "INSERT INTO notes
         (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
         VALUES (?, 'Title', '[]', '[]', ?, 0, 0, 'h', 'd', 1)",
            params![vault_path, content],
        )
        .is_ok(),
        "failed to insert test note"
    );
    conn.last_insert_rowid()
}

fn insert_fm_field(conn: &Connection, note_id: i64, field: &str, value: &str) {
    assert!(
        conn.execute(
            "INSERT INTO note_frontmatter_fields
         (note_id, field, value, value_norm) VALUES (?, ?, ?, ?)",
            params![note_id, field, value, value.to_lowercase()],
        )
        .is_ok(),
        "failed to insert test frontmatter field"
    );
}

fn raw(path: &str) -> RawSearchResult {
    RawSearchResult {
        path: path.into(),
        title: "Title".into(),
        tags: vec![],
        aliases: vec![],
        snippet: "index".into(),
        score: 0.9,
        scores: SearchScores::default(),
        semantic_heading: None,
        semantic_char_start: None,
        semantic_char_end: None,
    }
}

#[test]
fn raw_to_search_result_exposes_wiki_navigation_metadata() {
    let path = unique_path();
    let conn = match open_database(&path) {
        Ok(conn) => conn,
        Err(err) => panic!("failed to open temp db: {err}"),
    };
    let note_id = insert_note_with_content(&conn, "wiki/index.md", "Index body");
    insert_note_with_content(&conn, "raw/source.md", "Source body");
    insert_fm_field(&conn, note_id, "sources", "[[Source|source note]]");
    insert_fm_field(&conn, note_id, "sources", "https://example.com/source");
    assert!(
        conn.execute(
            "INSERT INTO links (from_path, to_path, raw_target, alias) VALUES (?, ?, ?, ?)",
            params!["wiki/index.md", "raw/source.md", "Source", "source note"],
        )
        .is_ok(),
        "failed to insert source link"
    );
    assert!(
        conn.execute(
            "INSERT INTO links (from_path, to_path, raw_target) VALUES (?, ?, ?)",
            params!["wiki/topic.md", "wiki/index.md", "wiki/index.md"],
        )
        .is_ok(),
        "failed to insert test link"
    );

    let raw = raw("wiki/index.md");
    let result = raw_to_search_result(
        &raw,
        SearchMode::Semantic,
        &conn,
        false,
        "index",
        raw.score,
        None,
    )
    .unwrap_or_else(|| panic!("raw result should convert"));

    assert!(result.is_index);
    assert_eq!(
        result.citations,
        vec!["raw/source.md", "https://example.com/source"]
    );
    assert_eq!(result.backlinks, vec!["wiki/topic.md"]);
    drop(conn);
    cleanup(&path);
}
