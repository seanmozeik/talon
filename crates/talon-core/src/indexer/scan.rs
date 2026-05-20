//! Full-vault scan and deletion reconciliation.

mod freshness;

use std::collections::HashSet;
use std::path::Path;

use fs_err as fs;
use rusqlite::Connection;
use walkdir::WalkDir;

use crate::TalonError;
use crate::config::{ChunkerConfig, TalonConfig};
use crate::graph::GraphBuildStats;
use crate::indexing::perform_note_deletion;

use super::prelude::{
    build_ignore_globset, build_include_globset, file_matches_ignore, file_matches_include,
    hash_file_content, load_notes_for_linking, scan_vault_markdown,
};
use super::wiring::{NoteIndexConfig, index_one_note_with_config};
use crate::text::frontmatter::normalize_keyword;
use crate::text::normalize_vault_path;

use freshness::{existing_metadata_is_up_to_date, file_mtime_ms};

/// Configuration for a vault scan.
#[derive(Debug, Clone, Default)]
pub struct IndexerConfig {
    /// Glob-like include patterns. The wildcard `**/*.md` matches all
    /// markdown; substring patterns also work.
    pub include_patterns: Vec<String>,
    /// Substring ignore patterns layered on top of the built-in defaults
    /// (`.obsidian`, `.git`, `templates`, `.canvas`).
    pub ignore_patterns: Vec<String>,
    /// Optional Talon config used to resolve per-note scope names during indexing.
    pub talon_config: Option<TalonConfig>,
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
            talon_config: None,
        }
    }
}

/// Counters returned by a vault scan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IndexerStats {
    /// Files (re)indexed during this scan.
    pub indexed: u32,
    /// Files skipped (filter mismatch or content hash unchanged).
    pub skipped: u32,
    /// Files reconciled away (present in DB, missing on disk).
    pub deleted: u32,
    /// Graph artifact stats from the post-scan rebuild.
    pub graph: Option<GraphBuildStats>,
}

/// Walks `vault_root`, indexes any markdown file that matches the
/// include/ignore filters and whose content hash differs from the row in
/// `notes`.
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
/// Scope names are resolved from `config.talon_config` when present.
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
    let include_set = build_include_globset(&config.include_patterns).map_err(|message| {
        TalonError::InvalidInput {
            field: "include_patterns",
            message,
        }
    })?;
    let ignore_set = build_ignore_globset(&config.ignore_patterns).map_err(|message| {
        TalonError::InvalidInput {
            field: "ignore_patterns",
            message,
        }
    })?;
    let mut stats = IndexerStats::default();
    let mut ignored_link_targets: Option<HashSet<String>> = None;
    let mut linking_cache = None;

    for rel_path in scan_vault_markdown(vault_root) {
        let included = file_matches_include(&rel_path, &include_set)
            && !file_matches_ignore(&rel_path, &ignore_set);
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

        if existing_metadata_is_up_to_date(conn, &rel_path, mtime_ms, size_bytes) {
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

        if existing_is_up_to_date(conn, &rel_path, &content) {
            stats.skipped = stats.skipped.saturating_add(1);
            continue;
        }

        let ignored_link_targets = ignored_link_targets
            .get_or_insert_with(|| collect_ignored_link_targets(vault_root, &ignore_set));
        let linking_cache = if let Some(cache) = &mut linking_cache {
            cache
        } else {
            let cache = load_notes_for_linking(conn).map_err(|source| TalonError::Sqlite {
                context: "load notes for link cache",
                source,
            })?;
            linking_cache.insert(cache)
        };
        let note_config = NoteIndexConfig {
            chunker: chunker_config,
            talon_config: config.talon_config.as_ref(),
            ignored_link_targets: ignored_link_targets.clone(),
        };
        let outcome = index_one_note_with_config(
            conn,
            &rel_path,
            &content,
            mtime_ms,
            size_bytes,
            linking_cache,
            &note_config,
        )?;
        *linking_cache = outcome.updated_links_cache;
        stats.indexed = stats.indexed.saturating_add(1);
    }

    Ok(stats)
}

fn existing_is_up_to_date(conn: &Connection, vault_path: &str, content: &str) -> bool {
    let row: Option<(String, i64)> = conn
        .query_row(
            "SELECT hash, active FROM notes WHERE vault_path = ?",
            [vault_path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();
    let Some((existing_hash, 1)) = row else {
        return false;
    };
    existing_hash == hash_file_content(content)
}

fn collect_ignored_link_targets(
    vault_root: &Path,
    ignore_set: &globset::GlobSet,
) -> HashSet<String> {
    WalkDir::new(vault_root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let rel = entry.path().strip_prefix(vault_root).ok()?;
            let rel_path = rel.to_string_lossy().replace('\\', "/");
            file_matches_ignore(&rel_path, ignore_set).then_some(rel_path)
        })
        .flat_map(|rel_path| ignored_link_target_variants(&rel_path))
        .collect()
}

fn ignored_link_target_variants(rel_path: &str) -> Vec<String> {
    let normalized_path = normalize_keyword(&normalize_vault_path(rel_path));
    let mut variants = Vec::with_capacity(4);
    variants.push(normalized_path.clone());
    if let Some(path_stem) = normalized_path.rsplit_once('.').map(|(stem, _)| stem) {
        variants.push(path_stem.to_string());
    }
    if let Some(file_name) = normalized_path.rsplit('/').next() {
        variants.push(file_name.to_string());
        if let Some(file_stem) = file_name.rsplit_once('.').map(|(stem, _)| stem) {
            variants.push(file_stem.to_string());
        }
    }
    variants
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

/// Soft-deletes any active note that no longer passes the scan filters.
///
/// # Errors
///
/// Returns [`TalonError::InvalidInput`] for invalid glob patterns, or
/// [`TalonError::Sqlite`] if row enumeration or deletion fails.
pub fn reconcile_ignored_notes(
    conn: &mut Connection,
    config: &IndexerConfig,
) -> Result<u32, TalonError> {
    let include_set = build_include_globset(&config.include_patterns).map_err(|message| {
        TalonError::InvalidInput {
            field: "include_patterns",
            message,
        }
    })?;
    let ignore_set = build_ignore_globset(&config.ignore_patterns).map_err(|message| {
        TalonError::InvalidInput {
            field: "ignore_patterns",
            message,
        }
    })?;
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
        let included = file_matches_include(&vault_path, &include_set)
            && !file_matches_ignore(&vault_path, &ignore_set);
        if !included {
            perform_note_deletion(conn, note_id, &vault_path)?;
            deleted = deleted.saturating_add(1);
        }
    }
    Ok(deleted)
}

#[cfg(test)]
#[path = "scan_tests.rs"]
mod tests;
