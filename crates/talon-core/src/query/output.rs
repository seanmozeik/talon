//! Query tool output types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::contracts::{ContainerPath, VaultPath};

use super::related::RelationKind;

/// Read result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadResult {
    /// Whether the note was found.
    pub found: bool,
    /// Vault-relative path.
    pub vault_path: VaultPath,
    /// Optional note title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional note content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Outgoing links.
    #[serde(default)]
    pub links: Vec<String>,
    /// Backlinks.
    #[serde(default)]
    pub backlinks: Vec<String>,
    /// Tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Aliases.
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// Read response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadResponse {
    /// Vault root (absolute container path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault: Option<ContainerPath>,
    /// Read results.
    pub results: Vec<ReadResult>,
}

impl ReadResponse {
    /// Builds a stub read response for CLI scaffolding.
    #[must_use]
    pub const fn stub() -> Self {
        Self {
            vault: None,
            results: Vec::new(),
        }
    }
}

/// The recall sections bundled together.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultRecall {
    /// Top search results (hybrid pipeline output).
    pub active_notes: Vec<NoteExcerpt>,
    /// Notes reachable via link graph from `active_notes`.
    pub linked_context: Vec<LinkedNote>,
}

/// A note excerpt returned in `recall.active_notes`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteExcerpt {
    /// Vault-relative path.
    pub vault_path: VaultPath,
    /// Display title.
    pub title: String,
    /// Result snippet (with heading breadcrumb when available).
    pub snippet: String,
    /// Hybrid retrieval score (post-rerank, post-scope-multiplier).
    pub score: f64,
    /// 1-based rank within `active_notes`.
    pub rank: u32,
    /// Last modified date in "YYYY-MM-DD" format, empty when unavailable.
    pub mtime: String,
}

/// A note reachable via the link graph returned in `recall.linked_context`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedNote {
    /// Vault-relative path.
    pub vault_path: VaultPath,
    /// Display title.
    pub title: String,
    /// Raw link text that created this edge.
    pub link_text: String,
    /// Direction relative to the source note(s).
    pub relation: RelationKind,
    /// Number of graph hops from the top `active_note`.
    pub hops: u8,
}

/// Vault-native context recall response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallResponse {
    /// Vault root (absolute container path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault: Option<ContainerPath>,
    /// The five recall sections, or null when skipped by confidence gate.
    pub vault_recall: Option<VaultRecall>,
    /// Calibrated evidence quality score in [0, 1].
    pub evidence_score: f64,
    /// Estimated tokens used in the payload (≤ `budget_tokens` within ±2%).
    pub tokens_used: u32,
    /// Paths suppressed by `--exclude` before budget allocation.
    pub excluded: Vec<String>,
    /// Paths retrieved but dropped during greedy budget trimming.
    pub excluded_by_budget: Vec<String>,
    /// True when `evidence_score` < `min_confidence` or zero results returned.
    pub skipped: bool,
}

/// A single frontmatter entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaEntry {
    /// Vault-relative path.
    pub path: VaultPath,
    /// Frontmatter key-value pairs.
    pub frontmatter: BTreeMap<String, serde_json::Value>,
    /// Resolved scope name, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// File modification time as RFC 3339 / ISO 8601.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime: Option<String>,
}

/// Frontmatter query response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaResponse {
    /// Vault root (absolute container path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault: Option<ContainerPath>,
    /// Frontmatter entries.
    pub entries: Vec<MetaEntry>,
    /// Tag counts, if requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_counts: Option<BTreeMap<String, u32>>,
}

/// A change entry (added or modified).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeEntry {
    /// Vault-relative path.
    pub path: VaultPath,
    /// When this file was last indexed (RFC 3339 / ISO 8601).
    pub indexed_at: String,
}

/// A tombstone entry (deleted file).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TombstoneEntry {
    /// Vault-relative path.
    pub path: VaultPath,
    /// When the file was detected as deleted (RFC 3339 / ISO 8601).
    pub deleted_at: String,
}

/// Change feed response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangesResponse {
    /// Vault root (absolute container path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault: Option<ContainerPath>,
    /// Files newly indexed since the timestamp.
    pub added: Vec<ChangeEntry>,
    /// Files re-indexed since the timestamp.
    pub modified: Vec<ChangeEntry>,
    /// Files deleted (from tombstones).
    pub deleted: Vec<TombstoneEntry>,
}
