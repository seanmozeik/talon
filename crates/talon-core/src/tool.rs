//! MCP and CLI tool input/output contracts.

use crate::constants::{DEFAULT_LIMIT, RELATED_DEFAULT_DEPTH};
use crate::error::{ErrorCode, TalonError, TalonResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ── Positive count ──────────────────────────────────────────────────────────

/// A positive count accepted at the tool boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct PositiveCount(u16);

impl Default for PositiveCount {
    fn default() -> Self {
        Self(DEFAULT_LIMIT)
    }
}

impl PositiveCount {
    /// Builds a positive count.
    ///
    /// # Errors
    ///
    /// Returns [`TalonError::InvalidInput`] when `value` is zero.
    pub fn new(value: u16, field: &'static str) -> TalonResult<Self> {
        if value == 0 {
            return Err(TalonError::InvalidInput {
                field,
                message: "must be greater than zero".to_string(),
            });
        }
        Ok(Self(value))
    }

    /// Returns the primitive count.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for PositiveCount {
    type Error = TalonError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value, "count")
    }
}

impl From<PositiveCount> for u16 {
    fn from(value: PositiveCount) -> Self {
        value.0
    }
}

// ── Path types ──────────────────────────────────────────────────────────────

/// Vault-relative path returned by Talon.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VaultPath(String);

impl VaultPath {
    /// Parses a non-empty vault-relative path.
    ///
    /// # Errors
    ///
    /// Returns [`TalonError::InvalidInput`] when the path is empty.
    pub fn parse(value: impl Into<String>) -> TalonResult<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(TalonError::InvalidInput {
                field: "path",
                message: "must not be empty".to_string(),
            });
        }
        Ok(Self(value))
    }

    /// Returns the path as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Container-absolute path used when a tool needs absolute addressing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContainerPath(String);

impl ContainerPath {
    /// Parses a non-empty container path.
    ///
    /// # Errors
    ///
    /// Returns [`TalonError::InvalidInput`] when the path is empty.
    pub fn parse(value: impl Into<String>) -> TalonResult<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(TalonError::InvalidInput {
                field: "path",
                message: "must not be empty".to_string(),
            });
        }
        Ok(Self(value))
    }

    /// Returns the path as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ── Enums ───────────────────────────────────────────────────────────────────

/// Search mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SearchMode {
    /// Hybrid lexical plus semantic search.
    #[default]
    Hybrid,
    /// Semantic-only search.
    Semantic,
    /// Full-text search.
    Fulltext,
    /// Title and alias search.
    Title,
}

/// Related-note traversal direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    /// Outgoing wikilinks.
    Outgoing,
    /// Backlinks.
    Backlinks,
    /// Outgoing wikilinks and backlinks.
    #[default]
    Both,
}

/// Match provenance for a search result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MatchKind {
    /// Full-text match.
    Fulltext,
    /// Semantic/vector match.
    Semantic,
    /// Title match.
    Title,
    /// Alias match.
    Alias,
    /// Related-note match.
    Related,
}

/// Frontmatter filter accepted by search and related queries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FrontmatterFilter {
    /// Tag-like frontmatter shorthand.
    Text(String),
    /// Any-of string values.
    Texts(Vec<String>),
    /// Exact key/value matches.
    Fields(BTreeMap<String, FrontmatterValue>),
}

/// Frontmatter scalar values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FrontmatterValue {
    /// String value.
    Text(String),
    /// Numeric value.
    Number(f64),
    /// Boolean value.
    Boolean(bool),
}

/// Frontmatter value type for storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrontmatterValueType {
    String,
    Number,
    Bool,
    Date,
    List,
}

/// `--where` filter operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WhereOperator {
    Equals,
    NotEquals,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    Contains,
    Exists,
}

/// A single `--where` filter clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhereClause {
    /// Frontmatter key to filter on.
    pub key: String,
    /// Comparison operator.
    pub op: WhereOperator,
    /// Value to compare against (omitted for `exists`).
    pub value: Option<String>,
}

/// Lint check type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintCheck {
    /// Files with no incoming wikilinks.
    Orphans,
    /// Links whose targets don't resolve to indexed files.
    BrokenLinks,
    /// Frontmatter `sources:` pointing to non-existent paths.
    DanglingRefs,
    /// Files with no incoming AND no outgoing wikilinks.
    Unreferenced,
}

// ── Input types ─────────────────────────────────────────────────────────────

/// Tool input envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum TalonInput {
    /// Search request.
    Search(SearchInput),
    /// Read request.
    Read(ReadInput),
    /// Sync/index request.
    Sync(SyncInput),
    /// Status request.
    Status(StatusInput),
    /// Related-note request.
    Related(RelatedInput),
    /// Frontmatter query request.
    Meta(MetaInput),
    /// Change feed request.
    Changes(ChangesInput),
    /// Lint check request.
    Lint(LintInput),
}

/// Search request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchInput {
    /// Primary query.
    pub query: Option<String>,
    /// Batch queries.
    #[serde(default)]
    pub queries: Vec<String>,
    /// Search mode.
    #[serde(default)]
    pub mode: SearchMode,
    /// Lexical-only search when true.
    #[serde(default)]
    pub fast: bool,
    /// Result limit.
    #[serde(default)]
    pub limit: PositiveCount,
    /// Optional path scope.
    #[serde(default)]
    pub path: Option<String>,
    /// Optional tag scope.
    #[serde(default)]
    pub tag: Vec<String>,
    /// Optional frontmatter filter.
    #[serde(default)]
    pub frontmatter: Option<FrontmatterFilter>,
    /// Include related notes.
    #[serde(default)]
    pub related: bool,
    /// Related traversal depth.
    #[serde(default = "default_depth")]
    pub depth: u8,
    /// Related traversal direction.
    #[serde(default)]
    pub direction: Direction,
    /// Scope names to include (additive).
    #[serde(default)]
    pub scope: Vec<String>,
    /// Scope names to search exclusively (mutually exclusive with `scope`).
    #[serde(default)]
    pub scope_only: Vec<String>,
    /// Frontmatter `--where` filters (AND-composed).
    #[serde(default)]
    pub where_: Vec<WhereClause>,
    /// Filter results indexed since this timestamp.
    #[serde(default)]
    pub since: Option<String>,
}

impl Default for SearchInput {
    fn default() -> Self {
        Self {
            query: None,
            queries: Vec::new(),
            mode: SearchMode::Hybrid,
            fast: false,
            limit: PositiveCount(DEFAULT_LIMIT),
            path: None,
            tag: Vec::new(),
            frontmatter: None,
            related: false,
            depth: RELATED_DEFAULT_DEPTH,
            direction: Direction::Both,
            scope: Vec::new(),
            scope_only: Vec::new(),
            where_: Vec::new(),
            since: None,
        }
    }
}

impl SearchInput {
    /// Builds a CLI search request from validated command arguments.
    ///
    /// # Errors
    ///
    /// Returns [`TalonError::InvalidInput`] when `limit` is zero.
    pub fn from_cli_query(
        query: String,
        mode: SearchMode,
        fast: bool,
        limit: Option<u16>,
    ) -> TalonResult<Self> {
        let mut input = Self {
            query: Some(query),
            mode,
            fast,
            ..Self::default()
        };
        if let Some(limit) = limit {
            input.limit = PositiveCount::new(limit, "limit")?;
        }
        Ok(input)
    }
}

/// Read request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReadInput {
    /// Single path to read.
    pub path: Option<String>,
    /// Include raw content.
    #[serde(default)]
    pub raw: bool,
    /// First line to include.
    #[serde(default)]
    pub from_line: Option<PositiveCount>,
    /// Maximum lines to include.
    #[serde(default)]
    pub max_lines: Option<PositiveCount>,
}

/// Sync request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncInput {
    /// Specific paths to sync (empty = full pass).
    #[serde(default)]
    pub paths: Vec<String>,
    /// Skip embeddings (lexical-only pass).
    #[serde(default)]
    pub fast: bool,
    /// Reset vector state before syncing.
    #[serde(default)]
    pub force: bool,
    /// Return immediately if sync is already running.
    #[serde(default)]
    pub no_wait: bool,
}

/// Status request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StatusInput {
    /// Emit JSON output.
    #[serde(default)]
    pub json: bool,
}

/// Related-note request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedInput {
    /// Path to find related notes for.
    pub path: String,
    /// Graph traversal depth.
    #[serde(default = "default_depth")]
    pub depth: u8,
    /// Traversal direction.
    #[serde(default)]
    pub direction: Direction,
    /// Scope names to include.
    #[serde(default)]
    pub scope: Vec<String>,
    /// Scope names to search exclusively.
    #[serde(default)]
    pub scope_only: Vec<String>,
}

/// Frontmatter query request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaInput {
    /// Frontmatter `--where` filters (AND-composed).
    #[serde(default)]
    pub where_: Vec<WhereClause>,
    /// Filter results indexed since this timestamp.
    #[serde(default)]
    pub since: Option<String>,
    /// Scope names to include.
    #[serde(default)]
    pub scope: Vec<String>,
    /// Scope names to search exclusively.
    #[serde(default)]
    pub scope_only: Vec<String>,
    /// Frontmatter fields to select (comma-separated).
    #[serde(default)]
    pub select: Vec<String>,
    /// Emit tag counts.
    #[serde(default)]
    pub tag_counts: bool,
    /// Reverse-source index: return files listed in a path's `sources:` frontmatter.
    #[serde(default)]
    pub sources: Option<String>,
    /// Result limit.
    #[serde(default)]
    pub limit: PositiveCount,
}

impl Default for MetaInput {
    fn default() -> Self {
        Self {
            where_: Vec::new(),
            since: None,
            scope: Vec::new(),
            scope_only: Vec::new(),
            select: Vec::new(),
            tag_counts: false,
            sources: None,
            limit: PositiveCount(DEFAULT_LIMIT),
        }
    }
}

/// Change feed request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangesInput {
    /// Return changes since this timestamp.
    pub since: String,
    /// Scope names to include.
    #[serde(default)]
    pub scope: Vec<String>,
    /// Scope names to search exclusively.
    #[serde(default)]
    pub scope_only: Vec<String>,
    /// Result limit.
    #[serde(default)]
    pub limit: PositiveCount,
}

/// Lint check request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LintInput {
    /// Which lint check to run.
    pub check: LintCheck,
    /// Scope names to include.
    #[serde(default)]
    pub scope: Vec<String>,
    /// Scope names to search exclusively.
    #[serde(default)]
    pub scope_only: Vec<String>,
}

const fn default_depth() -> u8 {
    RELATED_DEFAULT_DEPTH
}

// ── Response types ──────────────────────────────────────────────────────────

/// Unified output envelope for all Talon responses.
///
/// Every JSON response uses this shape:
/// - Success: `{ action, version, ok: true, data: ..., meta: ... }`
/// - Error: `{ action, version, ok: false, error: { code, message, detail } }`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum TalonResponse {
    /// Search response.
    Search(SearchResponse),
    /// Read response.
    Read(ReadResponse),
    /// Sync response.
    Sync(SyncResponse),
    /// Status response.
    Status(StatusResponse),
    /// Related-note response.
    Related(RelatedResponse),
    /// Frontmatter query response.
    Meta(MetaResponse),
    /// Change feed response.
    Changes(ChangesResponse),
    /// Lint check response.
    Lint(LintResponse),
}

/// Error envelope used when `ok: false`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    /// Error code from the fixed enum.
    pub code: ErrorCode,
    /// Human-readable error message.
    pub message: String,
    /// Optional structured context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

/// Metadata included in every successful response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseMeta {
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Number of results returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_count: Option<u32>,
    /// Warnings produced during the call.
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Resolved active scope set, where applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_set: Option<Vec<String>>,
    /// Resolved absolute timestamp, if `--since` was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
}

/// Search response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    /// Effective query.
    pub query: Option<String>,
    /// Effective mode.
    pub mode: SearchMode,
    /// Whether lexical-only mode was used.
    pub fast: bool,
    /// Whether query expansion ran.
    pub expanded: bool,
    /// Whether reranking ran.
    pub reranked: bool,
    /// Index version.
    pub index_version: String,
    /// Result count.
    pub total: u32,
    /// Search results.
    pub results: Vec<SearchResult>,
}

impl SearchResponse {
    /// Builds an empty scaffold response for a parsed search request.
    #[must_use]
    pub fn empty_scaffold(input: SearchInput) -> Self {
        Self {
            query: input.query,
            mode: input.mode,
            fast: input.fast,
            expanded: false,
            reranked: false,
            index_version: "scaffold".to_string(),
            total: 0,
            results: Vec::new(),
        }
    }
}

/// Search result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    /// Vault-relative path.
    pub vault_path: VaultPath,
    /// Container path.
    pub path: ContainerPath,
    /// Display title.
    pub title: String,
    /// Result snippet.
    pub snippet: String,
    /// Result score (after scope multiplier).
    pub score: f32,
    /// Pre-multiplier score (before scope boost).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_score: Option<f32>,
    /// Match provenance.
    pub match_kind: MatchKind,
    /// Resolved scope name, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
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

/// Sync response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResponse {
    /// Whether the sync completed.
    pub completed: bool,
    /// Status label.
    pub status: SyncStatus,
    /// Whether lexical-only mode was requested.
    pub fast: bool,
    /// Whether vector reset was requested.
    pub force: bool,
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
    /// Duration in milliseconds.
    pub duration_ms: u64,
}

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
}

impl StatusResponse {
    /// Builds the scaffold status response before indexing is implemented.
    ///
    /// # Errors
    ///
    /// Returns [`TalonError::InvalidInput`] if the fixed scaffold path is invalid.
    pub fn scaffold() -> TalonResult<Self> {
        Ok(Self {
            state: StatusState::ConfigError,
            enabled: false,
            reason: Some("talon rust port is scaffolded; index is not implemented yet".to_string()),
            container_mount: ContainerPath::parse("/opt/data/workspace/obsidian")?,
            index_version: "scaffold".to_string(),
            index: IndexStats {
                active_notes: 0,
                chunk_count: 0,
                failed_embeddings: 0,
                vector_dimensions: None,
            },
            scopes: None,
        })
    }
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

/// Related-note response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedResponse {
    /// Source path.
    pub path: VaultPath,
    /// Direction traversed.
    pub direction: Direction,
    /// Related notes.
    pub results: Vec<RelatedResult>,
}

/// A single related-note result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedResult {
    /// Vault-relative path.
    pub vault_path: VaultPath,
    /// Display title.
    pub title: String,
    /// Link text from source.
    pub link_text: String,
    /// Direction: outgoing or backlink.
    pub relation: RelationKind,
}

/// Relation kind (outgoing vs backlink).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelationKind {
    Outgoing,
    Backlink,
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

/// A single frontmatter entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaEntry {
    /// Vault-relative path.
    pub path: VaultPath,
    /// Frontmatter key-value pairs.
    pub frontmatter: BTreeMap<String, serde_json::Value>,
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

/// Lint check response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LintResponse {
    /// The check that was run.
    pub check: LintCheck,
    /// Lint findings.
    pub findings: Vec<LintFinding>,
}

/// A single lint finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LintFinding {
    /// Vault-relative path of the file.
    pub path: VaultPath,
    /// Description of the issue.
    pub message: String,
    /// Line number, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}
