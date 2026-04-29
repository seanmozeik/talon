use rusqlite::Connection;

use super::*;

fn fresh_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    run_migrations(&mut conn).unwrap();
    conn
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('table','view') AND name = ?",
            [name],
            |row| row.get(0),
        )
        .unwrap();
    count > 0
}

fn index_exists(conn: &Connection, name: &str) -> bool {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?",
            [name],
            |row| row.get(0),
        )
        .unwrap();
    count > 0
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap();
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    columns.iter().any(|name| name == column)
}

fn trigger_exists(conn: &Connection, name: &str) -> bool {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name = ?",
            [name],
            |row| row.get(0),
        )
        .unwrap();
    count > 0
}

#[test]
fn migrations_create_all_schema_tables() {
    let conn = fresh_db();
    for table in [
        "notes",
        "chunks",
        "links",
        "note_aliases",
        "note_tags",
        "note_frontmatter_fields",
        "settings",
        "db_meta",
        "event_log",
        "llm_cache",
        "vector_metadata",
        "notes_fts_bm25",
        "notes_fts_fuzzy",
    ] {
        assert!(table_exists(&conn, table), "missing table: {table}");
    }
}

#[test]
fn migrations_create_all_indexes() {
    let conn = fresh_db();
    for index in [
        "idx_links_to",
        "idx_chunks_note_chunk_index",
        "idx_note_aliases_alias_norm",
        "idx_note_tags_tag_norm",
        "idx_fm_field_value_norm",
        "idx_fm_field_type_value",
        "idx_notes_active_path",
        "idx_notes_hash",
        "idx_notes_docid",
        "idx_chunks_hash",
    ] {
        assert!(index_exists(&conn, index), "missing index: {index}");
    }
}

#[test]
fn migrations_create_all_triggers() {
    let conn = fresh_db();
    for trigger in ["notes_fts_ai", "notes_fts_au", "notes_fts_ad"] {
        assert!(trigger_exists(&conn, trigger), "missing trigger: {trigger}");
    }
}

#[test]
fn migrations_create_frontmatter_value_type_column() {
    let conn = fresh_db();
    assert!(column_exists(
        &conn,
        "note_frontmatter_fields",
        "value_type"
    ));
}

#[test]
fn migrations_seed_db_version_setting() {
    let conn = fresh_db();
    let value: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?",
            [DB_VERSION_KEY],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(value, "0");
}

#[test]
fn migrations_seed_db_version_metadata() {
    let conn = fresh_db();
    let value: String = conn
        .query_row(
            "SELECT value FROM db_meta WHERE key = ?",
            [DB_VERSION_KEY],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(value, "0");
}

#[test]
fn read_db_version_defaults_to_zero_without_metadata() {
    let conn = Connection::open_in_memory().unwrap();
    assert_eq!(read_db_version(&conn), 0);
}

#[test]
fn bump_db_version_increments_monotonically() {
    let conn = fresh_db();
    let first = bump_db_version(&conn).unwrap();
    let second = bump_db_version(&conn).unwrap();
    assert_eq!(first, 1);
    assert_eq!(second, 2);
    assert_eq!(read_db_version(&conn), 2);
}

#[test]
fn migrations_are_idempotent() {
    let mut conn = Connection::open_in_memory().unwrap();
    run_migrations(&mut conn).unwrap();
    // Re-running must succeed without errors and without duplicating the
    // seeded settings row (the INSERT OR IGNORE clause guarantees this).
    run_migrations(&mut conn).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM settings WHERE key = ?",
            [DB_VERSION_KEY],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    let meta_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM db_meta WHERE key = ?",
            [DB_VERSION_KEY],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(meta_count, 1);
}

#[test]
fn pragmas_are_set() {
    let conn = fresh_db();
    let busy: i64 = conn
        .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
        .unwrap();
    assert_eq!(busy, i64::from(TALON_SQLITE_BUSY_TIMEOUT_MS));

    let fk: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .unwrap();
    assert_eq!(fk, 1);

    // In-memory databases force `journal_mode = memory`; only assert WAL on
    // file-backed connections (covered by `store::tests`).
}

#[test]
fn fts_trigger_indexes_inserted_note() {
    let conn = fresh_db();
    conn.execute(
        "INSERT INTO notes
             (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
             VALUES (?, ?, '', '', ?, 0, 0, 'h', 'd', 1)",
        ["a.md", "Hello", "Hello world body"],
    )
    .unwrap();
    let fts_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM notes_fts_bm25 WHERE notes_fts_bm25 MATCH ?",
            ["world"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(fts_count, 1);
}

#[test]
fn fts_trigger_removes_deleted_note() {
    let conn = fresh_db();
    conn.execute(
        "INSERT INTO notes
             (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
             VALUES (?, ?, '', '', ?, 0, 0, 'h', 'd', 1)",
        ["a.md", "Hello", "Hello world body"],
    )
    .unwrap();
    conn.execute("DELETE FROM notes WHERE vault_path = ?", ["a.md"])
        .unwrap();
    let fts_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM notes_fts_bm25 WHERE notes_fts_bm25 MATCH ?",
            ["world"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(fts_count, 0);
}

#[test]
fn foreign_keys_cascade_chunks_on_note_delete() {
    let conn = fresh_db();
    conn.execute(
        "INSERT INTO notes
             (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
             VALUES (?, ?, '', '', ?, 0, 0, 'h', 'd', 1)",
        ["a.md", "Hello", "body"],
    )
    .unwrap();
    let note_id: i64 = conn
        .query_row("SELECT id FROM notes WHERE vault_path = ?", ["a.md"], |r| {
            r.get(0)
        })
        .unwrap();
    conn.execute(
        "INSERT INTO chunks
             (note_id, chunk_index, text, embedding_text, chunk_hash, token_estimate)
             VALUES (?, 0, 'body', 'body', 'ch', 1)",
        [note_id],
    )
    .unwrap();

    conn.execute("DELETE FROM notes WHERE id = ?", [note_id])
        .unwrap();
    let chunk_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE note_id = ?",
            [note_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(chunk_count, 0);
}
