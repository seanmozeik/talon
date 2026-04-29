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
