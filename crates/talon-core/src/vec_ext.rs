//! Loadable `sqlite-vec` extension wiring.
//!
//! Ports `services/talon/sqlite-vec.ts`. Two responsibilities:
//!
//! - [`register_sqlite_vec`] registers the statically-linked `sqlite-vec`
//!   entry point with `SQLite`'s auto-extension table so every `rusqlite`
//!   `Connection` opened afterwards has `vec0` available. The TS port loaded
//!   the extension by file path because the `bun:sqlite` runtime does not
//!   statically link it; the Rust port links it via the `sqlite-vec` crate so
//!   no `Homebrew` quirk is needed.
//! - [`ensure_vec_chunks`] creates (or rebuilds at the new dimensionality) the
//!   `vec_chunks` virtual table and resets vector state when dimensions
//!   change, so swapping embedding models is a recoverable operation.

use std::ffi::c_char;
use std::sync::OnceLock;

use rusqlite::{Connection, params};

use crate::error::TalonError;

/// Returns true when the `sqlite-vec` extension has been globally registered
/// during the current process lifetime.
#[must_use]
pub fn is_vec_registered() -> bool {
    REGISTERED.get().copied().unwrap_or(false)
}

static REGISTERED: OnceLock<bool> = OnceLock::new();

/// Registers the `sqlite_vec` C entrypoint with `SQLite`'s auto-extension
/// table.
///
/// Safe to call repeatedly — registration only happens once per process.
/// After this returns, every `Connection::open*` call will have the `vec0`
/// virtual table module and `vec_*` SQL functions available.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] if the underlying `sqlite3_auto_extension`
/// call fails.
pub fn register_sqlite_vec() -> Result<(), TalonError> {
    if is_vec_registered() {
        return Ok(());
    }
    register_via_ffi()?;
    let _ = REGISTERED.set(true);
    Ok(())
}

#[expect(
    unsafe_code,
    reason = "registering a statically-linked SQLite extension entry point requires FFI; the call is idempotent and the function pointer is a `const` from `sqlite-vec`"
)]
fn register_via_ffi() -> Result<(), TalonError> {
    // SAFETY: `sqlite3_vec_init` is a `pub fn` exported from the
    // statically-linked `sqlite-vec` C source (see `sqlite_vec` crate). We
    // transmute its address to the C `xEntryPoint` function pointer type that
    // `sqlite3_auto_extension` expects (Sqlite's loadable-extension entry
    // signature: `(*mut sqlite3, *mut *mut c_char, *const sqlite3_api_routines) -> i32`).
    // `sqlite_vec` declares the upstream symbol with no parameters because
    // its rust binding predates the strict ffi typing in newer rusqlite; the
    // C ABI passes the same registers either way, so the call is sound.
    // `sqlite3_auto_extension` mutates SQLite's global auto-extension table,
    // which SQLite serializes internally.
    type EntryPoint = unsafe extern "C" fn(
        *mut rusqlite::ffi::sqlite3,
        *mut *mut c_char,
        *const rusqlite::ffi::sqlite3_api_routines,
    ) -> i32;
    let rc = unsafe {
        let entry: EntryPoint =
            std::mem::transmute::<*const (), EntryPoint>(sqlite_vec::sqlite3_vec_init as *const ());
        rusqlite::ffi::sqlite3_auto_extension(Some(entry))
    };
    if rc == rusqlite::ffi::SQLITE_OK {
        Ok(())
    } else {
        Err(TalonError::Internal {
            message: format!("sqlite3_auto_extension failed with rc={rc}"),
        })
    }
}

/// Drops `vec_chunks` if it exists; safe even if the table is absent.
fn drop_vec_chunks(conn: &Connection) -> Result<(), TalonError> {
    conn.execute("DROP TABLE IF EXISTS vec_chunks", [])
        .map_err(|source| TalonError::Sqlite {
            context: "drop vec_chunks",
            source,
        })?;
    Ok(())
}

/// Clears the dimension-tracking metadata so a fresh embed pass can repopulate.
fn clear_vector_metadata(conn: &Connection) -> Result<(), TalonError> {
    conn.execute("DELETE FROM vector_metadata", [])
        .map_err(|source| TalonError::Sqlite {
            context: "clear vector_metadata",
            source,
        })?;
    Ok(())
}

/// Marks every chunk on an active note as `pending` so the next embed pass
/// re-encodes them at the new dimensionality.
fn mark_active_chunks_pending(conn: &Connection) -> Result<(), TalonError> {
    conn.execute(
        "UPDATE chunks SET embedding_status = 'pending'
         WHERE note_id IN (SELECT id FROM notes WHERE active = 1)",
        [],
    )
    .map_err(|source| TalonError::Sqlite {
        context: "mark chunks pending",
        source,
    })?;
    Ok(())
}

/// Returns the `vec_chunks` virtual table's embedding dimension, parsed from
/// `sqlite_master.sql`, or `None` if the table does not exist.
#[must_use]
pub fn get_vec_chunks_dimensions(conn: &Connection) -> Option<u32> {
    let sql: rusqlite::Result<String> = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?",
        params!["vec_chunks"],
        |row| row.get(0),
    );
    let sql = sql.ok()?;
    parse_dimensions_from_create_sql(&sql)
}

fn parse_dimensions_from_create_sql(sql: &str) -> Option<u32> {
    let lower = sql.to_ascii_lowercase();
    let key = "embedding";
    let start = lower.find(key)?;
    let after = &sql[start + key.len()..];
    let bracket_open = after.find('[')?;
    let bracket_close = after.find(']')?;
    if bracket_close <= bracket_open {
        return None;
    }
    after[bracket_open + 1..bracket_close].trim().parse().ok()
}

/// Ensures the `vec_chunks` virtual table exists at the requested dimension.
///
/// Returns `true` if the table was created or recreated, `false` if it
/// already matched.
///
/// When the existing table has a different dimensionality, the function
/// drops `vec_chunks`, clears `vector_metadata`, and marks every active
/// chunk as `pending` so the next embed pass repopulates the index. This
/// makes swapping embedding models a recoverable operation rather than a
/// schema migration.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] for any underlying DDL or DML failure, and
/// [`TalonError::InvalidInput`] if `dimensions` is zero.
pub fn ensure_vec_chunks(conn: &Connection, dimensions: u32) -> Result<bool, TalonError> {
    if dimensions == 0 {
        return Err(TalonError::InvalidInput {
            field: "dimensions",
            message: "vec_chunks dimensions must be a positive integer".to_string(),
        });
    }
    let current = get_vec_chunks_dimensions(conn);
    if current == Some(dimensions) {
        return Ok(false);
    }
    if current.is_some() {
        drop_vec_chunks(conn)?;
        clear_vector_metadata(conn)?;
        mark_active_chunks_pending(conn)?;
    }
    let create_sql = format!(
        "CREATE VIRTUAL TABLE vec_chunks USING vec0(
            chunk_id INTEGER PRIMARY KEY,
            embedding float[{dimensions}] distance_metric=cosine
         )"
    );
    conn.execute(&create_sql, [])
        .map_err(|source| TalonError::Sqlite {
            context: "create vec_chunks",
            source,
        })?;
    Ok(true)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::store::open_database;
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
        let weird_spaces = "embedding   float[ 1024 ] distance_metric=cosine";
        assert_eq!(parse_dimensions_from_create_sql(weird_spaces), Some(1024));
        assert_eq!(parse_dimensions_from_create_sql("nothing here"), None);
        assert_eq!(parse_dimensions_from_create_sql("embedding float[]"), None);
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
        let created_again = ensure_vec_chunks(&conn, 768).unwrap();
        assert!(!created_again);
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn ensure_vec_chunks_rebuilds_on_dimension_change() {
        register_sqlite_vec().unwrap();
        let path = unique_path("resize");
        let conn = open_database(&path).unwrap();
        ensure_vec_chunks(&conn, 384).unwrap();
        // Insert some metadata + chunk that should be reset on rebuild.
        conn.execute(
            "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
             VALUES ('a.md', 'A', '[]', '[]', '', 0, 0, 'h', 'd', 1)",
            [],
        ).unwrap();
        let note_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO chunks (note_id, chunk_index, text, embedding_text, heading_path, char_start, char_end, chunk_hash, token_estimate, embedding_status)
             VALUES (?, 0, 'body', 'body', '', 0, 4, 'h', 1, 'ok')",
            params![note_id],
        ).unwrap();
        let chunk_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO vector_metadata (chunk_id, model, dimensions, embedded_at_ms) VALUES (?, 'm', 384, 0)",
            params![chunk_id],
        ).unwrap();

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
}
