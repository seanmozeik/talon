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
    /// Reranker protocol and score semantics.
    #[serde(default)]
    pub rerank: RerankConfig,
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

/// Reranker protocol and score semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RerankConfig {
    /// Request body shape sent to `/rerank`.
    #[serde(default)]
    pub request_shape: RerankRequestShape,
    /// Score semantics returned by the endpoint.
    #[serde(default)]
    pub score_scale: RerankScoreScale,
    /// Whether to ask TEI-style servers to truncate overlong inputs.
    #[serde(default = "default_rerank_truncate")]
    pub truncate: bool,
}

impl Default for RerankConfig {
    fn default() -> Self {
        Self {
            request_shape: RerankRequestShape::default(),
            score_scale: RerankScoreScale::default(),
            truncate: default_rerank_truncate(),
        }
    }
}

/// Reranker request body variant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RerankRequestShape {
    /// Minimal common reranker shape: `query`, `texts`, `return_text`.
    #[default]
    Minimal,
    /// TEI-compatible shape, adding `raw_scores` and `truncate`.
    Tei,
}

/// Score scale emitted by the reranker endpoint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RerankScoreScale {
    /// Endpoint returns normalized scores in `[0, 1]`.
    #[default]
    Normalized,
    /// Endpoint returns raw logits; Talon applies sigmoid before blending.
    Logits,
}

const fn default_rerank_truncate() -> bool {
    true
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
