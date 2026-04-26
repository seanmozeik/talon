//! Sync orchestration: advisory lock, full scan, reconcile.
//!
//! Ports `services/talon/sync/*.ts`. Owners of multiple [`crate::indexer`]
//! invocations that should run as a single logical "sync" go through here:
//! the [`lock`] module ensures only one Talon process touches the index at
//! a time across PIDs, and [`run_sync`] drives the full
//! scan + reconcile + tombstone-prune pipeline.

pub mod lock;

use std::path::Path;

use rusqlite::Connection;
use time::OffsetDateTime;

use crate::TalonError;
use crate::change_tracking::TOMBSTONE_RETENTION_MS;
use crate::embed::{EmbedPassOptions, EmbedPassStats, run_embed_pass};
use crate::indexer::{IndexerConfig, IndexerStats, reconcile_deletions, run_full_scan};
use crate::inference::InferenceClient;

pub use lock::{SyncLock, SyncLockError, acquire_sync_lock, is_sync_lock_held_by_live_process};

/// One-shot sync over a vault.
///
/// Holds [`SyncLock`] for the duration of the call so concurrent Talon
/// processes serialize. Runs the full scan, then reconciles deletions, then
/// (best-effort) prunes tombstones older than [`TOMBSTONE_RETENTION_MS`],
/// then optionally runs the embed pass.
///
/// When `embed_config` and `inference` are both `Some`, the embed pass runs
/// after reconciliation. When either is `None` (e.g. `--fast` mode), the
/// embed pass is skipped entirely.
///
/// # Errors
///
/// Returns [`SyncError::LockBusy`] if another process holds the lock,
/// [`SyncError::Indexer`] if the underlying scan or reconcile fails,
/// [`SyncError::Embed`] if the embed pass fails, and
/// [`SyncError::Lock`] if the lock file itself cannot be created/removed.
pub fn run_sync(
    conn: &mut Connection,
    vault_root: &Path,
    lock_path: &Path,
    config: &IndexerConfig,
    embed_config: Option<EmbedPassOptions>,
    inference: Option<&InferenceClient>,
) -> Result<(IndexerStats, Option<EmbedPassStats>), SyncError> {
    let _lock = acquire_sync_lock(lock_path).map_err(SyncError::from_lock)?;
    let mut stats = run_full_scan(conn, vault_root, config).map_err(SyncError::Indexer)?;
    let deleted = reconcile_deletions(conn, vault_root).map_err(SyncError::Indexer)?;
    stats.deleted = stats.deleted.saturating_add(deleted);

    // Tombstone state currently lives in the in-memory `ChangeIndex` (see
    // `crate::change_tracking`); the persistent change-feed table will land in
    // Phase 5 alongside `query::changes`. The constants below are referenced
    // here so the eventual prune wiring has an obvious home.
    let _ = TOMBSTONE_RETENTION_MS;
    let _ = OffsetDateTime::now_utc();

    // Run embed pass after reconciliation if configured.
    let embed_stats = if let (Some(opts), Some(client)) = (embed_config, inference) {
        Some(run_embed_pass(conn, client, &opts).map_err(|e| SyncError::Embed(e.to_string()))?)
    } else {
        None
    };

    Ok((stats, embed_stats))
}

/// Errors returned by [`run_sync`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SyncError {
    /// Another Talon process holds the sync lock.
    #[error("sync lock is held by another process")]
    LockBusy,
    /// Lock-file IO failed for a reason other than contention.
    #[error("sync lock IO error: {0}")]
    Lock(#[source] std::io::Error),
    /// Indexer-side failure (DB or filesystem).
    #[error(transparent)]
    Indexer(#[from] TalonError),
    /// Embed-pass failure (HTTP error, dim mismatch, etc.).
    #[error("embed pass failed: {0}")]
    Embed(String),
}

impl SyncError {
    fn from_lock(err: SyncLockError) -> Self {
        match err {
            SyncLockError::Busy => Self::LockBusy,
            SyncLockError::Io(io) => Self::Lock(io),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
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

        // Remove one note and re-sync — reconciler should soft-delete it.
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
}
