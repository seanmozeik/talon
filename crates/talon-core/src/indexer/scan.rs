//! Full-vault scan and deletion reconciliation.

use std::path::Path;
use std::time::UNIX_EPOCH;

use fs_err as fs;
use rusqlite::Connection;

use crate::TalonError;
use crate::config::ChunkerConfig;
use crate::indexing::perform_note_deletion;

use super::prelude::{
    load_scan_notes_for_linking, matches_ignore_patterns, matches_include_patterns,
    scan_vault_markdown,
};
use super::wiring::index_one_note_with_config;

/// Configuration for a vault scan.
#[derive(Debug, Clone, Default)]
pub struct IndexerConfig {
    /// Glob-like include patterns. The wildcard `**/*.md` matches all
    /// markdown; substring patterns also work.
    pub include_patterns: Vec<String>,
    /// Substring ignore patterns layered on top of the built-in defaults
    /// (`.obsidian`, `.git`, `templates`, `.canvas`).
    pub ignore_patterns: Vec<String>,
}

impl IndexerConfig {
    /// Returns a config that indexes every markdown file with no extra
    /// ignore patterns. Useful as a default for top-level CLI invocations
    /// when no scope filtering is in play.
    #[must_use]
    pub fn index_all() -> Self {
        Self {
            include_patterns: vec!["**/*.md".into()],
            ignore_patterns: Vec::new(),
        }
    }
}

/// Counters returned by a vault scan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IndexerStats {
    /// Files (re)indexed during this scan.
    pub indexed: u32,
    /// Files skipped (filter mismatch or mtime+size unchanged).
    pub skipped: u32,
    /// Files reconciled away (present in DB, missing on disk).
    pub deleted: u32,
}

fn file_mtime_ms(path: &Path) -> Option<i64> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let dur = modified.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(dur.as_millis()).ok()
}

/// Walks `vault_root`, indexes any markdown file that matches the
/// include/ignore filters and whose `(mtime_ms, size_bytes)` differs from
/// the row in `notes`.
///
/// # Errors
///
/// Returns the first error from any per-note indexing call. Filesystem
/// errors during traversal are logged via `tracing::warn` and the affected
/// path is counted as `skipped`.
pub fn run_full_scan(
    conn: &mut Connection,
    vault_root: &Path,
    config: &IndexerConfig,
) -> Result<IndexerStats, TalonError> {
    run_full_scan_with_chunker(conn, vault_root, config, &ChunkerConfig::default())
}

/// Like [`run_full_scan`] but with an explicit [`ChunkerConfig`].
///
/// # Errors
///
/// Returns the first error from any per-note indexing call.
pub fn run_full_scan_with_chunker(
    conn: &mut Connection,
    vault_root: &Path,
    config: &IndexerConfig,
    chunker_config: &ChunkerConfig,
) -> Result<IndexerStats, TalonError> {
    let mut stats = IndexerStats::default();
    let mut linking_cache = load_scan_notes_for_linking(
        conn,
        vault_root,
        &config.include_patterns,
        &config.ignore_patterns,
    )
    .map_err(|source| TalonError::Sqlite {
        context: "load notes for link cache",
        source,
    })?;

    for rel_path in scan_vault_markdown(vault_root) {
        let included = matches_include_patterns(&rel_path, &config.include_patterns)
            && !matches_ignore_patterns(&rel_path, &config.ignore_patterns);
        if !included {
            stats.skipped = stats.skipped.saturating_add(1);
            continue;
        }

        let full_path = vault_root.join(&rel_path);
        let meta = match fs::metadata(&full_path) {
            Ok(m) => m,
            Err(err) => {
                tracing::warn!("talon scan: stat {} failed: {err}", full_path.display());
                stats.skipped = stats.skipped.saturating_add(1);
                continue;
            }
        };
        if !meta.is_file() {
            stats.skipped = stats.skipped.saturating_add(1);
            continue;
        }
        let size_bytes = i64::try_from(meta.len()).unwrap_or(i64::MAX);
        let Some(mtime_ms) = file_mtime_ms(&full_path) else {
            tracing::warn!("talon scan: mtime unavailable for {}", full_path.display());
            stats.skipped = stats.skipped.saturating_add(1);
            continue;
        };

        if existing_is_up_to_date(conn, &rel_path, mtime_ms, size_bytes) {
            stats.skipped = stats.skipped.saturating_add(1);
            continue;
        }

        let content = match fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(err) => {
                tracing::warn!("talon scan: read {} failed: {err}", full_path.display());
                stats.skipped = stats.skipped.saturating_add(1);
                continue;
            }
        };

        let outcome = index_one_note_with_config(
            conn,
            &rel_path,
            &content,
            mtime_ms,
            size_bytes,
            &linking_cache,
            chunker_config,
        )?;
        linking_cache = outcome.updated_links_cache;
        stats.indexed = stats.indexed.saturating_add(1);
    }

    Ok(stats)
}

fn existing_is_up_to_date(
    conn: &Connection,
    vault_path: &str,
    mtime_ms: i64,
    size_bytes: i64,
) -> bool {
    let row: Option<(i64, i64, i64)> = conn
        .query_row(
            "SELECT mtime_ms, size_bytes, active FROM notes WHERE vault_path = ?",
            [vault_path],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok();
    matches!(
        row,
        Some((existing_mtime, existing_size, 1)) if existing_mtime == mtime_ms && existing_size == size_bytes
    )
}

/// Soft-deletes any active note in the index whose source file no longer
/// exists under `vault_root`.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] if the row enumeration or any deletion
/// fails.
pub fn reconcile_deletions(conn: &mut Connection, vault_root: &Path) -> Result<u32, TalonError> {
    let active_paths: Vec<(i64, String)> = {
        let mut stmt = conn
            .prepare_cached("SELECT id, vault_path FROM notes WHERE active = 1")
            .map_err(|source| TalonError::Sqlite {
                context: "prepare active-notes lookup",
                source,
            })?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|source| TalonError::Sqlite {
                context: "query active notes",
                source,
            })?;
        rows.collect::<rusqlite::Result<_>>()
            .map_err(|source| TalonError::Sqlite {
                context: "read active notes",
                source,
            })?
    };

    let mut deleted: u32 = 0;
    for (note_id, vault_path) in active_paths {
        let full_path = vault_root.join(&vault_path);
        if !full_path.exists() {
            perform_note_deletion(conn, note_id, &vault_path)?;
            deleted = deleted.saturating_add(1);
        }
    }
    Ok(deleted)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::store::open_database;
    use std::env::temp_dir;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_dir(label: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        temp_dir().join(format!("talon-scan-test-{label}-{pid}-{n}"))
    }

    fn cleanup_db(path: &std::path::Path) {
        let _ = fs_err::remove_file(path);
        let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
    }

    fn write_note(root: &std::path::Path, rel: &str, body: &str) {
        let full = root.join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&full, body).unwrap();
    }

    #[test]
    fn full_scan_indexes_every_markdown_file() {
        let vault = unique_dir("full");
        fs::create_dir_all(&vault).unwrap();
        write_note(&vault, "a.md", "# A\nbody a");
        write_note(&vault, "zone/b.md", "# B\nbody b");
        write_note(&vault, "zone/skip.txt", "ignored");

        let db = vault.join("idx.sqlite");
        let mut conn = open_database(&db).unwrap();
        let stats = run_full_scan(&mut conn, &vault, &IndexerConfig::index_all()).unwrap();

        assert_eq!(stats.indexed, 2);
        // Non-md files are filtered at the walkdir stage (matching the TS
        // `Bun.Glob('**/*.md')` behavior) so they never reach the per-file
        // counter — `skipped` only tracks files the scanner *considered*.
        assert_eq!(stats.deleted, 0);

        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM notes WHERE active = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(total, 2);

        drop(conn);
        cleanup_db(&db);
        fs::remove_dir_all(&vault).unwrap();
    }

    #[test]
    fn second_scan_skips_unchanged_files() {
        let vault = unique_dir("rescan");
        fs::create_dir_all(&vault).unwrap();
        write_note(&vault, "a.md", "# A");
        let db = vault.join("idx.sqlite");
        let mut conn = open_database(&db).unwrap();

        let first = run_full_scan(&mut conn, &vault, &IndexerConfig::index_all()).unwrap();
        assert_eq!(first.indexed, 1);

        let second = run_full_scan(&mut conn, &vault, &IndexerConfig::index_all()).unwrap();
        assert_eq!(second.indexed, 0);
        assert!(second.skipped >= 1);

        drop(conn);
        cleanup_db(&db);
        fs::remove_dir_all(&vault).unwrap();
    }

    #[test]
    fn ignore_patterns_skip_matching_paths() {
        let vault = unique_dir("ignore");
        fs::create_dir_all(&vault).unwrap();
        write_note(&vault, "keep.md", "# Keep");
        write_note(&vault, "templates/Daily.md", "# Template");
        let db = vault.join("idx.sqlite");
        let mut conn = open_database(&db).unwrap();

        let stats = run_full_scan(&mut conn, &vault, &IndexerConfig::index_all()).unwrap();
        assert_eq!(stats.indexed, 1);
        let active_paths: Vec<String> = conn
            .prepare_cached("SELECT vault_path FROM notes WHERE active = 1 ORDER BY vault_path")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(active_paths, vec!["keep.md".to_string()]);

        drop(conn);
        cleanup_db(&db);
        fs::remove_dir_all(&vault).unwrap();
    }

    #[test]
    fn reconcile_soft_deletes_missing_files() {
        let vault = unique_dir("reconcile");
        fs::create_dir_all(&vault).unwrap();
        write_note(&vault, "stay.md", "# Stay");
        write_note(&vault, "go.md", "# Go");
        let db = vault.join("idx.sqlite");
        let mut conn = open_database(&db).unwrap();
        run_full_scan(&mut conn, &vault, &IndexerConfig::index_all()).unwrap();

        fs::remove_file(vault.join("go.md")).unwrap();
        let deleted = reconcile_deletions(&mut conn, &vault).unwrap();
        assert_eq!(deleted, 1);

        let active_paths: Vec<String> = conn
            .prepare_cached("SELECT vault_path FROM notes WHERE active = 1")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(active_paths, vec!["stay.md".to_string()]);

        drop(conn);
        cleanup_db(&db);
        fs::remove_dir_all(&vault).unwrap();
    }
}
