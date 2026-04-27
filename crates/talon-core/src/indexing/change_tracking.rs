//! Change tracking types and tombstone management.
//!
//! Tracks file lifecycle: indexed, modified, deleted. Uses mtime comparison
///  and tombstone tables for change detection and `--since` queries.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod time;

pub use time::{TOMBSTONE_RETENTION_MS, now_ms, parse_since};

// ── Change tracking types ───────────────────────────────────────────────────

/// File state in the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileState {
    /// File is indexed and present.
    Active,
    /// File was deleted but tombstoned (for `--since` queries).
    Tombstoned,
}

/// A change entry in the change feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeEntry {
    /// Vault-relative path.
    pub path: String,
    /// When the file was last seen/indexed (milliseconds since epoch).
    pub last_indexed_at: u64,
    /// File state.
    pub state: FileState,
}

/// Change feed response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeFeed {
    /// Files newly indexed since the query timestamp.
    pub added: Vec<ChangeEntry>,
    /// Files re-indexed (modified) since the query timestamp.
    pub modified: Vec<ChangeEntry>,
    /// Files detected as deleted (tombstoned).
    pub deleted: Vec<ChangeEntry>,
}

/// Tombstone entry for deleted files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TombstoneEntry {
    /// Vault-relative path of the deleted file.
    pub path: String,
    /// When the file was detected as deleted (milliseconds since epoch).
    pub deleted_at: u64,
    /// When the file was last successfully indexed.
    pub last_indexed_at: u64,
}

/// Index metadata stored alongside the database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexMetadata {
    /// Schema version.
    pub schema_version: u32,
    /// When the index was last built (milliseconds since epoch).
    pub last_indexed_at: u64,
    /// When the index was last seen (file system check).
    pub last_seen_at: u64,
    /// Total number of active notes.
    pub active_notes: u32,
    /// Total number of chunks.
    pub chunk_count: u32,
    /// Total number of tombstones.
    pub tombstone_count: u32,
}

impl Default for IndexMetadata {
    fn default() -> Self {
        Self {
            schema_version: 1,
            last_indexed_at: 0,
            last_seen_at: 0,
            active_notes: 0,
            chunk_count: 0,
            tombstone_count: 0,
        }
    }
}

/// Change tracking state for a single file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChangeState {
    /// Vault-relative path.
    pub path: String,
    /// Last indexed timestamp (milliseconds since epoch).
    pub last_indexed_at: u64,
    /// Last seen timestamp (milliseconds since epoch).
    pub last_seen_at: u64,
    /// File modification time (milliseconds since epoch).
    pub mtime: u64,
    /// Whether the file is tombstoned.
    pub tombstoned: bool,
    /// When tombstoned, the deletion timestamp.
    pub tombstoned_at: Option<u64>,
}

#[allow(clippy::missing_const_for_fn)]
impl FileChangeState {
    /// Creates a new active file state.
    #[must_use]
    pub fn active(path: String, mtime: u64) -> Self {
        Self {
            path,
            last_indexed_at: 0,
            last_seen_at: 0,
            mtime,
            tombstoned: false,
            tombstoned_at: None,
        }
    }

    /// Marks the file as indexed.
    pub fn mark_indexed(&mut self, timestamp: u64) {
        self.last_indexed_at = timestamp;
        self.last_seen_at = timestamp;
    }

    /// Marks the file as seen (file system check).
    pub fn mark_seen(&mut self, timestamp: u64) {
        self.last_seen_at = timestamp;
    }

    /// Updates the mtime.
    pub fn update_mtime(&mut self, mtime: u64) {
        self.mtime = mtime;
    }

    /// Tombstones the file.
    pub fn tombstone(&mut self, timestamp: u64) {
        self.tombstoned = true;
        self.tombstoned_at = Some(timestamp);
    }

    /// Checks if the file has been modified since last indexed.
    ///
    /// Returns `false` for files that have never been indexed
    /// (`last_indexed_at == 0`). Use [`Self::last_indexed_at`] to distinguish
    /// "never indexed" from "indexed and unmodified".
    #[must_use]
    pub fn is_modified(&self) -> bool {
        self.last_indexed_at > 0 && self.mtime > self.last_indexed_at
    }

    /// Checks if the file is active (not tombstoned).
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.tombstoned
    }
}

/// Change tracking index: maps paths to their change state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeIndex {
    /// Maps path → change state.
    pub states: BTreeMap<String, FileChangeState>,
    /// Tombstoned files.
    pub tombstones: BTreeMap<String, TombstoneEntry>,
}

impl ChangeIndex {
    /// Registers a file as active.
    pub fn register_active(&mut self, path: String, mtime: u64, timestamp: u64) {
        let mut state = FileChangeState::active(path.clone(), mtime);
        state.mark_indexed(timestamp);
        state.mark_seen(timestamp);
        self.states.insert(path, state);
    }

    /// Updates a file's mtime.
    pub fn update_mtime(&mut self, path: &str, mtime: u64) {
        if let Some(state) = self.states.get_mut(path) {
            state.update_mtime(mtime);
            state.mark_seen(mtime);
        }
    }

    /// Marks a file as seen (file system check).
    pub fn mark_seen(&mut self, path: &str, timestamp: u64) {
        if let Some(state) = self.states.get_mut(path) {
            state.mark_seen(timestamp);
        }
    }

    /// Tombstones a deleted file.
    pub fn tombstone(&mut self, path: &str, timestamp: u64) {
        if let Some(state) = self.states.get_mut(path) {
            state.tombstone(timestamp);
            self.tombstones.insert(
                path.to_string(),
                TombstoneEntry {
                    path: path.to_string(),
                    deleted_at: timestamp,
                    last_indexed_at: state.last_indexed_at,
                },
            );
        }
    }

    /// Removes a file from the index (after tombstone cleanup).
    pub fn remove(&mut self, path: &str) {
        self.states.remove(path);
        self.tombstones.remove(path);
    }

    /// Gets files that have changed since the given timestamp.
    #[must_use]
    pub fn get_changes_since(&self, since: u64) -> (Vec<String>, Vec<String>) {
        let mut added = Vec::new();
        let mut modified = Vec::new();

        for (path, state) in &self.states {
            if state.last_indexed_at < since && state.last_seen_at >= since {
                if state.is_modified() {
                    modified.push(path.clone());
                } else {
                    added.push(path.clone());
                }
            }
        }

        added.sort();
        modified.sort();

        (added, modified)
    }

    /// Gets tombstoned files.
    #[must_use]
    pub fn get_tombstones(&self) -> Vec<&TombstoneEntry> {
        self.tombstones.values().collect()
    }

    /// Prunes tombstones older than the given age (in milliseconds).
    pub fn prune_tombstones(&mut self, max_age_ms: u64, current_time: u64) -> Vec<String> {
        let mut pruned = Vec::new();
        self.tombstones.retain(|path, entry| {
            if current_time - entry.deleted_at > max_age_ms {
                pruned.push(path.clone());
                false
            } else {
                true
            }
        });
        pruned
    }

    /// Computes change feed for `--since` queries.
    #[must_use]
    pub fn compute_change_feed(&self, since: u64) -> ChangeFeed {
        let mut added = Vec::new();
        let mut modified = Vec::new();
        let mut deleted = Vec::new();

        for (path, state) in &self.states {
            if state.last_seen_at >= since {
                let entry = ChangeEntry {
                    path: path.clone(),
                    last_indexed_at: state.last_indexed_at,
                    state: FileState::Active,
                };
                if state.is_modified() {
                    modified.push(entry);
                } else {
                    added.push(entry);
                }
            }
        }

        for (path, entry) in &self.tombstones {
            if entry.deleted_at >= since {
                deleted.push(ChangeEntry {
                    path: path.clone(),
                    last_indexed_at: entry.last_indexed_at,
                    state: FileState::Tombstoned,
                });
            }
        }

        added.sort_by_key(|e| e.path.clone());
        modified.sort_by_key(|e| e.path.clone());
        deleted.sort_by_key(|e| e.path.clone());

        ChangeFeed {
            added,
            modified,
            deleted,
        }
    }

    /// Returns index metadata.
    #[must_use]
    pub fn to_metadata(&self) -> IndexMetadata {
        IndexMetadata {
            schema_version: 1,
            last_indexed_at: self
                .states
                .values()
                .map(|s| s.last_indexed_at)
                .max()
                .unwrap_or(0),
            last_seen_at: self
                .states
                .values()
                .map(|s| s.last_seen_at)
                .max()
                .unwrap_or(0),
            active_notes: saturated_u32(self.states.values().filter(|s| s.is_active()).count()),
            chunk_count: 0,
            tombstone_count: saturated_u32(self.tombstones.len()),
        }
    }
}

fn saturated_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
