use std::path::PathBuf;

use talon_core::TalonConfig;

/// Controls how auto-refresh handles an already-running sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshLockPolicy {
    /// Return an error when another Talon process owns the sync lock.
    ErrorIfBusy,
    /// Leave the existing index untouched and continue with a read query.
    SkipIfBusy,
}

/// Returns the advisory sync lock path for a configured database.
#[must_use]
pub fn sync_lock_path(config: &TalonConfig) -> PathBuf {
    config
        .db_path
        .parent()
        .map_or_else(|| PathBuf::from("sync.lock"), |p| p.join("sync.lock"))
}

/// Auto-refresh the index against on-disk state before running a query.
///
/// Mirrors `talon sync` minus the embed pass. Recently edited files become
/// searchable via BM25 immediately; semantic embeddings catch up on the next
/// explicit `talon sync`.
///
/// # Errors
///
/// Returns an error if the underlying refresh fails. When `policy` is
/// [`RefreshLockPolicy::SkipIfBusy`], a live sync lock is treated as a no-op
/// because another process is already moving the index forward.
pub fn refresh_index_if_needed(
    config: &TalonConfig,
    conn: &mut talon_core::Connection,
    fast: bool,
    policy: RefreshLockPolicy,
) -> eyre::Result<()> {
    if fast {
        return Ok(());
    }
    let lock_path = sync_lock_path(config);
    let indexer_config = talon_core::IndexerConfig {
        include_patterns: config.include_patterns.clone(),
        ignore_patterns: config.ignore_patterns.clone(),
        talon_config: Some(config.clone()),
    };
    match talon_core::refresh_index(
        conn,
        &config.vault_path,
        &lock_path,
        &indexer_config,
        &config.chunker,
    ) {
        Ok(_) => {}
        Err(talon_core::SyncError::LockBusy) if policy == RefreshLockPolicy::SkipIfBusy => {}
        Err(e) => return Err(eyre::eyre!("auto-refresh failed: {e}")),
    }
    Ok(())
}

/// Auto-refreshes when the caller already owns the sync lock.
///
/// # Errors
///
/// Returns an error if the underlying refresh fails.
pub fn refresh_index_with_lock(
    config: &TalonConfig,
    conn: &mut talon_core::Connection,
    lock: talon_core::SyncLock,
) -> eyre::Result<()> {
    let indexer_config = talon_core::IndexerConfig {
        include_patterns: config.include_patterns.clone(),
        ignore_patterns: config.ignore_patterns.clone(),
        talon_config: Some(config.clone()),
    };
    talon_core::refresh_index_locked(
        conn,
        &config.vault_path,
        &indexer_config,
        &config.chunker,
        lock,
    )
    .map_err(|e| eyre::eyre!("auto-refresh failed: {e}"))?;
    Ok(())
}
