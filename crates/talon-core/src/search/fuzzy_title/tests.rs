use super::*;
use crate::store::open_database;
use crate::text::frontmatter::normalize_keyword;
use rusqlite::params;
use std::env::temp_dir;
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_path() -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    temp_dir().join(format!("talon-fuzzy-test-{pid}-{n}.sqlite"))
}

fn cleanup(path: &std::path::Path) {
    let _ = fs_err::remove_file(path);
    let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
    let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
}

fn insert_note(conn: &Connection, vault_path: &str, title: &str, aliases_json: &str) -> i64 {
    conn.execute(
        "INSERT INTO notes
         (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
         VALUES (?, ?, '[]', ?, ?, 0, 0, 'h', 'd', 1)",
        params![vault_path, title, aliases_json, title],
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

#[test]
fn max_alias_overlap_filters_short_aliases() {
    let aliases = vec!["ab".into(), "atomic".into()];
    let overlap = max_alias_overlap("atomic", &aliases);
    assert_eq!(overlap, 1.0);
    let only_short = max_alias_overlap("ab", &["ab".to_string()]);
    assert_eq!(only_short, 0.0);
}

#[test]
fn fuzzy_title_finds_close_match() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    insert_note(&conn, "a.md", "Zettelkasten", "[]");
    insert_note(&conn, "b.md", "Unrelated Topic", "[]");

    let parts = search_title_parts(&conn, "zettelkasten", 10, &PreFilter::none());
    assert!(parts.fuzzy.iter().any(|r| r.path == "a.md"));
    assert!(parts.exact_alias.is_empty());
    drop(conn);
    cleanup(&path);
}

#[test]
fn fuzzy_separates_exact_alias_from_fuzzy() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    let id = insert_note(&conn, "a.md", "Atomic Notes", "[\"Atomic\"]");
    insert_alias(&conn, id, "Atomic");

    let parts = search_title_parts(&conn, "atomic", 10, &PreFilter::none());
    assert_eq!(parts.exact_alias.len(), 1);
    assert_eq!(parts.exact_alias[0].path, "a.md");
    assert!(parts.fuzzy.iter().all(|r| r.path != "a.md"));
    drop(conn);
    cleanup(&path);
}

#[test]
fn search_fuzzy_title_unions_both_buckets() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    let id = insert_note(&conn, "a.md", "Atomic", "[\"Atomic\"]");
    insert_alias(&conn, id, "Atomic");
    insert_note(&conn, "b.md", "Atomically Inclined", "[]");

    let out = search_fuzzy_title(&conn, "atomic", 10, &PreFilter::none());
    let paths: Vec<&str> = out.iter().map(|r| r.path.as_str()).collect();
    assert!(paths.contains(&"a.md"));
    assert!(paths.contains(&"b.md"));
    let a = out.iter().find(|r| r.path == "a.md").unwrap();
    let b = out.iter().find(|r| r.path == "b.md").unwrap();
    assert_eq!(a.score, 1.0);
    assert!(b.score > 0.0 && b.score < 1.0);
    drop(conn);
    cleanup(&path);
}

#[test]
fn trigram_matches_accented_title_without_accent_in_query() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    insert_note(&conn, "cafe.md", "Café del Sol", "[]");

    let parts = search_title_parts(&conn, "cafe", 10, &PreFilter::none());
    assert!(
        parts.fuzzy.iter().any(|r| r.path == "cafe.md"),
        "trigram search should match accented title with unaccented query"
    );
    drop(conn);
    cleanup(&path);
}

#[test]
fn trigram_cyrillic_substring_search() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    insert_note(&conn, "ru.md", "Концепция zettelkasten", "[]");

    let parts = search_title_parts(&conn, "Концепция", 10, &PreFilter::none());
    assert!(
        parts.fuzzy.iter().any(|r| r.path == "ru.md"),
        "trigram search should find Cyrillic title substring"
    );
    drop(conn);
    cleanup(&path);
}

#[test]
fn trigram_overlap_squared_shorter_title_higher_score() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    insert_note(&conn, "a.md", "Atomic Notes", "[]");
    insert_note(&conn, "b.md", "Notes on Atomic Habits", "[]");

    let parts = search_title_parts(&conn, "atom", 10, &PreFilter::none());
    let a = parts.fuzzy.iter().find(|r| r.path == "a.md").unwrap();
    let b = parts.fuzzy.iter().find(|r| r.path == "b.md").unwrap();
    assert!(a.score > b.score);
    drop(conn);
    cleanup(&path);
}

#[test]
fn trigram_overlap_squared_penalty_with_typo() {
    let path = unique_path();
    let conn = open_database(&path).unwrap();
    insert_note(&conn, "a.md", "Atomic Notes", "[]");

    let perfect_parts = search_title_parts(&conn, "atomic", 10, &PreFilter::none());
    let typo_parts = search_title_parts(&conn, "atomik", 10, &PreFilter::none());

    let perfect = perfect_parts
        .fuzzy
        .iter()
        .find(|r| r.path == "a.md")
        .unwrap()
        .score;
    let typo = typo_parts
        .fuzzy
        .iter()
        .find(|r| r.path == "a.md")
        .unwrap()
        .score;
    assert!(typo < perfect);
    drop(conn);
    cleanup(&path);
}
