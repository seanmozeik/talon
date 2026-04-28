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
    /// File modification time as RFC 3339 / ISO 8601 (`"2026-04-25T10:23:00Z"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime: Option<String>,
    /// Per-result match anchors (populated when `SearchInput.anchors == true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_anchors: Option<Vec<MatchAnchor>>,
}

/// Pipeline-stage diagnostics, populated only when verbose is requested.
///
/// All fields are optional so that short-circuited stages (fast mode, decisive
/// BM25 probe, missing rerank inference) simply omit their entry rather than
/// reporting zeroes. Mirrors qmd's stderr stage timings (qmd `cli/qmd.ts:2407`)
/// in a structured form.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchDiagnostics {
    /// Wall-clock time spent on LLM query expansion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expansion_ms: Option<u64>,
    /// Top BM25 probe score that bypassed expansion (when present, the LLM
    /// expansion stage was skipped entirely).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strong_signal_score: Option<f64>,
    /// Number of candidates sent to the cross-encoder reranker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rerank_candidates: Option<u32>,
    /// Wall-clock time spent reranking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rerank_ms: Option<u64>,
}

impl SearchDiagnostics {
    /// Returns `true` when no stage produced a measurement.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.expansion_ms.is_none()
            && self.strong_signal_score.is_none()
            && self.rerank_candidates.is_none()
            && self.rerank_ms.is_none()
    }
}

/// Search response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    /// Vault root (absolute container path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault: Option<ContainerPath>,
    /// Effective query.
    pub query: Option<String>,
    /// Effective mode.
    pub mode: SearchMode,
    /// Whether lexical-only mode was used.
    pub fast: bool,
    /// Whether query expansion ran.
    pub expanded: bool,
    /// Query variants produced by expansion or supplied explicitly.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub expanded_queries: Vec<String>,
    /// Whether reranking ran.
    pub reranked: bool,
    /// Index version.
    pub index_version: String,
    /// Result count.
    pub total: u32,
    /// Search results.
    pub results: Vec<SearchResult>,
    /// Pipeline-stage diagnostics, populated only when verbose is requested.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub diagnostics: Option<SearchDiagnostics>,
}

impl SearchResponse {
    /// Builds an empty response when the input query is blank.
    #[must_use]
    pub fn empty_input() -> Self {
        Self {
            vault: None,
            query: None,
            mode: SearchMode::Hybrid,
            fast: false,
            expanded: false,
            expanded_queries: Vec::new(),
            reranked: false,
            index_version: "1".to_string(),
            total: 0,
            results: Vec::new(),
            diagnostics: None,
        }
    }
}
