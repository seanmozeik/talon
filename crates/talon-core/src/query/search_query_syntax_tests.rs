use super::*;
use crate::store::open_database;
use rusqlite::{Connection, params};
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_path() -> std::path::PathBuf {
    static C: AtomicU64 = AtomicU64::new(0);
    let n = C.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "talon-search-query-syntax-test-{}-{n}.sqlite",
        std::process::id()
    ))
}

fn cleanup(path: &std::path::Path) {
    let _ = fs_err::remove_file(path);
    let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
    let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
}

fn insert_note(conn: &Connection, vault_path: &str, content: &str) -> i64 {
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

fn insert_tag(conn: &Connection, note_id: i64, tag: &str) {
    assert!(
        conn.execute(
            "INSERT INTO note_tags (note_id, tag, tag_norm) VALUES (?, ?, ?)",
            params![note_id, tag, crate::text::normalize_keyword(tag)],
        )
        .is_ok(),
        "failed to insert test tag"
    );
}

fn insert_heading_chunk(conn: &Connection, note_id: i64, heading: &str) {
    assert!(
        conn.execute(
            "INSERT INTO chunks
             (note_id, chunk_index, text, embedding_text, heading_path, char_start, char_end,
              chunk_hash, token_estimate, embedding_status)
             VALUES (?, 0, 'alpha chunk', 'alpha chunk', ?, 0, 11, 'h', 2, 'pending')",
            params![note_id, heading],
        )
        .is_ok(),
        "failed to insert test chunk"
    );
}

#[test]
fn search_query_tag_syntax_filters_candidates() {
    let path = unique_path();
    let conn = open_database(&path).unwrap_or_else(|err| panic!("failed to open temp db: {err}"));
    let included = insert_note(&conn, "notes/included.md", "alpha shared");
    insert_note(&conn, "notes/excluded.md", "alpha shared");
    insert_tag(&conn, included, "fermentation");

    let input = SearchInput {
        query: Some("alpha #fermentation".to_string()),
        mode: SearchMode::Fulltext,
        ..SearchInput::default()
    };
    let response = run_search(&conn, &input, None, None, None);

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].vault_path.as_str(), "notes/included.md");
    drop(conn);
    cleanup(&path);
}

#[test]
fn search_query_heading_syntax_filters_candidates() {
    let path = unique_path();
    let conn = open_database(&path).unwrap_or_else(|err| panic!("failed to open temp db: {err}"));
    let included = insert_note(&conn, "notes/included.md", "alpha shared");
    let excluded = insert_note(&conn, "notes/excluded.md", "alpha shared");
    insert_heading_chunk(&conn, included, "Targets");
    insert_heading_chunk(&conn, excluded, "Other");

    let input = SearchInput {
        query: Some("alpha heading:Targets".to_string()),
        mode: SearchMode::Fulltext,
        ..SearchInput::default()
    };
    let response = run_search(&conn, &input, None, None, None);

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].vault_path.as_str(), "notes/included.md");
    drop(conn);
    cleanup(&path);
}
