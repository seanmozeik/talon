//! MCP and CLI tool input/output contracts.

use crate::constants::{DEFAULT_LIMIT, DEFAULT_SNIPPET_LENGTH, RELATED_DEFAULT_DEPTH};
use crate::error::{TalonError, TalonResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A positive count accepted at the tool boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct PositiveCount(u16);

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
    Status,
}

/// Search request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SearchInput {
    /// Primary query.
    pub query: Option<String>,
    /// Batch queries.
    pub queries: Vec<String>,
    /// Search mode.
    pub mode: SearchMode,
    /// Lexical-only search when true.
    pub fast: bool,
    /// Result limit.
    pub limit: PositiveCount,
    /// Snippet length.
    pub snippet_length: PositiveCount,
    /// Optional path scope.
    pub path: Option<String>,
    /// Optional tag scope.
    pub tag: Vec<String>,
    /// Optional path/text scope.
    pub scope: Vec<String>,
    /// Optional frontmatter filter.
    pub frontmatter: Option<FrontmatterFilter>,
    /// Include related notes.
    pub related: bool,
    /// Related traversal depth.
    pub depth: u8,
    /// Related traversal direction.
    pub direction: Direction,
}

impl Default for SearchInput {
    fn default() -> Self {
        Self {
            query: None,
            queries: Vec::new(),
            mode: SearchMode::Hybrid,
            fast: false,
            limit: PositiveCount(DEFAULT_LIMIT),
            snippet_length: PositiveCount(DEFAULT_SNIPPET_LENGTH),
            path: None,
            tag: Vec::new(),
            scope: Vec::new(),
            frontmatter: None,
            related: false,
            depth: RELATED_DEFAULT_DEPTH,
            direction: Direction::Both,
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
#[non_exhaustive]
pub struct ReadInput {
    /// Single path.
    pub path: Option<String>,
    /// Multiple paths.
    pub paths: Vec<String>,
    /// Include raw content.
    pub raw: bool,
    /// First line to include.
    pub from_line: Option<PositiveCount>,
    /// Maximum lines to include.
    pub max_lines: Option<PositiveCount>,
    /// Include line numbers.
    pub line_numbers: bool,
}

/// Sync request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SyncInput {
    /// Single path.
    pub path: Option<String>,
    /// Multiple paths.
    pub paths: Vec<String>,
    /// Skip embeddings.
    pub fast: bool,
    /// Reset vector state before syncing.
    pub force: bool,
}

/// Tool response envelope.
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
}

/// Search response scaffold.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
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

/// Search result scaffold.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SearchResult {
    /// Vault-relative path.
    pub vault_path: VaultPath,
    /// Container path.
    pub path: ContainerPath,
    /// Display title.
    pub title: String,
    /// Result snippet.
    pub snippet: String,
    /// Result score.
    pub score: f32,
    /// Match provenance.
    pub match_kind: MatchKind,
}

/// Read response scaffold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ReadResponse {
    /// Read results.
    pub results: Vec<ReadResult>,
}

/// Read result scaffold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ReadResult {
    /// Whether the note was found.
    pub found: bool,
    /// Vault-relative path.
    pub vault_path: VaultPath,
    /// Container path.
    pub path: ContainerPath,
    /// Optional note title.
    pub title: Option<String>,
    /// Optional note content.
    pub content: Option<String>,
    /// Outgoing links.
    pub links: Vec<String>,
    /// Backlinks.
    pub backlinks: Vec<String>,
    /// Tags.
    pub tags: Vec<String>,
    /// Aliases.
    pub aliases: Vec<String>,
}

/// Sync response scaffold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
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
    /// Remaining pending embeddings.
    pub pending_embeddings: u32,
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
}

/// Status response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct StatusResponse {
    /// Readiness state.
    pub state: StatusState,
    /// Whether Talon is enabled.
    pub enabled: bool,
    /// Optional reason for non-ready states.
    pub reason: Option<String>,
    /// Container mount path.
    pub container_mount: ContainerPath,
    /// Index version.
    pub index_version: String,
    /// Index statistics.
    pub index: IndexStats,
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
                pending_embeddings: 0,
                failed_embeddings: 0,
                vector_dimensions: None,
            },
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
#[non_exhaustive]
pub struct IndexStats {
    /// Active notes.
    pub active_notes: u32,
    /// Indexed chunks.
    pub chunk_count: u32,
    /// Pending embeddings.
    pub pending_embeddings: u32,
    /// Failed embeddings.
    pub failed_embeddings: u32,
    /// Vector dimensions, if known.
    pub vector_dimensions: Option<u16>,
}
