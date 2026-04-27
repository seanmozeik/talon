//! Search tool output types.

use serde::{Deserialize, Serialize};

use crate::contracts::{ContainerPath, VaultPath};

use super::input::SearchMode;

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
