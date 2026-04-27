use super::*;
use crate::store::open_database;
use rusqlite::params;
use std::env::temp_dir;
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_path(label: &str) -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    temp_dir().join(format!("talon-vec-test-{label}-{pid}-{n}.sqlite"))
}

fn cleanup(path: &std::path::Path) {
    let _ = fs_err::remove_file(path);
    let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
    let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
}

#[test]
fn parse_dimensions_handles_various_sql_shapes() {
    let sql = "CREATE VIRTUAL TABLE vec_chunks USING vec0(chunk_id INTEGER PRIMARY KEY, embedding float[768] distance_metric=cosine)";
    assert_eq!(parse_dimensions_from_create_sql(sql), Some(768));
    let lowered = sql.to_lowercase();
    assert_eq!(parse_dimensions_from_create_sql(&lowered), Some(768));
    let weird_spaces = "embedding   int8[ 1024 ] distance_metric=cosine";
    assert_eq!(parse_dimensions_from_create_sql(weird_spaces), Some(1024));
    assert_eq!(parse_dimensions_from_create_sql("nothing here"), None);
    assert_eq!(parse_dimensions_from_create_sql("embedding float[]"), None);
}

#[test]
fn parse_schema_detects_storage_type() {
    assert_eq!(
        parse_schema_from_create_sql("embedding float[384] distance_metric=cosine"),
        Some(VecChunksSchema {
            dimensions: 384,
            storage: VecEmbeddingStorage::Float,
        })
    );
    assert_eq!(
        parse_schema_from_create_sql("embedding int8[384] distance_metric=cosine"),
        Some(VecChunksSchema {
            dimensions: 384,
            storage: VecEmbeddingStorage::Int8,
        })
    );
}

#[test]
fn register_is_idempotent() {
    register_sqlite_vec().unwrap();
    register_sqlite_vec().unwrap();
    assert!(is_vec_registered());
}

#[test]
fn ensure_vec_chunks_creates_then_no_ops() {
    register_sqlite_vec().unwrap();
    let path = unique_path("create");
    let conn = open_database(&path).unwrap();
    let created_first = ensure_vec_chunks(&conn, 768).unwrap();
    assert!(created_first);
    assert_eq!(get_vec_chunks_dimensions(&conn), Some(768));
    assert_eq!(
        get_vec_chunks_schema(&conn),
        Some(VecChunksSchema {
            dimensions: 768,
            storage: VecEmbeddingStorage::Int8,
        })
    );
    let created_again = ensure_vec_chunks(&conn, 768).unwrap();
    assert!(!created_again);
    drop(conn);
    cleanup(&path);
}

#[test]
fn ensure_vec_chunks_rebuilds_float_schema_at_same_dimension() {
    register_sqlite_vec().unwrap();
    let path = unique_path("float-cutover");
    let conn = open_database(&path).unwrap();
    conn.execute(
        "CREATE VIRTUAL TABLE vec_chunks USING vec0(
            chunk_id INTEGER PRIMARY KEY,
            embedding float[384] distance_metric=cosine
         )",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
         VALUES ('a.md', 'A', '[]', '[]', '', 0, 0, 'h', 'd', 1)",
        [],
    )
    .unwrap();
    let note_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO chunks (note_id, chunk_index, text, embedding_text, heading_path, char_start, char_end, chunk_hash, token_estimate, embedding_status)
         VALUES (?, 0, 'body', 'body', '', 0, 4, 'h', 1, 'ok')",
        params![note_id],
    )
    .unwrap();
    let chunk_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO vector_metadata (chunk_id, model, dimensions, embedded_at_ms) VALUES (?, 'm', 384, 0)",
        params![chunk_id],
    )
    .unwrap();

    let rebuilt = ensure_vec_chunks(&conn, 384).unwrap();
    assert!(rebuilt);
    assert_eq!(
        get_vec_chunks_schema(&conn),
        Some(VecChunksSchema {
            dimensions: 384,
            storage: VecEmbeddingStorage::Int8,
        })
    );
    let metadata_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM vector_metadata", [], |r| r.get(0))
        .unwrap();
    assert_eq!(metadata_count, 0);
    let chunk_status: String = conn
        .query_row(
            "SELECT embedding_status FROM chunks WHERE id = ?",
            params![chunk_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(chunk_status, "pending");

    drop(conn);
    cleanup(&path);
}

#[test]
fn ensure_vec_chunks_rebuilds_on_dimension_change() {
    register_sqlite_vec().unwrap();
    let path = unique_path("resize");
    let conn = open_database(&path).unwrap();
    ensure_vec_chunks(&conn, 384).unwrap();
    conn.execute(
        "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
         VALUES ('a.md', 'A', '[]', '[]', '', 0, 0, 'h', 'd', 1)",
        [],
    )
    .unwrap();
    let note_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO chunks (note_id, chunk_index, text, embedding_text, heading_path, char_start, char_end, chunk_hash, token_estimate, embedding_status)
         VALUES (?, 0, 'body', 'body', '', 0, 4, 'h', 1, 'ok')",
        params![note_id],
    )
    .unwrap();
    let chunk_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO vector_metadata (chunk_id, model, dimensions, embedded_at_ms) VALUES (?, 'm', 384, 0)",
        params![chunk_id],
    )
    .unwrap();

    let rebuilt = ensure_vec_chunks(&conn, 768).unwrap();
    assert!(rebuilt);
    assert_eq!(get_vec_chunks_dimensions(&conn), Some(768));

    let metadata_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM vector_metadata", [], |r| r.get(0))
        .unwrap();
    assert_eq!(metadata_count, 0);
    let chunk_status: String = conn
        .query_row(
            "SELECT embedding_status FROM chunks WHERE id = ?",
            params![chunk_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(chunk_status, "pending");

    drop(conn);
    cleanup(&path);
}

#[test]
fn ensure_vec_chunks_rejects_zero_dimensions() {
    register_sqlite_vec().unwrap();
    let path = unique_path("zero");
    let conn = open_database(&path).unwrap();
    let err = ensure_vec_chunks(&conn, 0).unwrap_err();
    assert!(matches!(
        err,
        TalonError::InvalidInput {
            field: "dimensions",
            ..
        }
    ));
    drop(conn);
    cleanup(&path);
}
