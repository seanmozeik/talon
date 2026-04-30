//! Search tool input types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::SearchConfig;
use crate::constants::{DEFAULT_LIMIT, RELATED_DEFAULT_DEPTH};
use crate::contracts::PositiveCount;
use crate::error::TalonResult;
use crate::search::constants::CANDIDATE_FLOOR_U16;

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
    /// Starts-with / prefix match (`^=`).
    StartsWith,
    /// Glob pattern match (`~=`). Uses [`globset`] syntax.
    GlobMatch,
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
    /// Disambiguating context for expansion, rerank, and chunk selection.
    #[serde(
        default,
        deserialize_with = "crate::search::intent::deserialize_optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub intent: Option<String>,
    /// Search mode.
    #[serde(default)]
    pub mode: SearchMode,
    /// Lexical-only search when true.
    #[serde(default)]
    pub fast: bool,
    /// Result limit.
    #[serde(default)]
    pub limit: PositiveCount,
    /// Candidate pool size for RRF/rerank over-fetch.
    #[serde(default)]
    pub candidate_limit: PositiveCount,
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
    /// Include every configured scope, overriding `default = false`.
    #[serde(default)]
    pub scope_all: bool,
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
            intent: None,
            mode: SearchMode::Hybrid,
            fast: false,
            limit: PositiveCount::from_const(DEFAULT_LIMIT),
            candidate_limit: PositiveCount::from_const(CANDIDATE_FLOOR_U16),
            path: None,
            tag: Vec::new(),
            frontmatter: None,
            related: false,
            depth: RELATED_DEFAULT_DEPTH,
            direction: Direction::Both,
            scope: Vec::new(),
            scope_only: Vec::new(),
            scope_all: false,
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
    /// Returns [`TalonError::InvalidInput`] when `limit` or `candidate_limit` is zero.
    pub fn from_cli_query(
        query: String,
        intent: Option<String>,
        mode: SearchMode,
        fast: bool,
        limit: Option<u16>,
        candidate_limit: Option<u16>,
        config: Option<&SearchConfig>,
    ) -> TalonResult<Self> {
        let mut input = Self {
            query: Some(query),
            intent: crate::search::intent::normalize_optional(intent),
            mode,
            fast,
            ..Self::from_search_config(config)?
        };
        if let Some(limit) = limit {
            input.limit = PositiveCount::new(limit, "limit")?;
        }
        if let Some(candidate_limit) = candidate_limit {
            input.candidate_limit = PositiveCount::new(candidate_limit, "candidate_limit")?;
        }
        Ok(input)
    }

    /// Builds a search request seeded from configured defaults.
    ///
    /// # Errors
    ///
    /// Returns [`TalonError::InvalidInput`] when configured `limit` or
    /// `candidate_limit` is zero.
    pub fn from_search_config(config: Option<&SearchConfig>) -> TalonResult<Self> {
        let Some(config) = config else {
            return Ok(Self::default());
        };

        Ok(Self {
            limit: PositiveCount::new(config.limit, "limit")?,
            candidate_limit: PositiveCount::new(config.candidate_limit, "candidate_limit")?,
            ..Self::default()
        })
    }
}

const fn default_depth() -> u8 {
    RELATED_DEFAULT_DEPTH
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SearchConfig;

    #[test]
    fn from_cli_query_uses_config_defaults_when_flags_are_absent() {
        let config = SearchConfig {
            candidate_limit: 60,
            limit: 12,
            ..SearchConfig::default()
        };

        let input = SearchInput::from_cli_query(
            "hello".to_string(),
            None,
            SearchMode::Hybrid,
            false,
            None,
            None,
            Some(&config),
        )
        .unwrap_or_else(|err| panic!("search input should build: {err}"));

        assert_eq!(input.limit.get(), 12);
        assert_eq!(input.candidate_limit.get(), 60);
    }

    #[test]
    fn from_cli_query_flags_override_config_defaults() {
        let config = SearchConfig {
            candidate_limit: 60,
            limit: 12,
            ..SearchConfig::default()
        };

        let input = SearchInput::from_cli_query(
            "hello".to_string(),
            None,
            SearchMode::Hybrid,
            false,
            Some(20),
            Some(80),
            Some(&config),
        )
        .unwrap_or_else(|err| panic!("search input should build: {err}"));

        assert_eq!(input.limit.get(), 20);
        assert_eq!(input.candidate_limit.get(), 80);
    }
}
