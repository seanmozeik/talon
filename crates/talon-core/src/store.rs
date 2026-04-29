//! Database open/close + migration entry point.
//!
//! Ports `store.ts` from the `TypeScript` reference. The core distinction is
//! that we use `rusqlite` with bundled `SQLite`, so the `macOS` `Homebrew` quirk
//! that `setCustomSQLite()` worked around is gone for free.

use std::path::Path;

use fs_err as fs;
use rusqlite::{Connection, OpenFlags};

use crate::TalonError;
use crate::indexing::migrations::{TALON_SQLITE_BUSY_TIMEOUT_MS, run_migrations};

/// Opens (or creates) the Talon index database at `path`.
///
/// Creates parent directories if they do not exist. The current schema is
/// initialized only when the database has no Talon schema yet; existing indexes
/// are opened without running DDL.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] for SQLite-level failures and
/// [`TalonError::Internal`] for filesystem failures (e.g. `mkdir -p` failing
/// on the parent directory).
pub fn open_database(path: &Path) -> Result<Connection, TalonError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|err| TalonError::Internal {
            message: format!(
                "creating parent directory {} failed: {err}",
                parent.display()
            ),
        })?;
    }

    let mut conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|source| TalonError::Sqlite {
        context: "open database",
        source,
    })?;

    apply_connection_pragmas(&conn)?;
    if !has_talon_schema(&conn)? {
        run_migrations(&mut conn)?;
    }
    Ok(conn)
}

/// Opens an existing Talon index database for read-only query work.
///
/// Does not create parent directories and does not run migrations. Callers that
/// need to create or refresh the index should use [`open_database`].
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] when the database cannot be opened read-only
/// or the read-only connection PRAGMAs cannot be applied.
pub fn open_database_read_only(path: &Path) -> Result<Connection, TalonError> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|source| TalonError::Sqlite {
        context: "open database read-only",
        source,
    })?;

    apply_query_pragmas(&conn)?;
    Ok(conn)
}

fn apply_connection_pragmas(conn: &Connection) -> Result<(), TalonError> {
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|source| TalonError::Sqlite {
            context: "set journal_mode",
            source,
        })?;
    apply_common_pragmas(conn)
}

fn apply_query_pragmas(conn: &Connection) -> Result<(), TalonError> {
    apply_common_pragmas(conn)?;
    conn.pragma_update(None, "query_only", "ON")
        .map_err(|source| TalonError::Sqlite {
            context: "set query_only",
            source,
        })?;
    Ok(())
}

fn apply_common_pragmas(conn: &Connection) -> Result<(), TalonError> {
    conn.pragma_update(None, "busy_timeout", TALON_SQLITE_BUSY_TIMEOUT_MS)
        .map_err(|source| TalonError::Sqlite {
            context: "set busy_timeout",
            source,
        })?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|source| TalonError::Sqlite {
            context: "set foreign_keys",
            source,
        })?;

    Ok(())
}

fn has_talon_schema(conn: &Connection) -> Result<bool, TalonError> {
    conn.query_row(
        "SELECT EXISTS (
           SELECT 1 FROM sqlite_master
           WHERE type = 'table' AND name = 'notes'
         )",
        [],
        |row| row.get::<_, bool>(0),
    )
    .map_err(|source| TalonError::Sqlite {
        context: "inspect talon schema",
        source,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::env::temp_dir;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_path(label: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        temp_dir().join(format!("talon-test-{label}-{pid}-{n}.sqlite"))
    }

    #[test]
    fn open_database_creates_file() {
        let path = unique_path("create");
        let conn = open_database(&path).unwrap();
        drop(conn);
        assert!(path.exists());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn open_database_creates_parent_dirs() {
        let nested = temp_dir()
            .join(format!("talon-test-parent-{}", std::process::id()))
            .join("a")
            .join("b")
            .join("idx.sqlite");
        let _ = fs::remove_dir_all(nested.parent().unwrap().parent().unwrap().parent().unwrap());

        let conn = open_database(&nested).unwrap();
        drop(conn);
        assert!(nested.exists());
        let _ = fs::remove_dir_all(nested.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn open_database_enables_wal_on_file() {
        let path = unique_path("wal");
        let conn = open_database(&path).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        drop(conn);
        let _ = fs::remove_file(&path);
        // WAL mode leaves -wal/-shm sidecar files; clean those up too.
        let _ = fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs::remove_file(path.with_extension("sqlite-shm"));
    }

    #[test]
    fn open_database_initializes_schema_when_missing() {
        let path = unique_path("migrated");
        let conn = open_database(&path).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'notes'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        drop(conn);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs::remove_file(path.with_extension("sqlite-shm"));
    }

    #[test]
    fn reopening_existing_database_succeeds() {
        let path = unique_path("reopen");
        let conn = open_database(&path).unwrap();
        drop(conn);
        // Reopen and confirm the migrations are idempotent.
        let conn = open_database(&path).unwrap();
        let value: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'db_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(value, "0");
        drop(conn);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs::remove_file(path.with_extension("sqlite-shm"));
    }

    #[test]
    fn reopening_existing_database_does_not_reinitialize_schema() {
        let path = unique_path("no-reinit");
        let conn = open_database(&path).unwrap();
        conn.execute("DROP TRIGGER notes_fts_ai", []).unwrap();
        drop(conn);

        let conn = open_database(&path).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name = 'notes_fts_ai'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        drop(conn);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs::remove_file(path.with_extension("sqlite-shm"));
    }

    #[test]
    fn open_database_read_only_opens_existing_database_without_writes() {
        let path = unique_path("readonly");
        let conn = open_database(&path).unwrap();
        drop(conn);

        let conn = open_database_read_only(&path).unwrap();
        let result = conn.execute(
            "INSERT INTO settings (key, value) VALUES ('readonly-test', '1')",
            [],
        );

        assert!(result.is_err(), "read-only connection should reject writes");
        drop(conn);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs::remove_file(path.with_extension("sqlite-shm"));
    }

    #[test]
    fn open_database_read_only_does_not_create_missing_database() {
        let path = unique_path("readonly-missing");

        let result = open_database_read_only(&path);

        assert!(
            result.is_err(),
            "read-only open should require an existing database"
        );
        assert!(!path.exists());
    }
}
