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
    /// Container path.
    pub path: ContainerPath,
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
    /// Read results.
    pub results: Vec<ReadResult>,
}

impl ReadResponse {
    /// Builds a stub read response for CLI scaffolding.
    #[must_use]
    pub const fn stub() -> Self {
        Self {
            results: Vec::new(),
        }
    }
}

/// The five recall sections bundled together.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultRecall {
    /// Top search results (hybrid pipeline output).
    pub active_notes: Vec<NoteExcerpt>,
    /// Notes reachable via link graph from `active_notes`.
    pub linked_context: Vec<LinkedNote>,
    /// Frontmatter key-value facts from `active_notes`.
    pub frontmatter: Vec<FrontmatterFact>,
    /// Recently edited notes within the since window.
    pub recent_edits: Vec<EditedNote>,
    /// Fuzzy title/alias matches below the main score threshold.
    pub fuzzy_anchors: Vec<FuzzyAnchor>,
}

/// A fuzzy title/alias match returned in `recall.fuzzy_anchors`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuzzyAnchor {
    /// Vault-relative path.
    pub vault_path: VaultPath,
    /// Container-absolute path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<ContainerPath>,
    /// Display title.
    pub title: String,
    /// Matching snippet or alias text.
    pub snippet: String,
    /// Title/alias match score.
    pub match_score: f64,
}

/// A single frontmatter key-value pair returned in `recall.frontmatter`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontmatterFact {
    /// Vault-relative path of the containing note.
    pub vault_path: VaultPath,
    /// Container-absolute path of the containing note.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<ContainerPath>,
    /// Frontmatter key.
    pub key: String,
    /// Frontmatter value.
    pub value: serde_json::Value,
}

/// A note excerpt returned in `recall.active_notes`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteExcerpt {
    /// Vault-relative path.
    pub vault_path: VaultPath,
    /// Container-absolute path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<ContainerPath>,
    /// Display title.
    pub title: String,
    /// Result snippet (with heading breadcrumb when available).
    pub snippet: String,
    /// Hybrid retrieval score (post-rerank, post-scope-multiplier).
    pub score: f64,
    /// 1-based rank within `active_notes`.
    pub rank: u32,
}

/// A note reachable via the link graph returned in `recall.linked_context`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedNote {
    /// Vault-relative path.
    pub vault_path: VaultPath,
    /// Container-absolute path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<ContainerPath>,
    /// Display title.
    pub title: String,
    /// Raw link text that created this edge.
    pub link_text: String,
    /// Direction relative to the source note(s).
    pub relation: RelationKind,
    /// Number of graph hops from the top `active_note`.
    pub hops: u8,
}

/// A recently-edited note returned in `recall.recent_edits`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditedNote {
    /// Vault-relative path.
    pub vault_path: VaultPath,
    /// Container-absolute path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<ContainerPath>,
    /// Display title.
    pub title: String,
    /// When the note was last indexed (millis since epoch).
    pub indexed_at: u64,
    /// Days since last modification (fractional).
    pub days_since_modified: f64,
    /// Composite recency+relevance score used for ordering.
    pub score: f64,
}

/// Vault-native context recall response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallResponse {
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
}

/// Frontmatter query response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaResponse {
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
    /// When this file was last indexed (millis since epoch).
    pub indexed_at: u64,
}

/// A tombstone entry (deleted file).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TombstoneEntry {
    /// Vault-relative path.
    pub path: VaultPath,
    /// When the file was detected as deleted (millis since epoch).
    pub deleted_at: u64,
}

/// Change feed response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangesResponse {
    /// Files newly indexed since the timestamp.
    pub added: Vec<ChangeEntry>,
    /// Files re-indexed since the timestamp.
    pub modified: Vec<ChangeEntry>,
    /// Files deleted (from tombstones).
    pub deleted: Vec<TombstoneEntry>,
}
