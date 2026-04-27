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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VecEmbeddingStorage {
    Float,
    Int8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VecChunksSchema {
    dimensions: u32,
    storage: VecEmbeddingStorage,
}

/// Returns the `vec_chunks` virtual table's embedding dimension, parsed from
/// `sqlite_master.sql`, or `None` if the table does not exist.
#[must_use]
pub fn get_vec_chunks_dimensions(conn: &Connection) -> Option<u32> {
    get_vec_chunks_schema(conn).map(|schema| schema.dimensions)
}

fn get_vec_chunks_schema(conn: &Connection) -> Option<VecChunksSchema> {
    let sql: rusqlite::Result<String> = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?",
        params!["vec_chunks"],
        |row| row.get(0),
    );
    let sql = sql.ok()?;
    parse_schema_from_create_sql(&sql)
}

#[cfg(test)]
fn parse_dimensions_from_create_sql(sql: &str) -> Option<u32> {
    parse_schema_from_create_sql(sql).map(|schema| schema.dimensions)
}

fn parse_schema_from_create_sql(sql: &str) -> Option<VecChunksSchema> {
    let lower = sql.to_ascii_lowercase();
    let key = "embedding";
    let start = lower.find(key)?;
    let after = &sql[start + key.len()..];
    let after_lower = &lower[start + key.len()..];
    let trimmed = after_lower.trim_start();
    let storage = if trimmed.starts_with("float[") {
        VecEmbeddingStorage::Float
    } else if trimmed.starts_with("int8[") {
        VecEmbeddingStorage::Int8
    } else {
        return None;
    };
    let bracket_open = after.find('[')?;
    let bracket_close = after.find(']')?;
    if bracket_close <= bracket_open {
        return None;
    }
    let dimensions = after[bracket_open + 1..bracket_close].trim().parse().ok()?;
    Some(VecChunksSchema {
        dimensions,
        storage,
    })
}

/// Ensures the `vec_chunks` virtual table exists at the requested dimension.
///
/// Returns `true` if the table was created or recreated, `false` if it
/// already matched.
///
/// When the existing table has a different dimensionality or uses the old
/// float storage type, the function
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
    let current = get_vec_chunks_schema(conn);
    let expected = VecChunksSchema {
        dimensions,
        storage: VecEmbeddingStorage::Int8,
    };
    if current == Some(expected) {
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
            embedding int8[{dimensions}] distance_metric=cosine
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
#[path = "vec_ext_tests.rs"]
mod tests;
