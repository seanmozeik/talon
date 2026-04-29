//! Wire types for `talon ask` LLM prompts.

use serde::Deserialize;

/// Query-planning JSON returned by the ask model.
#[derive(Debug, Clone, Deserialize)]
pub struct AskPlanBody {
    /// Search queries proposed by the model.
    pub queries: Vec<String>,
}
