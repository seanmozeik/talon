//! Internal pipeline types: per-signal score breakdown and the result row
//! that flows from retrievers through fusion and reranking.

/// Per-signal score breakdown for a single result.
///
/// Each field is `Some` when the corresponding signal contributed to the
/// score, and `None` otherwise.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchScores {
    /// BM25 lexical score (normalized to `[0, 1]`).
    pub bm25: Option<f64>,
    /// Fuzzy title/alias score.
    pub fuzzy_title: Option<f64>,
    /// Hybrid fused score (post-RRF).
    pub hybrid: Option<f64>,
    /// Semantic (vector) score.
    pub semantic: Option<f64>,
    /// Cross-encoder rerank score.
    pub rerank: Option<f64>,
}

/// Internal raw search result. Mirrors `RawSearchResult` from the TS
/// reference. The public response type lives in [`crate::tool::SearchResult`]
/// and is built from this by the query layer.
#[derive(Debug, Clone, PartialEq)]
pub struct RawSearchResult {
    /// Vault-relative path of the note.
    pub path: String,
    /// Display title.
    pub title: String,
    /// Tag list (parsed from `notes.tags`).
    pub tags: Vec<String>,
    /// Alias list (parsed from `notes.aliases`).
    pub aliases: Vec<String>,
    /// Snippet text (FTS-derived for lexical, chunk text for semantic).
    pub snippet: String,
    /// Score after the final per-pipeline normalization step.
    pub score: f64,
    /// Per-signal score breakdown.
    pub scores: SearchScores,
}

/// Intermediate hybrid score data produced by the RRF normalization step.
#[derive(Debug, Clone, PartialEq)]
pub struct HybridScoreData {
    /// Vault-relative path.
    pub path: String,
    /// Display title.
    pub title: String,
    /// Tags.
    pub tags: Vec<String>,
    /// Aliases.
    pub aliases: Vec<String>,
    /// Snippet text.
    pub snippet: String,
    /// BM25 contribution (if any).
    pub bm25: Option<f64>,
    /// Fuzzy title contribution (if any).
    pub fuzzy_title: Option<f64>,
    /// Semantic contribution (if any).
    pub semantic: Option<f64>,
    /// Hybrid score before per-pipeline normalization (clamped to `[0, 1]`).
    pub hybrid_before_norm: Option<f64>,
}
