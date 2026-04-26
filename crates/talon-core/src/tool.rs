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

    /// Returns a root (`/`) container path. Infallible alternative to `parse("/")`.
    #[must_use]
    pub fn root() -> Self {
        Self("/".to_string())
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
    /// Include per-result `previewAnchors` (BM25 + semantic). Opt-in; adds
    /// one extra DB lookup per result so is off by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchors: Option<bool>,
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
            anchors: None,
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

/// Action-discriminated response payload, serialized inside `TalonEnvelope.data`.
///
/// When serialized, produces `{ action: "<action>", ...fields }` — the action
/// discriminator is redundant with the envelope's top-level `action` but kept
/// for forward-compatibility with MCP tool call results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum TalonResponseData {
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

/// Unified output envelope for all Talon responses.
///
/// Every JSON response uses this shape:
/// - Success: `{ action, version, ok: true, data: ..., meta: ... }`
/// - Error:  `{ action, version, ok: false, error: { code, message, detail } }`
///
/// See Decision 8 in the design spec for the locked contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TalonEnvelope {
    /// Action name in kebab-case (e.g. "search", "sync", "status").
    pub action: String,
    /// Cargo package version at build time.
    pub version: String,
    /// Whether the call succeeded.
    pub ok: bool,
    /// Action-discriminated payload (present when `ok: true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<TalonResponseData>,
    /// Metadata (present when `ok: true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ResponseMeta>,
    /// Error envelope (present when `ok: false`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorEnvelope>,
}

impl TalonEnvelope {
    /// Builds a success envelope.
    #[must_use]
    pub fn ok(action: &'static str, data: TalonResponseData, meta: ResponseMeta) -> Self {
        Self {
            action: action.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            ok: true,
            data: Some(data),
            meta: Some(meta),
            error: None,
        }
    }

    /// Builds an error envelope.
    #[must_use]
    pub fn err(action: &str, error: ErrorEnvelope) -> Self {
        Self {
            action: action.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            ok: false,
            data: None,
            meta: None,
            error: Some(error),
        }
    }

    /// Returns the inner response data, if present.
    #[must_use]
    pub const fn data(&self) -> Option<&TalonResponseData> {
        self.data.as_ref()
    }

    /// Returns the inner response data, if present and mutable.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn data_mut(&mut self) -> Option<&mut TalonResponseData> {
        self.data.as_mut()
    }

    /// Extracts the inner response data.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn into_data(self) -> Option<TalonResponseData> {
        self.data
    }

    /// Returns the human-readable response for this envelope.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn as_response(&self) -> Option<&dyn TalonResponseTrait> {
        self.data.as_ref().map(|d| d as &dyn TalonResponseTrait)
    }
}

/// Trait for accessing response data from `TalonResponseData`.
///
/// Implemented by each response variant so output formatters can match on
/// the inner type without knowing the enum discriminant.
pub trait TalonResponseTrait {
    /// Returns the action name.
    fn action(&self) -> &str;
}

impl TalonResponseTrait for TalonResponseData {
    fn action(&self) -> &str {
        match self {
            Self::Search(_) => "search",
            Self::Read(_) => "read",
            Self::Sync(_) => "sync",
            Self::Status(_) => "status",
            Self::Related(_) => "related",
            Self::Meta(_) => "meta",
            Self::Changes(_) => "changes",
            Self::Lint(_) => "lint",
        }
    }
}

// ── Response inner-type accessor impls ──────────────────────────────────────

impl TalonResponseData {
    /// Returns a reference to the inner `SearchResponse`, if present.
    #[must_use]
    pub const fn as_search(&self) -> Option<&SearchResponse> {
        match self {
            Self::Search(r) => Some(r),
            _ => None,
        }
    }

    /// Returns a reference to the inner `SyncResponse`, if present.
    #[must_use]
    pub const fn as_sync(&self) -> Option<&SyncResponse> {
        match self {
            Self::Sync(r) => Some(r),
            _ => None,
        }
    }

    /// Returns a reference to the inner `StatusResponse`, if present.
    #[must_use]
    pub const fn as_status(&self) -> Option<&StatusResponse> {
        match self {
            Self::Status(r) => Some(r),
            _ => None,
        }
    }

    /// Returns a reference to the inner `RelatedResponse`, if present.
    #[must_use]
    pub const fn as_related(&self) -> Option<&RelatedResponse> {
        match self {
            Self::Related(r) => Some(r),
            _ => None,
        }
    }

    /// Returns a reference to the inner `MetaResponse`, if present.
    #[must_use]
    pub const fn as_meta(&self) -> Option<&MetaResponse> {
        match self {
            Self::Meta(r) => Some(r),
            _ => None,
        }
    }

    /// Returns a reference to the inner `ChangesResponse`, if present.
    #[must_use]
    pub const fn as_changes(&self) -> Option<&ChangesResponse> {
        match self {
            Self::Changes(r) => Some(r),
            _ => None,
        }
    }

    /// Returns a reference to the inner `LintResponse`, if present.
    #[must_use]
    pub const fn as_lint(&self) -> Option<&LintResponse> {
        match self {
            Self::Lint(r) => Some(r),
            _ => None,
        }
    }

    /// Returns a reference to the inner `ReadResponse`, if present.
    #[must_use]
    pub const fn as_read(&self) -> Option<&ReadResponse> {
        match self {
            Self::Read(r) => Some(r),
            _ => None,
        }
    }
}

/// Error envelope used when `ok: false`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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
    /// Builds an empty response when the input query is blank.
    #[must_use]
    pub fn empty_input() -> Self {
        Self {
            query: None,
            mode: SearchMode::Hybrid,
            fast: false,
            expanded: false,
            reranked: false,
            index_version: "1".to_string(),
            total: 0,
            results: Vec::new(),
        }
    }
}

/// Which retrieval strategy produced an anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnchorKind {
    /// BM25 / lexical match — positionally precise, fragment-level.
    Bm25,
    /// Semantic / vector match — chunk-level with char offsets.
    Semantic,
}

/// A scroll-to / highlight anchor for a specific block inside the source note.
///
/// Ports `MatchAnchor` from `obsidian-hybrid-search` (MIT licensed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchAnchor {
    /// Retrieval strategy that produced this anchor.
    pub kind: AnchorKind,
    /// Heading chain above the matching block (e.g. `"Section > Sub"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading_path: Option<String>,
    /// DOM-matchable text derived from the block (first 80 chars, syntax stripped).
    pub match_text: String,
    /// UTF-8 char offset of the block start relative to note body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub char_start: Option<u32>,
    /// UTF-8 char offset of the block end relative to note body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub char_end: Option<u32>,
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
    /// Result snippet (with heading breadcrumb prepended when available).
    pub snippet: String,
    /// Result score (after scope multiplier).
    pub score: f64,
    /// Pre-multiplier score (before scope boost).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_score: Option<f64>,
    /// Match provenance.
    pub match_kind: MatchKind,
    /// Resolved scope name, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Per-result match anchors (populated when `SearchInput.anchors == true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_anchors: Option<Vec<MatchAnchor>>,
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
#[allow(clippy::struct_excessive_bools)] // pre-existing: 4 bools from US-004b embed fields
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

// ── Round-trip tests ────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod envelope_tests {
    use super::*;

    fn success_meta() -> ResponseMeta {
        ResponseMeta {
            duration_ms: 42,
            result_count: Some(3),
            warnings: Vec::new(),
            scope_set: None,
            since: None,
        }
    }

    fn error_envelope() -> ErrorEnvelope {
        ErrorEnvelope {
            code: ErrorCode::Internal,
            message: "something broke".to_string(),
            detail: None,
        }
    }

    // ── Success envelope ──────────────────────────────────────────────

    #[test]
    fn search_success_round_trip() {
        let data = TalonResponseData::Search(SearchResponse {
            query: Some("hello world".to_string()),
            mode: SearchMode::Hybrid,
            fast: false,
            expanded: true,
            reranked: true,
            index_version: "1".to_string(),
            total: 3,
            results: Vec::new(),
        });
        let envelope = TalonEnvelope::ok("search", data, success_meta());
        let json = serde_json::to_string(&envelope).unwrap();
        let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip.action, "search");
        assert!(round_trip.ok);
        assert!(round_trip.data.is_some());
        assert!(round_trip.meta.is_some());
        assert!(round_trip.error.is_none());
        // Verify top-level keys are exactly {action, version, ok, data, meta}
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let mut keys: Vec<String> = parsed.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, vec!["action", "data", "meta", "ok", "version"]);
    }

    #[test]
    fn sync_success_round_trip() {
        let data = TalonResponseData::Sync(SyncResponse {
            completed: true,
            status: SyncStatus::Ok,
            fast: false,
            force: false,
            path_count: 1,
            indexed: 5,
            skipped: 0,
            deleted: 0,
            embedded: 5,
            embed_failed: 0,
            dimension_mismatch: false,
            embed_remediation: None,
            embed_diagnostics: Vec::new(),
            duration_ms: 100,
        });
        let envelope = TalonEnvelope::ok("sync", data, success_meta());
        let json = serde_json::to_string(&envelope).unwrap();
        let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip.action, "sync");
        assert!(round_trip.ok);
        assert!(round_trip.data.is_some());
    }

    #[test]
    fn status_success_round_trip() {
        let data = TalonResponseData::Status(StatusResponse {
            state: StatusState::Ready,
            enabled: true,
            reason: None,
            container_mount: ContainerPath::parse("/vault").unwrap(),
            index_version: "1".to_string(),
            index: IndexStats {
                active_notes: 100,
                chunk_count: 500,
                failed_embeddings: 0,
                vector_dimensions: Some(384),
            },
            scopes: None,
        });
        let envelope = TalonEnvelope::ok("status", data, success_meta());
        let json = serde_json::to_string(&envelope).unwrap();
        let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip.action, "status");
        assert!(round_trip.ok);
    }

    #[test]
    fn read_success_round_trip() {
        let data = TalonResponseData::Read(ReadResponse::stub());
        let envelope = TalonEnvelope::ok("read", data, success_meta());
        let json = serde_json::to_string(&envelope).unwrap();
        let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip.action, "read");
        assert!(round_trip.ok);
    }

    #[test]
    fn related_success_round_trip() {
        let data = TalonResponseData::Related(RelatedResponse {
            path: VaultPath::parse("test.md").unwrap(),
            direction: Direction::Both,
            results: Vec::new(),
        });
        let envelope = TalonEnvelope::ok("related", data, success_meta());
        let json = serde_json::to_string(&envelope).unwrap();
        let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip.action, "related");
        assert!(round_trip.ok);
    }

    #[test]
    fn meta_success_round_trip() {
        let data = TalonResponseData::Meta(MetaResponse {
            entries: Vec::new(),
            tag_counts: Some(BTreeMap::new()),
        });
        let envelope = TalonEnvelope::ok("meta", data, success_meta());
        let json = serde_json::to_string(&envelope).unwrap();
        let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip.action, "meta");
        assert!(round_trip.ok);
    }

    #[test]
    fn changes_success_round_trip() {
        let data = TalonResponseData::Changes(ChangesResponse {
            added: Vec::new(),
            modified: Vec::new(),
            deleted: Vec::new(),
        });
        let envelope = TalonEnvelope::ok("changes", data, success_meta());
        let json = serde_json::to_string(&envelope).unwrap();
        let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip.action, "changes");
        assert!(round_trip.ok);
    }

    #[test]
    fn lint_success_round_trip() {
        let data = TalonResponseData::Lint(LintResponse {
            check: LintCheck::Orphans,
            findings: Vec::new(),
        });
        let envelope = TalonEnvelope::ok("lint", data, success_meta());
        let json = serde_json::to_string(&envelope).unwrap();
        let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip.action, "lint");
        assert!(round_trip.ok);
    }

    // ── Error envelope ────────────────────────────────────────────────

    #[test]
    fn error_round_trip() {
        let envelope = TalonEnvelope::err("search", error_envelope());
        let json = serde_json::to_string(&envelope).unwrap();
        let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip.action, "search");
        assert!(!round_trip.ok);
        assert!(round_trip.data.is_none());
        assert!(round_trip.meta.is_none());
        assert!(round_trip.error.is_some());
        // Verify top-level keys are exactly {action, version, ok, error}
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let mut keys: Vec<String> = parsed.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, vec!["action", "error", "ok", "version"]);
    }

    // ── Top-level key assertions ──────────────────────────────────────

    #[test]
    fn success_envelope_has_exactly_five_keys() {
        let data = TalonResponseData::Search(SearchResponse::empty_input());
        let envelope = TalonEnvelope::ok("search", data, success_meta());
        let json = serde_json::to_string(&envelope).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed.as_object().unwrap().keys().count(),
            5,
            "success envelope must have exactly 5 top-level keys"
        );
    }

    #[test]
    fn error_envelope_has_exactly_four_keys() {
        let envelope = TalonEnvelope::err("search", error_envelope());
        let json = serde_json::to_string(&envelope).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed.as_object().unwrap().keys().count(),
            4,
            "error envelope must have exactly 4 top-level keys"
        );
    }

    #[test]
    fn version_is_cargo_pkg_version() {
        let data = TalonResponseData::Search(SearchResponse::empty_input());
        let envelope = TalonEnvelope::ok("search", data, success_meta());
        assert_eq!(envelope.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn action_is_kebab_case() {
        let data = TalonResponseData::Search(SearchResponse::empty_input());
        let envelope = TalonEnvelope::ok("search", data, success_meta());
        assert_eq!(envelope.action, "search");
        let data2 = TalonResponseData::Search(SearchResponse::empty_input());
        let envelope = TalonEnvelope::ok("my-action", data2, success_meta());
        assert_eq!(envelope.action, "my-action");
    }

    // ── ResponseMeta optional fields ──────────────────────────────────

    #[test]
    fn meta_skips_none_fields() {
        let meta = ResponseMeta {
            duration_ms: 10,
            result_count: None,
            warnings: Vec::new(),
            scope_set: None,
            since: None,
        };
        let data = TalonResponseData::Search(SearchResponse::empty_input());
        let envelope = TalonEnvelope::ok("search", data, meta);
        let json = serde_json::to_string(&envelope).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        // Debug: print the full JSON
        eprintln!("JSON: {json}");
        let meta_obj = parsed.get("meta").unwrap().as_object().unwrap();
        // result_count, scope_set, since should be absent
        assert!(!meta_obj.contains_key("resultCount"));
        assert!(!meta_obj.contains_key("scopeSet"));
        assert!(!meta_obj.contains_key("since"));
        // duration_ms should be present (camelCase)
        assert!(meta_obj.contains_key("durationMs"));
        assert_eq!(meta_obj["durationMs"], 10);
    }

    #[test]
    fn error_skips_none_detail() {
        let env = TalonEnvelope::err(
            "search",
            ErrorEnvelope {
                code: ErrorCode::Internal,
                message: "boom".to_string(),
                detail: None,
            },
        );
        let json = serde_json::to_string(&env).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let error_obj = parsed.get("error").unwrap().as_object().unwrap();
        assert!(!error_obj.contains_key("detail"));
    }
}
