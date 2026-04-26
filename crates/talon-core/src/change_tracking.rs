//! Change tracking types and tombstone management.
//!
//! Tracks file lifecycle: indexed, modified, deleted. Uses mtime comparison
///  and tombstone tables for change detection and `--since` queries.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    #[allow(clippy::cast_possible_truncation)]
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
            active_notes: self.states.values().filter(|s| s.is_active()).count() as u32,
            chunk_count: 0, // Computed separately by indexer
            tombstone_count: self.tombstones.len() as u32,
        }
    }
}

/// Parses an `--since` timestamp string.
///
/// Accepts ISO 8601 format (e.g., `2024-01-15T10:30:00Z`) or milliseconds since epoch.
///
/// # Errors
///
/// Returns [`crate::TalonError::InvalidSince`] if the timestamp cannot be parsed.
///
/// # Panics
///
/// This function does not panic.
pub fn parse_since(timestamp: &str) -> Result<u64, crate::TalonError> {
    // Try parsing as milliseconds since epoch (numeric string)
    if let Ok(ms) = timestamp.parse::<u64>() {
        return Ok(ms);
    }

    // Try parsing as ISO 8601 / RFC 3339:
    // - 2024-01-15T10:30:00Z
    // - 2024-01-15T10:30:00+00:00
    if let Ok(dt) =
        time::OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
    {
        return Ok(unix_millis(dt));
    }

    // Try date-only format (YYYY-MM-DD); midnight UTC is implied.
    let date_format = time::macros::format_description!("[year]-[month]-[day]");
    if let Ok(date) = time::Date::parse(timestamp, date_format) {
        let dt = date
            .with_hms(0, 0, 0)
            .map(time::PrimitiveDateTime::assume_utc)
            .map_err(|err| crate::TalonError::InvalidSince {
                message: format!("00:00:00 is always valid (unreachable): {err}"),
            })?;
        return Ok(unix_millis(dt));
    }

    Err(crate::TalonError::InvalidSince {
        message: format!("unable to parse timestamp: {timestamp}"),
    })
}

/// Returns the current time in milliseconds since epoch.
#[must_use]
pub fn now_ms() -> u64 {
    unix_millis(time::OffsetDateTime::now_utc())
}

fn unix_millis(dt: time::OffsetDateTime) -> u64 {
    let nanos = dt.unix_timestamp_nanos();
    if nanos < 0 {
        return 0;
    }
    let millis = nanos / 1_000_000;
    u64::try_from(millis).unwrap_or(u64::MAX)
}

/// Default tombstone retention period: 90 days in milliseconds.
pub const TOMBSTONE_RETENTION_MS: u64 = 90 * 24 * 60 * 60 * 1000;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_file_change_state_active() {
        let state = FileChangeState::active("test.md".to_string(), 1000);
        assert!(state.is_active());
        assert!(!state.is_modified());
    }

    #[test]
    fn test_file_change_state_modified() {
        let mut state = FileChangeState::active("test.md".to_string(), 2000);
        state.mark_indexed(1000);
        assert!(state.is_modified());
    }

    #[test]
    fn test_file_change_state_tombstoned() {
        let mut state = FileChangeState::active("test.md".to_string(), 1000);
        state.tombstone(2000);
        assert!(!state.is_active());
        assert!(state.tombstoned);
        assert_eq!(state.tombstoned_at, Some(2000));
    }

    #[test]
    fn test_change_index_register_and_update() {
        let mut idx = ChangeIndex::default();
        idx.register_active("a.md".to_string(), 1000, 1000);
        idx.update_mtime("a.md", 2000);

        let state = idx.states.get("a.md").unwrap();
        assert_eq!(state.mtime, 2000);
        assert!(state.is_modified());
    }

    #[test]
    fn test_change_index_tombstone() {
        let mut idx = ChangeIndex::default();
        idx.register_active("a.md".to_string(), 1000, 1000);
        idx.tombstone("a.md", 2000);

        assert!(idx.states.get("a.md").unwrap().tombstoned);
        assert_eq!(idx.tombstones.len(), 1);
    }

    #[test]
    fn test_change_index_prune_tombstones() {
        let mut idx = ChangeIndex::default();
        idx.register_active("a.md".to_string(), 1000, 1000);
        idx.tombstone("a.md", 1000);

        // Prune tombstones older than 500ms (should prune)
        let pruned = idx.prune_tombstones(500, 2000);
        assert_eq!(pruned.len(), 1);
        assert!(idx.tombstones.is_empty());
    }

    #[test]
    fn test_parse_since_numeric() {
        assert_eq!(parse_since("1700000000000").unwrap(), 1_700_000_000_000);
    }

    #[test]
    fn test_parse_since_iso8601() {
        let result = parse_since("2024-01-15T10:30:00Z").unwrap();
        // Just verify it parses without error
        assert!(result > 0);
    }

    #[test]
    fn test_parse_since_date_only() {
        let result = parse_since("2024-01-15").unwrap();
        assert!(result > 0);
    }

    #[test]
    fn test_parse_since_invalid() {
        assert!(parse_since("not-a-timestamp").is_err());
    }

    #[test]
    fn test_change_feed_computation() {
        let mut idx = ChangeIndex::default();
        idx.register_active("a.md".to_string(), 1000, 1000);
        idx.register_active("b.md".to_string(), 2000, 2000);
        idx.update_mtime("b.md", 3000);

        let feed = idx.compute_change_feed(1500);

        // a.md was indexed before since, so not in feed
        // b.md was indexed after since and is modified
        assert!(feed.added.is_empty());
        assert_eq!(feed.modified.len(), 1);
        assert_eq!(feed.modified[0].path, "b.md");
    }
}
