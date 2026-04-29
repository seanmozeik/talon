use rusqlite::params;

use super::*;
use crate::search::{WhereClause, WhereOperator};
use crate::store::open_database;
use std::env::temp_dir;
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_path() -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    temp_dir().join(format!("talon-bm25-test-{pid}-{n}.sqlite"))
}

fn cleanup(path: &std::path::Path) {
    let _ = fs_err::remove_file(path);
    let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
    let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
}

fn insert_note(
    conn: &Connection,
    vault_path: &str,
    title: &str,
    content: &str,
    aliases_json: &str,
) -> i64 {
    conn.execute(
        "INSERT INTO notes
         (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
         VALUES (?, ?, '[]', ?, ?, 0, 0, 'h', 'd', 1)",
        params![vault_path, title, aliases_json, content],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn insert_alias(conn: &Connection, note_id: i64, alias: &str) {
    let norm = normalize_keyword(alias);
    conn.execute(
        "INSERT INTO note_aliases (note_id, alias, alias_norm) VALUES (?, ?, ?)",
        params![note_id, alias, norm],
    )
    .unwrap();
}

fn insert_frontmatter_field(
    conn: &Connection,
    note_id: i64,
    field: &str,
    value: &str,
    value_type: &str,
) {
    conn.execute(
        "INSERT INTO note_frontmatter_fields (note_id, field, value, value_type, value_norm)
         VALUES (?, ?, ?, ?, ?)",
        params![note_id, field, value, value_type, normalize_keyword(value)],
    )
    .unwrap();
}

#[test]
fn bm25_returns_matching_notes_and_score_in_unit_interval() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    insert_note(
        &conn,
        "a.md",
        "Atomic Notes",
        "atomic notes are small",
        "[]",
    );
    insert_note(&conn, "b.md", "Other", "completely different content", "[]");

    let results = search_bm25(&conn, "atomic notes", 10, 300, &PreFilter::none());
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].path, "a.md");
    assert!(results[0].score > 0.0 && results[0].score <= 1.0);
    assert!(results[0].scores.bm25.is_some());
    assert!(!results[0].snippet.is_empty());
    drop(conn);
    cleanup(&path);
}

#[test]
fn bm25_matches_or_so_more_terms_means_better_rank() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    insert_note(&conn, "both.md", "Foo Bar", "alpha beta", "[]");
    insert_note(&conn, "one.md", "Foo Only", "alpha gamma", "[]");

    let results = search_bm25(&conn, "alpha beta", 10, 300, &PreFilter::none());
    let both = results.iter().position(|r| r.path == "both.md").unwrap();
    let one = results.iter().position(|r| r.path == "one.md").unwrap();
    assert!(both < one, "both.md should outrank one.md");
    drop(conn);
    cleanup(&path);
}

#[test]
fn bm25_ignores_inactive_notes() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    insert_note(&conn, "a.md", "Atomic", "atomic notes", "[]");
    conn.execute("UPDATE notes SET active = 0 WHERE vault_path = 'a.md'", [])
        .unwrap();
    let results = search_bm25(&conn, "atomic", 10, 300, &PreFilter::none());
    assert!(results.is_empty());
    drop(conn);
    cleanup(&path);
}

#[test]
fn bm25_prefilter_numeric_where_uses_value_type() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    let high = insert_note(&conn, "high.md", "Alpha", "alpha shared", "[]");
    let low = insert_note(&conn, "low.md", "Alpha", "alpha shared", "[]");
    insert_frontmatter_field(&conn, high, "priority", "10", "number");
    insert_frontmatter_field(&conn, low, "priority", "2", "number");
    let pre_filter = PreFilter {
        since_ms: None,
        accepted_note_ids: None,
        where_clauses: vec![WhereClause {
            key: "priority".into(),
            op: WhereOperator::GreaterThan,
            value: Some("3".into()),
        }],
    };

    let results = search_bm25(&conn, "alpha", 10, 300, &pre_filter);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].path, "high.md");
    drop(conn);
    cleanup(&path);
}

#[test]
fn alias_exact_finds_normalized_match() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    let id = insert_note(
        &conn,
        "a.md",
        "Atomic Notes",
        "body",
        "[\"Atomic\",\"Zettel\"]",
    );
    insert_alias(&conn, id, "Atomic");
    insert_alias(&conn, id, "Zettel");

    let results = search_by_alias_exact(&conn, "atomic", 10, &PreFilter::none());
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].path, "a.md");
    assert!((results[0].score - 1.0).abs() < f64::EPSILON);
    assert_eq!(results[0].aliases, vec!["Atomic", "Zettel"]);
    drop(conn);
    cleanup(&path);
}

#[test]
fn alias_exact_misses_non_match() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    let id = insert_note(&conn, "a.md", "Atomic", "body", "[]");
    insert_alias(&conn, id, "Atomic");
    assert!(search_by_alias_exact(&conn, "completely-other", 10, &PreFilter::none()).is_empty());
    drop(conn);
    cleanup(&path);
}

#[test]
fn alias_exact_finds_two_char_alias() {
    // Aliases shorter than FUZZY_ALIAS_MIN_LEN (3) produce no trigrams and
    // are invisible to the FTS5 MATCH operator. The alias_norm exact-match
    // path must find them regardless.
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    let id = insert_note(
        &conn,
        "ai.md",
        "Artificial Intelligence",
        "body",
        "[\"AI\"]",
    );
    insert_alias(&conn, id, "AI");

    let results = search_by_alias_exact(&conn, "AI", 10, &PreFilter::none());
    assert_eq!(
        results.len(),
        1,
        "2-char alias must be findable via exact match"
    );
    assert_eq!(results[0].path, "ai.md");
    assert!((results[0].score - 1.0).abs() < f64::EPSILON);
    drop(conn);
    cleanup(&path);
}

#[test]
#[ignore = "diagnostic only"]
#[allow(clippy::unwrap_used)]
fn bm25_score_diagnostic_temp() {
    use crate::open_database;
    let path = temp_dir().join("score_diag_test.sqlite");
    let conn = open_database(&path).unwrap();
    for i in 0..20 {
        conn.execute(
            "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active) VALUES (?, ?, '[]', '[]', ?, 0, 0, 'h', 'd', 1)",
            rusqlite::params![format!("dummy-{i}.md"), format!("Unrelated Topic {i}"), format!("content about something completely different topic number {i}")],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active) VALUES (?, ?, '[]', '[]', ?, 0, 0, 'h', 'd', 1)",
        rusqlite::params!["signal.md", "crystallophosphene Research", "unique term found nowhere else"],
    )
    .unwrap();
    let results = search_bm25(&conn, "crystallophosphene", 2, 300, &PreFilter::none());
    for r in &results {
        eprintln!("  path={} score={:.6}", r.path, r.score);
    }
    assert!(!results.is_empty());
    let _ = fs_err::remove_file(&path);
    let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
    let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
}
