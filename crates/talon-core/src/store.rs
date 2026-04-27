//! Database open/close + migration entry point.
//!
//! Ports `store.ts` from the `TypeScript` reference. The core distinction is
//! that we use `rusqlite` with bundled `SQLite`, so the `macOS` `Homebrew` quirk
//! that `setCustomSQLite()` worked around is gone for free.

use std::path::Path;

use fs_err as fs;
use rusqlite::{Connection, OpenFlags};

use crate::TalonError;
use crate::indexing::migrations::run_migrations;

/// Opens (or creates) the Talon index database at `path` with the standard
/// PRAGMA configuration and applies all migrations.
///
/// Creates parent directories if they do not exist.
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

    run_migrations(&mut conn)?;
    Ok(conn)
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
    fn open_database_runs_migrations() {
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
}
