//! Query tool input types.

use serde::{Deserialize, Serialize};

use crate::constants::DEFAULT_LIMIT;
use crate::contracts::PositiveCount;
use crate::search::input::WhereClause;

/// Output format for the recall command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecallFormat {
    /// Structured JSON (default).
    #[default]
    Json,
    /// Prompt-XML block ready for agent context injection.
    PromptXml,
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

/// Context recall request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallInput {
    /// The current user message to recall context for.
    pub message: String,
    /// Prior conversation turns (last N user/assistant messages) fed to expansion.
    #[serde(default)]
    pub prior_messages: Vec<String>,
    /// Token budget for the response payload (default 2000).
    #[serde(default = "default_recall_budget")]
    pub budget_tokens: u32,
    /// Vault paths to exclude from all retrieval sections.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Scope names to include (additive).
    #[serde(default)]
    pub scope: Vec<String>,
    /// Scope names to search exclusively.
    #[serde(default)]
    pub scope_only: Vec<String>,
    /// Filter results indexed since this timestamp (default: 7 days ago).
    #[serde(default)]
    pub since: Option<String>,
    /// Output format.
    #[serde(default)]
    pub format: RecallFormat,
    /// Link graph traversal depth for `linked_context` (1-3, default 1).
    #[serde(default = "default_recall_depth")]
    pub depth: u8,
    /// Recency half-life in days for `recent_edits` scoring (default 7).
    #[serde(default = "default_half_life")]
    pub recency_half_life_days: u8,
    /// Minimum `evidence_score` threshold; below this, return `skipped=true` (default 0.0).
    #[serde(default)]
    pub min_confidence: f64,
    /// Skip expansion and rerank (fast lexical-only path).
    #[serde(default)]
    pub fast: bool,
}

const fn default_recall_budget() -> u32 {
    2000
}

const fn default_recall_depth() -> u8 {
    1
}

const fn default_half_life() -> u8 {
    7
}

impl Default for RecallInput {
    fn default() -> Self {
        Self {
            message: String::new(),
            prior_messages: Vec::new(),
            budget_tokens: default_recall_budget(),
            exclude: Vec::new(),
            scope: Vec::new(),
            scope_only: Vec::new(),
            since: None,
            format: RecallFormat::Json,
            depth: default_recall_depth(),
            recency_half_life_days: default_half_life(),
            min_confidence: 0.0,
            fast: false,
        }
    }
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
            limit: PositiveCount::from_const(DEFAULT_LIMIT),
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
