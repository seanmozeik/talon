use super::*;
use crate::store::open_database;
use fs_err as fs;
use std::env::temp_dir;
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_dir(label: &str) -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    temp_dir().join(format!("talon-sync-test-{label}-{pid}-{n}"))
}

#[test]
fn run_sync_indexes_then_reconciles() {
    let vault = unique_dir("end-to-end");
    fs::create_dir_all(&vault).unwrap();
    fs::write(vault.join("a.md"), "# A").unwrap();
    fs::write(vault.join("b.md"), "# B").unwrap();
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");
    let mut conn = open_database(&db).unwrap();

    let (first, embed) = run_sync(
        &mut conn,
        &vault,
        &lock,
        &IndexerConfig::index_all(),
        None,
        None,
    )
    .unwrap();
    assert_eq!(first.indexed, 2);
    assert_eq!(first.deleted, 0);
    assert!(embed.is_none());

    fs::remove_file(vault.join("b.md")).unwrap();
    let (second, _) = run_sync(
        &mut conn,
        &vault,
        &lock,
        &IndexerConfig::index_all(),
        None,
        None,
    )
    .unwrap();
    assert_eq!(second.indexed, 0);
    assert_eq!(second.deleted, 1);

    let active: i64 = conn
        .query_row("SELECT COUNT(*) FROM notes WHERE active = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(active, 1);

    drop(conn);
    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(db.with_extension("sqlite-wal"));
    let _ = fs::remove_file(db.with_extension("sqlite-shm"));
    fs::remove_dir_all(&vault).unwrap();
}

#[test]
fn run_sync_reconciles_renamed_file_without_orphan_chunks() {
    let vault = unique_dir("rename");
    fs::create_dir_all(&vault).unwrap();
    fs::write(
        vault.join("old.md"),
        "# Stable Title\n\nA paragraph with enough text to produce a searchable chunk.",
    )
    .unwrap();
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");
    let mut conn = open_database(&db).unwrap();

    let (first, _) = run_sync(
        &mut conn,
        &vault,
        &lock,
        &IndexerConfig::index_all(),
        None,
        None,
    )
    .unwrap();
    assert_eq!(first.indexed, 1);
    assert_eq!(first.deleted, 0);

    fs::rename(vault.join("old.md"), vault.join("new.md")).unwrap();
    let (second, _) = run_sync(
        &mut conn,
        &vault,
        &lock,
        &IndexerConfig::index_all(),
        None,
        None,
    )
    .unwrap();
    assert_eq!(second.indexed, 1);
    assert_eq!(second.deleted, 1);

    let active_paths: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT vault_path FROM notes WHERE active = 1 ORDER BY vault_path")
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    assert_eq!(active_paths, vec!["new.md"]);

    let old_chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM chunks
             JOIN notes ON notes.id = chunks.note_id
             WHERE notes.vault_path = 'old.md'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(old_chunks, 0);

    let new_chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM chunks
             JOIN notes ON notes.id = chunks.note_id
             WHERE notes.vault_path = 'new.md'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(new_chunks > 0);

    drop(conn);
    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(db.with_extension("sqlite-wal"));
    let _ = fs::remove_file(db.with_extension("sqlite-shm"));
    fs::remove_dir_all(&vault).unwrap();
}
