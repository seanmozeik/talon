#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use rusqlite::Connection;
use rusqlite::params;

use super::*;
use crate::contracts::PositiveCount;
use crate::indexing::migrations::run_migrations;
use crate::query::ReadInput;

fn fresh_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    run_migrations(&mut conn).unwrap();
    conn
}

fn insert_note(conn: &Connection, vault_path: &str, title: &str, content: &str) -> i64 {
    conn.execute(
        "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
         VALUES (?, ?, '[]', '[]', ?, 0, 0, 'h', 'd', 1)",
        params![vault_path, title, content],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn vault_root() -> PathBuf {
    PathBuf::from("/vault")
}

fn read_input(path: &str) -> ReadInput {
    ReadInput {
        path: Some(path.to_string()),
        raw: false,
        from_line: None,
        max_lines: None,
    }
}

#[test]
fn strips_frontmatter_by_default() {
    let conn = fresh_db();
    insert_note(
        &conn,
        "notes/example.md",
        "Example",
        "---\ntitle: Example\ntags: [rust]\n---\n\nHello world\n",
    );

    let resp = run_read(&conn, &vault_root(), &read_input("notes/example.md"));
    let result = &resp.results[0];

    assert!(result.found);
    let content = result.content.as_deref().unwrap();
    assert!(
        !content.contains("---"),
        "frontmatter delimiters must be stripped"
    );
    assert!(
        !content.contains("title:"),
        "frontmatter fields must be stripped"
    );
    assert!(content.contains("Hello world"));
}

#[test]
fn raw_mode_preserves_frontmatter() {
    let conn = fresh_db();
    insert_note(
        &conn,
        "notes/raw.md",
        "Raw",
        "---\ntitle: Raw\n---\n\nBody here\n",
    );

    let resp = run_read(
        &conn,
        &vault_root(),
        &ReadInput {
            path: Some("notes/raw.md".to_string()),
            raw: true,
            from_line: None,
            max_lines: None,
        },
    );
    let result = &resp.results[0];
    assert!(result.found);
    let content = result.content.as_deref().unwrap();
    assert!(
        content.contains("---"),
        "raw mode must preserve frontmatter delimiters"
    );
    assert!(content.contains("title: Raw"));
}

#[test]
fn line_range_clips_body() {
    let conn = fresh_db();
    let body = "line1\nline2\nline3\nline4\nline5\n";
    insert_note(&conn, "notes/lines.md", "Lines", body);

    let resp = run_read(
        &conn,
        &vault_root(),
        &ReadInput {
            path: Some("notes/lines.md".to_string()),
            raw: true,
            from_line: Some(PositiveCount::new(2, "from_line").unwrap()),
            max_lines: Some(PositiveCount::new(2, "max_lines").unwrap()),
        },
    );
    let result = &resp.results[0];
    assert!(result.found);
    let content = result.content.as_deref().unwrap();
    assert_eq!(content, "line2\nline3");
}

#[test]
fn missing_note_returns_not_found() {
    let conn = fresh_db();

    let resp = run_read(&conn, &vault_root(), &read_input("does/not/exist.md"));
    assert_eq!(resp.results.len(), 1);
    let result = &resp.results[0];
    assert!(!result.found);
    assert!(result.content.is_none());
}

#[test]
fn extension_less_path_resolves_md_note() {
    let conn = fresh_db();
    insert_note(&conn, "notes/example.md", "Example", "Hello");

    let resp = run_read(&conn, &vault_root(), &read_input("notes/example"));
    let result = &resp.results[0];
    assert!(result.found, "should resolve without .md extension");
    assert_eq!(result.vault_path.as_str(), "notes/example.md");
}

#[test]
fn links_and_backlinks_hydrated() {
    let conn = fresh_db();
    insert_note(&conn, "a.md", "A", "content");
    insert_note(&conn, "b.md", "B", "content");
    conn.execute(
        "INSERT INTO links (from_path, to_path, raw_target) VALUES (?, ?, ?)",
        params!["a.md", "b.md", "[[b]]"],
    )
    .unwrap();

    let resp = run_read(&conn, &vault_root(), &read_input("a.md"));
    let result = &resp.results[0];
    assert!(result.found);
    assert!(
        result.links.contains(&"b.md".to_string()),
        "outgoing link must be present"
    );

    let resp_b = run_read(&conn, &vault_root(), &read_input("b.md"));
    let result_b = &resp_b.results[0];
    assert!(
        result_b.backlinks.contains(&"a.md".to_string()),
        "backlink must be present"
    );
}
