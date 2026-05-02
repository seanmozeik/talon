//! Indexing tool output types.

use serde::{Deserialize, Serialize};

use crate::contracts::{ContainerPath, VaultPath};

use super::input::InspectCheck;

/// Sync status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SyncStatus {
    /// Sync completed.
    Ok,
    /// Sync partially completed.
    Partial,
    /// Sync failed.
    Failed,
    /// Sync busy (lock held by another process).
    Busy,
}

/// Sync response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
pub struct SyncResponse {
    /// Whether the sync completed.
    pub completed: bool,
    /// Status label.
    pub status: SyncStatus,
    /// Whether lexical-only mode was requested.
    pub fast: bool,
    /// Whether vector reset was requested.
    pub force: bool,
    /// Whether the index database was rebuilt before syncing.
    pub rebuild: bool,
    /// Number of paths in scope.
    pub path_count: u32,
    /// Indexed notes.
    pub indexed: u32,
    /// Skipped notes.
    pub skipped: u32,
    /// Deleted notes.
    pub deleted: u32,
    /// Notes embedded during this sync pass.
    pub embedded: u32,
    /// Notes that failed embedding.
    pub embed_failed: u32,
    /// True if any vector dimension differed mid-pass; semantic search is
    /// disabled until the next consistent pass succeeds.
    pub dimension_mismatch: bool,
    /// Operator-facing remediation hint (present when the embed pass detected
    /// a recoverable problem; e.g. dim mismatch tells the user to re-run
    /// with `--force`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_remediation: Option<String>,
    /// Up to 20 redacted detail strings from the embed pass.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub embed_diagnostics: Vec<String>,
    /// Graph artifact stats from sync-time graph rebuild.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<crate::graph::GraphBuildStats>,
    /// Duration in milliseconds.
    pub duration_ms: u64,
}

/// Status state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StatusState {
    /// Disabled by config.
    Disabled,
    /// Config is invalid.
    ConfigError,
    /// Ready to serve requests.
    Ready,
}

/// Index statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    /// Active notes.
    pub active_notes: u32,
    /// Indexed chunks.
    pub chunk_count: u32,
    /// Failed embeddings.
    pub failed_embeddings: u32,
    /// Vector dimensions, if known.
    pub vector_dimensions: Option<u16>,
}

/// Scope report from status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeReport {
    /// Total number of configured scopes.
    pub total_scopes: u32,
    /// Default scope names.
    pub default_scopes: Vec<String>,
    /// Files not matching any scope.
    pub unscoped_count: u32,
}

/// Status response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    /// Readiness state.
    pub state: StatusState,
    /// Whether Talon is enabled.
    pub enabled: bool,
    /// Optional reason for non-ready states.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Container mount path.
    pub container_mount: ContainerPath,
    /// Index version.
    pub index_version: String,
    /// Index statistics.
    pub index: IndexStats,
    /// Scope report.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<ScopeReport>,
    /// Resolved vault path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault_path: Option<String>,
    /// Config file path used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    /// Database path used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db_path: Option<String>,
}

/// A single inspect finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectFinding {
    /// Inspect check that produced this finding.
    pub check: InspectCheck,
    /// Vault-relative path of the file.
    pub path: VaultPath,
    /// Description of the finding.
    pub message: String,
    /// Line number, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// Inspect check response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectResponse {
    /// Vault root (absolute container path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault: Option<ContainerPath>,
    /// The check that was run.
    pub check: InspectCheck,
    /// Inspect findings.
    pub findings: Vec<InspectFinding>,
}
