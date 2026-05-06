#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::env::temp_dir;
use std::sync::atomic::{AtomicU64, Ordering};

use fs_err as fs;
use rusqlite::Connection;

use super::*;
use crate::store::open_database;

fn unique_dir(label: &str) -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    temp_dir().join(format!("talon-scan-test-{label}-{pid}-{n}"))
}

fn cleanup_db(path: &std::path::Path) {
    let _ = fs_err::remove_file(path);
    let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
    let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
}

fn write_note(root: &std::path::Path, rel: &str, body: &str) {
    let full = root.join(rel);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&full, body).unwrap();
}

fn active_paths(conn: &Connection) -> Vec<String> {
    conn.prepare_cached("SELECT vault_path FROM notes WHERE active = 1 ORDER BY vault_path")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .filter_map(Result::ok)
        .collect()
}

fn link_targets(conn: &Connection) -> Vec<String> {
    conn.prepare_cached("SELECT to_path FROM links ORDER BY to_path")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .filter_map(Result::ok)
        .collect()
}

#[test]
fn full_scan_indexes_every_markdown_file() {
    let vault = unique_dir("full");
    fs::create_dir_all(&vault).unwrap();
    write_note(&vault, "a.md", "# A\nbody a");
    write_note(&vault, "zone/b.md", "# B\nbody b");
    write_note(&vault, "zone/skip.txt", "ignored");
    let db = vault.join("idx.sqlite");
    let mut conn = open_database(&db).unwrap();
    let stats = run_full_scan(&mut conn, &vault, &IndexerConfig::index_all()).unwrap();
    assert_eq!(stats.indexed, 2);
    assert_eq!(stats.deleted, 0);
    assert_eq!(active_paths(&conn), vec!["a.md", "zone/b.md"]);
    drop(conn);
    cleanup_db(&db);
    fs::remove_dir_all(&vault).unwrap();
}

#[test]
fn second_scan_skips_unchanged_files() {
    let vault = unique_dir("rescan");
    fs::create_dir_all(&vault).unwrap();
    write_note(&vault, "a.md", "# A");
    let db = vault.join("idx.sqlite");
    let mut conn = open_database(&db).unwrap();
    let first = run_full_scan(&mut conn, &vault, &IndexerConfig::index_all()).unwrap();
    assert_eq!(first.indexed, 1);
    let second = run_full_scan(&mut conn, &vault, &IndexerConfig::index_all()).unwrap();
    assert_eq!(second.indexed, 0);
    assert!(second.skipped >= 1);
    drop(conn);
    cleanup_db(&db);
    fs::remove_dir_all(&vault).unwrap();
}

#[test]
fn second_scan_skips_unchanged_frontmatter_files() {
    let vault = unique_dir("frontmatter-rescan");
    fs::create_dir_all(&vault).unwrap();
    write_note(&vault, "a.md", "---\ntitle: A\n---\n\n# A");
    let db = vault.join("idx.sqlite");
    let mut conn = open_database(&db).unwrap();

    let first = run_full_scan(&mut conn, &vault, &IndexerConfig::index_all()).unwrap();
    assert_eq!(first.indexed, 1);

    let second = run_full_scan(&mut conn, &vault, &IndexerConfig::index_all()).unwrap();
    assert_eq!(second.indexed, 0);
    assert_eq!(second.skipped, 1);

    drop(conn);
    cleanup_db(&db);
    fs::remove_dir_all(&vault).unwrap();
}

#[test]
fn modified_file_is_indexed_again_not_skipped() {
    let vault = unique_dir("modified");
    fs::create_dir_all(&vault).unwrap();
    write_note(&vault, "a.md", "# A\nfirst");
    let db = vault.join("idx.sqlite");
    let mut conn = open_database(&db).unwrap();

    let first = run_full_scan(&mut conn, &vault, &IndexerConfig::index_all()).unwrap();
    assert_eq!(first.indexed, 1);

    write_note(&vault, "a.md", "# A\nsecond revision with more bytes");
    let second = run_full_scan(&mut conn, &vault, &IndexerConfig::index_all()).unwrap();
    assert_eq!(second.indexed, 1);

    let content: String = conn
        .query_row(
            "SELECT content FROM notes WHERE vault_path = 'a.md' AND active = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(content.contains("second revision"));

    drop(conn);
    cleanup_db(&db);
    fs::remove_dir_all(&vault).unwrap();
}

#[test]
fn metadata_only_change_is_skipped_when_hash_matches() {
    let vault = unique_dir("metadata-only");
    fs::create_dir_all(&vault).unwrap();
    write_note(&vault, "a.md", "# A\nalpha");
    let db = vault.join("idx.sqlite");
    let mut conn = open_database(&db).unwrap();

    let first = run_full_scan(&mut conn, &vault, &IndexerConfig::index_all()).unwrap();
    assert_eq!(first.indexed, 1);

    conn.execute(
        "UPDATE notes SET mtime_ms = mtime_ms - 1000, size_bytes = size_bytes + 1 WHERE vault_path = 'a.md'",
        [],
    )
    .unwrap();

    let second = run_full_scan(&mut conn, &vault, &IndexerConfig::index_all()).unwrap();
    assert_eq!(second.indexed, 0);
    assert_eq!(second.skipped, 1);

    drop(conn);
    cleanup_db(&db);
    fs::remove_dir_all(&vault).unwrap();
}

#[test]
fn ignore_patterns_skip_matching_paths_case_insensitively() {
    let vault = unique_dir("ignore");
    fs::create_dir_all(&vault).unwrap();
    write_note(&vault, "keep.md", "# Keep");
    write_note(&vault, "Templates/Daily.md", "# Template");
    let db = vault.join("idx.sqlite");
    let mut conn = open_database(&db).unwrap();

    let stats = run_full_scan(&mut conn, &vault, &IndexerConfig::index_all()).unwrap();
    assert_eq!(stats.indexed, 1);
    assert_eq!(active_paths(&conn), vec!["keep.md"]);

    drop(conn);
    cleanup_db(&db);
    fs::remove_dir_all(&vault).unwrap();
}

#[test]
fn full_scan_does_not_store_unresolved_links_to_ignored_existing_files() {
    let vault = unique_dir("ignored-links");
    fs::create_dir_all(&vault).unwrap();
    write_note(
        &vault,
        "source.md",
        "[[menu-board]] [[templates/Recipe Template]] [[nonexistent]]",
    );
    write_note(&vault, "templates/Recipe Template.md", "# Recipe");
    fs::write(vault.join("menu-board.canvas"), "{}").unwrap();
    let db = vault.join("idx.sqlite");
    let mut conn = open_database(&db).unwrap();

    let config = IndexerConfig {
        ignore_patterns: vec!["*.canvas".into()],
        ..IndexerConfig::index_all()
    };
    let stats = run_full_scan(&mut conn, &vault, &config).unwrap();

    assert_eq!(stats.indexed, 1);
    assert_eq!(link_targets(&conn), vec!["nonexistent"]);

    drop(conn);
    cleanup_db(&db);
    fs::remove_dir_all(&vault).unwrap();
}

#[test]
fn full_scan_keeps_unresolved_links_that_only_match_ignored_extensions() {
    let vault = unique_dir("missing-canvas");
    fs::create_dir_all(&vault).unwrap();
    write_note(&vault, "source.md", "[[nonexistent]]");
    let db = vault.join("idx.sqlite");
    let mut conn = open_database(&db).unwrap();

    let config = IndexerConfig {
        ignore_patterns: vec!["*.canvas".into()],
        ..IndexerConfig::index_all()
    };
    run_full_scan(&mut conn, &vault, &config).unwrap();

    assert_eq!(link_targets(&conn), vec!["nonexistent"]);

    drop(conn);
    cleanup_db(&db);
    fs::remove_dir_all(&vault).unwrap();
}

#[test]
fn reconcile_soft_deletes_missing_files() {
    let vault = unique_dir("reconcile");
    fs::create_dir_all(&vault).unwrap();
    write_note(&vault, "stay.md", "# Stay");
    write_note(&vault, "go.md", "# Go");
    let db = vault.join("idx.sqlite");
    let mut conn = open_database(&db).unwrap();
    run_full_scan(&mut conn, &vault, &IndexerConfig::index_all()).unwrap();

    fs::remove_file(vault.join("go.md")).unwrap();
    let deleted = reconcile_deletions(&mut conn, &vault).unwrap();
    assert_eq!(deleted, 1);
    assert_eq!(active_paths(&conn), vec!["stay.md"]);

    drop(conn);
    cleanup_db(&db);
    fs::remove_dir_all(&vault).unwrap();
}

#[test]
fn reconcile_ignored_notes_prunes_case_mismatch() {
    let vault = unique_dir("reconcile-ignored");
    fs::create_dir_all(&vault).unwrap();
    write_note(&vault, "keep.md", "# Keep");
    write_note(&vault, "Templates/Daily.md", "# Template");
    let db = vault.join("idx.sqlite");
    let mut conn = open_database(&db).unwrap();

    write_note(&vault, "seed.md", "# Template");
    run_full_scan(&mut conn, &vault, &IndexerConfig::index_all()).unwrap();
    conn.execute(
        "UPDATE notes SET vault_path = 'Templates/Daily.md' WHERE vault_path = 'seed.md'",
        [],
    )
    .unwrap();
    assert_eq!(active_paths(&conn), vec!["Templates/Daily.md", "keep.md"]);

    let deleted = reconcile_ignored_notes(&mut conn, &IndexerConfig::index_all()).unwrap();
    assert_eq!(deleted, 1);
    assert_eq!(active_paths(&conn), vec!["keep.md"]);

    drop(conn);
    cleanup_db(&db);
    fs::remove_dir_all(&vault).unwrap();
}
