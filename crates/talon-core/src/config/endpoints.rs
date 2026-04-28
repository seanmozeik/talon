//! TEI/OpenAI-compatible endpoint configuration types.

use serde::{Deserialize, Serialize};

/// TEI-compatible inference endpoint configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceConfig {
    /// Base URL for TEI-compatible routes.
    pub base_url: String,
    /// Model names used by the endpoint.
    pub models: InferenceModels,
}

/// Inference model names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceModels {
    /// Query embedding model.
    pub query_embedding: String,
    /// Document embedding model.
    pub document_embedding: String,
    /// Chunk embedding model.
    pub chunk_embedding: String,
    /// Reranker model.
    pub reranker: String,
}

/// OpenAI-compatible query expansion configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpansionConfig {
    /// Provider label, such as `openai-compatible`.
    pub provider: String,
    /// Chat-completions-compatible base URL.
    pub base_url: String,
    /// Expansion model name.
    pub model: String,
    /// Optional total completion token cap.
    ///
    /// Leave unset for thinking models because many OpenAI-compatible local
    /// servers count hidden reasoning tokens against this budget.
    #[serde(default)]
    pub max_tokens: Option<u32>,
}
