//! Search tool input types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::constants::{DEFAULT_LIMIT, RELATED_DEFAULT_DEPTH};
use crate::contracts::PositiveCount;
use crate::error::TalonResult;

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

/// Frontmatter filter accepted by search and related queries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FrontmatterFilter {
    /// Tag-like frontmatter shorthand.
    Text(String),
    /// Any-of string values.
    Texts(Vec<String>),
    /// Exact key/value matches.
    Fields(BTreeMap<String, FrontmatterValue>),
}

pub use crate::text::frontmatter::{FrontmatterValue, FrontmatterValueType};

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

/// Search request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
            limit: PositiveCount::from_const(DEFAULT_LIMIT),
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

const fn default_depth() -> u8 {
    RELATED_DEFAULT_DEPTH
}
