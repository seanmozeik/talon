//! Sync orchestration: advisory lock, full scan, reconcile.
//!
//! Ports `services/talon/sync/*.ts`. Owners of multiple [`crate::indexer`]
//! invocations that should run as a single logical "sync" go through here:
//! the [`lock`] module ensures only one Talon process touches the index at
//! a time across PIDs, and [`run_sync`] drives the full
//! scan + reconcile + tombstone-prune pipeline.

pub mod lock;
mod relink;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;

use std::path::Path;
use std::time::Instant;

use fs_err as fs;
use rusqlite::{Connection, OptionalExtension};
use time::OffsetDateTime;

use crate::TalonError;
use crate::config::ChunkerConfig;
use crate::embed::{EmbedPassOptions, EmbedPassStats, run_embed_pass};
use crate::graph::{GraphBuildInput, rebuild_graph};
use crate::indexer::{
    IndexerConfig, IndexerStats, reconcile_deletions, reconcile_ignored_notes,
    run_full_scan_with_chunker,
};
use crate::indexing::change_tracking::TOMBSTONE_RETENTION_MS;
use crate::indexing::migrations::read_db_version;
use crate::inference::InferenceClient;

pub use lock::{SyncLock, SyncLockError, acquire_sync_lock, is_sync_lock_held_by_live_process};
pub use relink::relink_unresolved;

/// Deletes the `SQLite` index database and companion WAL/SHM files.
///
/// Callers should hold the sync lock before invoking this so no other Talon
/// process can read or write the database while it is being replaced.
///
/// # Errors
///
/// Returns the first filesystem error other than `NotFound`.
pub fn remove_index_files(db_path: &Path) -> std::io::Result<()> {
    for path in [
        db_path.to_path_buf(),
        db_path.with_extension("sqlite-wal"),
        db_path.with_extension("sqlite-shm"),
    ] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

/// No-embed sync used by query surfaces to keep the index in step with
/// on-disk state without paying the embedding round-trip.
///
/// Equivalent to [`run_sync_with_chunker`] called with `embed_config = None`
/// and `inference = None` — runs the full scan, reconciles deletions, and
/// re-resolves links, then returns. Recently-edited files are searchable
/// via BM25 immediately; their semantic embeddings catch up on the next
/// explicit `talon sync`.
///
/// # Errors
///
/// Same as [`run_sync_with_chunker`]. Errors loudly on lock contention
/// (`SyncError::LockBusy`) — query commands should propagate this rather
/// than silently fall back to stale state.
pub fn refresh_index(
    conn: &mut Connection,
    vault_root: &Path,
    lock_path: &Path,
    config: &IndexerConfig,
    chunker: &ChunkerConfig,
) -> Result<IndexerStats, SyncError> {
    let (stats, _embed) =
        run_sync_with_chunker(conn, vault_root, lock_path, config, None, None, chunker)?;
    Ok(stats)
}

/// Like [`refresh_index`] when the caller already owns the sync lock.
///
/// This is useful for process entry points that need to serialize database
/// migrations before opening a write-capable connection.
///
/// # Errors
///
/// Same as [`refresh_index`], except lock acquisition has already happened.
pub fn refresh_index_locked(
    conn: &mut Connection,
    vault_root: &Path,
    config: &IndexerConfig,
    chunker: &ChunkerConfig,
    lock: SyncLock,
) -> Result<IndexerStats, SyncError> {
    let (stats, _embed) =
        run_sync_with_chunker_locked(conn, vault_root, config, None, None, chunker, lock)?;
    Ok(stats)
}

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
    run_sync_with_chunker(
        conn,
        vault_root,
        lock_path,
        config,
        embed_config,
        inference,
        &ChunkerConfig::default(),
    )
}

/// Like [`run_sync`] but with an explicit [`ChunkerConfig`].
///
/// Per-note scope names are resolved from `config.talon_config` when present.
///
/// # Errors
///
/// See [`run_sync`] for error variants.
pub fn run_sync_with_chunker(
    conn: &mut Connection,
    vault_root: &Path,
    lock_path: &Path,
    config: &IndexerConfig,
    embed_config: Option<EmbedPassOptions>,
    inference: Option<&InferenceClient>,
    chunker_config: &ChunkerConfig,
) -> Result<(IndexerStats, Option<EmbedPassStats>), SyncError> {
    let lock = acquire_sync_lock(lock_path).map_err(SyncError::from_lock)?;
    run_sync_with_chunker_locked(
        conn,
        vault_root,
        config,
        embed_config,
        inference,
        chunker_config,
        lock,
    )
}

/// Like [`run_sync_with_chunker`] when the caller already owns the sync lock.
///
/// # Errors
///
/// See [`run_sync`] for non-lock error variants.
pub fn run_sync_with_chunker_locked(
    conn: &mut Connection,
    vault_root: &Path,
    config: &IndexerConfig,
    embed_config: Option<EmbedPassOptions>,
    inference: Option<&InferenceClient>,
    chunker_config: &ChunkerConfig,
    _lock: SyncLock,
) -> Result<(IndexerStats, Option<EmbedPassStats>), SyncError> {
    let profile = RefreshProfile::start();
    let mut stats = run_full_scan_with_chunker(conn, vault_root, config, chunker_config)
        .map_err(SyncError::Indexer)?;
    profile.mark("scan");
    let deleted = reconcile_deletions(conn, vault_root).map_err(SyncError::Indexer)?;
    stats.deleted = stats.deleted.saturating_add(deleted);
    profile.mark("deletions");
    let ignored = reconcile_ignored_notes(conn, config).map_err(SyncError::Indexer)?;
    stats.deleted = stats.deleted.saturating_add(ignored);
    profile.mark("ignored");

    // Closes the link-staleness window: incremental indexing only refreshes
    // a source file's resolved links when that source file is touched, so an
    // alias added to a target leaves prior links to it unresolved until the
    // sources change. This pass re-resolves any link still pointing at a
    // missing `to_path` and lets the new aliases / new target files satisfy
    // existing references.
    let graph_version_before_relink = graph_db_version(conn).map_err(SyncError::Indexer)?;
    relink_unresolved(conn).map_err(SyncError::Indexer)?;
    profile.mark("relink");
    if graph_version_before_relink != Some(read_db_version(conn)) {
        stats.graph = Some(rebuild_graph(conn, &GraphBuildInput).map_err(SyncError::Indexer)?);
    }
    profile.mark("graph");

    // Tombstone state currently lives in the in-memory `ChangeIndex` (see
    // `crate::indexing::change_tracking`); the persistent change-feed table will land in
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
    profile.mark("embed");

    Ok((stats, embed_stats))
}

struct RefreshProfile {
    enabled: bool,
    started: Instant,
    previous: std::cell::Cell<Instant>,
}

impl RefreshProfile {
    fn start() -> Self {
        let started = Instant::now();
        Self {
            enabled: std::env::var_os("TALON_PROFILE").is_some(),
            started,
            previous: std::cell::Cell::new(started),
        }
    }

    fn mark(&self, stage: &str) {
        if !self.enabled {
            return;
        }
        let now = Instant::now();
        let previous = self.previous.replace(now);
        eprintln!(
            "talon profile refresh {stage}: stage={}ms total={}ms",
            previous.elapsed().as_millis(),
            self.started.elapsed().as_millis()
        );
    }
}

fn graph_db_version(conn: &Connection) -> Result<Option<u64>, TalonError> {
    let version = conn
        .query_row(
            "SELECT value FROM graph_meta WHERE key = 'db_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|source| TalonError::Sqlite {
            context: "read graph db version",
            source,
        })?;
    Ok(version.and_then(|value| value.parse().ok()))
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
