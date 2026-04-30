//! Query expansion response types.

use serde::Deserialize;

/// Expected JSON shape inside the expansion model's content field.
///
/// The system prompt instructs the LLM to return exactly this structure.
#[derive(Debug, Clone, Deserialize)]
pub struct ExpansionBody {
    /// Reformulated search queries proposed by the model.
    pub queries: Vec<String>,
}

/// Retrieval intent distilled from an oversized recall prompt.
#[derive(Debug, Clone, Deserialize)]
pub struct RecallDistillationBody {
    /// Compact semantic query.
    pub search_query: String,
    /// Search-worthy phrases.
    #[serde(default)]
    pub phrases: Vec<String>,
    /// Identifiers, paths, tags, and other exact literals.
    #[serde(default)]
    pub identifiers: Vec<String>,
}
