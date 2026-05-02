use rusqlite::{Connection, params};

use super::*;
use crate::config::{
    ChunkerConfig, ExpansionConfig, InferenceConfig, InferenceModels, RerankConfig, Scope,
    ScopeGlob, ScopePriority, ScopesConfig, SearchConfig, TalonConfig,
};
use crate::indexing::migrations::run_migrations;
use crate::query::MetaInput;
use crate::search::{WhereClause, WhereOperator};
use std::path::PathBuf;

fn test_config_with_scopes(scopes: Vec<(&str, &str)>) -> TalonConfig {
    let mut map = ScopesConfig::new();
    for (name, glob) in scopes {
        map.insert(
            name.to_string(),
            Scope {
                glob: ScopeGlob::Single(glob.to_string()),
                priority: ScopePriority::Normal,
                default: true,
                inspect: true,
            },
        );
    }
    TalonConfig {
        vault_path: PathBuf::from("/vault"),
        db_path: PathBuf::from("/vault/.talon/index.db"),
        config_file_path: None,
        include_patterns: Vec::new(),
        ignore_patterns: Vec::new(),
        inference: InferenceConfig {
            base_url: "http://localhost:8080".to_string(),
            models: InferenceModels {
                query_embedding: "q".to_string(),
                query_embedding_context_tokens: 512,
                document_embedding: "d".to_string(),
                chunk_embedding: "c".to_string(),
                reranker: "r".to_string(),
                reranker_context_tokens: 512,
            },
            rerank: RerankConfig::default(),
        },
        expansion: ExpansionConfig {
            provider: "openai-compatible".to_string(),
            base_url: "http://localhost:8080".to_string(),
            model: "x".to_string(),
            context_tokens: 32768,
            max_output_tokens: None,
        },
        ask: crate::config::AskConfig::default(),
        mcp: crate::config::McpConfig::default(),
        scopes: map,
        search: SearchConfig::default(),
        inspect: crate::config::InspectConfig::default(),
        chunker: ChunkerConfig::default(),
    }
}

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

fn insert_typed_fm_field(
    conn: &Connection,
    note_id: i64,
    field: &str,
    value: &str,
    value_type: &str,
) {
    conn.execute(
        "INSERT INTO note_frontmatter_fields \
             (note_id, field, value, value_type, value_norm) VALUES (?, ?, ?, ?, ?)",
        params![note_id, field, value, value_type, value.to_lowercase()],
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
        None,
    );

    let tc = resp
        .tag_counts
        .expect("tag_counts must be Some when requested");
    assert_eq!(tc.get("rust"), Some(&2));
    assert_eq!(tc.get("programming"), Some(&1));
    assert_eq!(tc.get("algorithms"), Some(&1));
}
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
        None,
    );

    assert_eq!(resp.entries.len(), 1);
    assert_eq!(resp.entries[0].path.as_str(), "project.md");
}
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
        None,
    );

    assert_eq!(resp.entries.len(), 1);
    assert_eq!(resp.entries[0].path.as_str(), "done.md");
}
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
        None,
    );

    assert_eq!(resp.entries.len(), 1);
    assert_eq!(resp.entries[0].path.as_str(), "rust-notes.md");
}
#[test]
fn where_numeric_comparison_uses_value_type() {
    let conn = fresh_db();
    let n1 = insert_note_with_fm(&conn, "high.md", "{}", 0);
    let n2 = insert_note_with_fm(&conn, "low.md", "{}", 0);
    insert_typed_fm_field(&conn, n1, "priority", "10", "number");
    insert_typed_fm_field(&conn, n2, "priority", "2", "number");

    let resp = query_meta(
        &conn,
        &MetaInput {
            where_: vec![WhereClause {
                key: "priority".into(),
                op: WhereOperator::GreaterThan,
                value: Some("3".into()),
            }],
            ..MetaInput::default()
        },
        None,
    );

    assert_eq!(resp.entries.len(), 1);
    assert_eq!(resp.entries[0].path.as_str(), "high.md");
}
#[test]
fn where_date_comparison_uses_value_type() {
    let conn = fresh_db();
    let n1 = insert_note_with_fm(&conn, "new.md", "{}", 0);
    let n2 = insert_note_with_fm(&conn, "old.md", "{}", 0);
    insert_typed_fm_field(&conn, n1, "due", "2026-04-29", "date");
    insert_typed_fm_field(&conn, n2, "due", "2026-04-01", "date");

    let resp = query_meta(
        &conn,
        &MetaInput {
            where_: vec![WhereClause {
                key: "due".into(),
                op: WhereOperator::GreaterThan,
                value: Some("2026-04-15".into()),
            }],
            ..MetaInput::default()
        },
        None,
    );

    assert_eq!(resp.entries.len(), 1);
    assert_eq!(resp.entries[0].path.as_str(), "new.md");
}
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
        None,
    );

    assert_eq!(resp.entries.len(), 1);
    assert_eq!(resp.entries[0].path.as_str(), "referencing.md");
}
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
        None,
    );

    assert_eq!(resp.entries.len(), 1);
    assert_eq!(resp.entries[0].path.as_str(), "new.md");
}
#[test]
fn scope_only_filters_by_configured_scope() {
    let conn = fresh_db();
    insert_note_with_fm(&conn, "Atlas/note.md", "{}", 0);
    insert_note_with_fm(&conn, "Search/note.md", "{}", 0);

    let config = test_config_with_scopes(vec![("atlas", "Atlas/**"), ("search", "Search/**")]);
    let resp = query_meta(
        &conn,
        &MetaInput {
            scope_only: vec!["atlas".into()],
            ..MetaInput::default()
        },
        Some(&config),
    );

    assert_eq!(resp.entries.len(), 1);
    assert_eq!(resp.entries[0].path.as_str(), "Atlas/note.md");
}
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
        None,
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
