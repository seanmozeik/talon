use rusqlite::{Connection, params};

use super::*;
use crate::indexing::migrations::run_migrations;
use crate::query::MetaInput;
use crate::search::{WhereClause, WhereOperator};

fn fresh_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    run_migrations(&mut conn).unwrap();
    conn
}

fn insert_note_with_fm(
    conn: &Connection,
    vault_path: &str,
    frontmatter_json: &str,
    mtime_ms: i64,
) -> i64 {
    conn.execute(
        "INSERT INTO notes \
             (vault_path, title, tags, aliases, content, frontmatter, \
              mtime_ms, size_bytes, hash, docid, active) \
             VALUES (?, '', '[]', '[]', '', ?, ?, 0, 'h', 'd', 1)",
        params![vault_path, frontmatter_json, mtime_ms],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn insert_fm_field(conn: &Connection, note_id: i64, field: &str, value: &str) {
    conn.execute(
        "INSERT INTO note_frontmatter_fields \
             (note_id, field, value, value_norm) VALUES (?, ?, ?, ?)",
        params![note_id, field, value, value.to_lowercase()],
    )
    .unwrap();
}

fn insert_tag(conn: &Connection, note_id: i64, tag: &str) {
    conn.execute(
        "INSERT INTO note_tags (note_id, tag, tag_norm) VALUES (?, ?, ?)",
        params![note_id, tag, tag.to_lowercase()],
    )
    .unwrap();
}

// ── Test 1: tag counts ─────────────────────���──────────────────────────────

#[test]
fn tag_counts_aggregates_by_tag() {
    let conn = fresh_db();
    let n1 = insert_note_with_fm(&conn, "a.md", "{}", 0);
    let n2 = insert_note_with_fm(&conn, "b.md", "{}", 0);
    insert_tag(&conn, n1, "rust");
    insert_tag(&conn, n1, "programming");
    insert_tag(&conn, n2, "rust");
    insert_tag(&conn, n2, "algorithms");

    let resp = query_meta(
        &conn,
        &MetaInput {
            tag_counts: true,
            ..MetaInput::default()
        },
    );

    let tc = resp
        .tag_counts
        .expect("tag_counts must be Some when requested");
    assert_eq!(tc.get("rust"), Some(&2));
    assert_eq!(tc.get("programming"), Some(&1));
    assert_eq!(tc.get("algorithms"), Some(&1));
}

// ── Test 2: where equals ──────────────────────────────────────────────────

#[test]
fn where_equals_filters_notes() {
    let conn = fresh_db();
    let n1 = insert_note_with_fm(&conn, "project.md", "{}", 0);
    let n2 = insert_note_with_fm(&conn, "note.md", "{}", 0);
    insert_fm_field(&conn, n1, "type", "project");
    insert_fm_field(&conn, n2, "type", "note");

    let resp = query_meta(
        &conn,
        &MetaInput {
            where_: vec![WhereClause {
                key: "type".into(),
                op: WhereOperator::Equals,
                value: Some("project".into()),
            }],
            ..MetaInput::default()
        },
    );

    assert_eq!(resp.entries.len(), 1);
    assert_eq!(resp.entries[0].path.as_str(), "project.md");
}

// ── Test 3: where exists ──────────────────────────────────────────────────

#[test]
fn where_exists_returns_notes_with_field() {
    let conn = fresh_db();
    let n1 = insert_note_with_fm(&conn, "done.md", "{}", 0);
    insert_note_with_fm(&conn, "empty.md", "{}", 0);
    insert_fm_field(&conn, n1, "status", "done");

    let resp = query_meta(
        &conn,
        &MetaInput {
            where_: vec![WhereClause {
                key: "status".into(),
                op: WhereOperator::Exists,
                value: None,
            }],
            ..MetaInput::default()
        },
    );

    assert_eq!(resp.entries.len(), 1);
    assert_eq!(resp.entries[0].path.as_str(), "done.md");
}

// ── Test 4: where contains ────────────────────────────────────────────────

#[test]
fn where_contains_matches_substring() {
    let conn = fresh_db();
    let n1 = insert_note_with_fm(&conn, "rust-notes.md", "{}", 0);
    let n2 = insert_note_with_fm(&conn, "python-notes.md", "{}", 0);
    insert_fm_field(&conn, n1, "description", "rust programming language");
    insert_fm_field(&conn, n2, "description", "python scripting");

    let resp = query_meta(
        &conn,
        &MetaInput {
            where_: vec![WhereClause {
                key: "description".into(),
                op: WhereOperator::Contains,
                value: Some("rust".into()),
            }],
            ..MetaInput::default()
        },
    );

    assert_eq!(resp.entries.len(), 1);
    assert_eq!(resp.entries[0].path.as_str(), "rust-notes.md");
}

// ── Test 5: sources lookup ────────────────────────────────────────────────

#[test]
fn sources_returns_notes_referencing_target() {
    let conn = fresh_db();
    let n1 = insert_note_with_fm(&conn, "referencing.md", "{}", 0);
    let n2 = insert_note_with_fm(&conn, "unrelated.md", "{}", 0);
    insert_fm_field(&conn, n1, "sources", "target.md");
    insert_fm_field(&conn, n2, "sources", "other.md");

    let resp = query_meta(
        &conn,
        &MetaInput {
            sources: Some("target.md".into()),
            ..MetaInput::default()
        },
    );

    assert_eq!(resp.entries.len(), 1);
    assert_eq!(resp.entries[0].path.as_str(), "referencing.md");
}

// ── Test 6: since filter ──────────────────────────────────────────────────

#[test]
fn since_filter_excludes_old_notes() {
    let conn = fresh_db();
    insert_note_with_fm(&conn, "old.md", "{}", 1000);
    insert_note_with_fm(&conn, "new.md", "{}", 3000);

    let resp = query_meta(
        &conn,
        &MetaInput {
            since: Some("2000".into()),
            ..MetaInput::default()
        },
    );

    assert_eq!(resp.entries.len(), 1);
    assert_eq!(resp.entries[0].path.as_str(), "new.md");
}

// ── Test 7: scope_only filter ─────────────────────────────────────────────

#[test]
fn scope_only_filters_by_prefix() {
    let conn = fresh_db();
    insert_note_with_fm(&conn, "Atlas/note.md", "{}", 0);
    insert_note_with_fm(&conn, "Search/note.md", "{}", 0);

    let resp = query_meta(
        &conn,
        &MetaInput {
            scope_only: vec!["Atlas".into()],
            ..MetaInput::default()
        },
    );

    assert_eq!(resp.entries.len(), 1);
    assert_eq!(resp.entries[0].path.as_str(), "Atlas/note.md");
}

// ── Test 8: select projects fields ────────────────────────────────────────

#[test]
fn select_projects_only_requested_fields() {
    let conn = fresh_db();
    let fm =
        r#"{"type":{"String":"project"},"status":{"String":"active"},"priority":{"Number":1.0}}"#;
    insert_note_with_fm(&conn, "proj.md", fm, 0);

    let resp = query_meta(
        &conn,
        &MetaInput {
            select: vec!["type".into()],
            ..MetaInput::default()
        },
    );

    assert_eq!(resp.entries.len(), 1);
    let fm = &resp.entries[0].frontmatter;
    assert!(fm.contains_key("type"), "selected field must be present");
    assert!(
        !fm.contains_key("status"),
        "non-selected field must be absent"
    );
    assert!(
        !fm.contains_key("priority"),
        "non-selected field must be absent"
    );
}
