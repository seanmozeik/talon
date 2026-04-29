//! TEI/OpenAI-compatible endpoint configuration types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::llm::ReasoningEffort;

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

/// Ask-command chat model override.
///
/// Transport settings are shared with `[expansion]`; this table only selects
/// the model and reasoning behavior used by `talon ask`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskConfig {
    /// Ask planner/synthesis model name. Falls back to `[expansion].model`.
    #[serde(default)]
    pub model: Option<String>,
    /// Provider-specific Qwen-style thinking toggle for query planning.
    ///
    /// When set, Talon sends `chat_template_kwargs.enable_thinking` in the
    /// OpenAI-compatible planning request body. Servers that do not support
    /// this field may ignore it.
    #[serde(default)]
    pub planning_enable_thinking: Option<bool>,
    /// Provider-specific Qwen-style thinking toggle for answer synthesis.
    ///
    /// When set, Talon sends `chat_template_kwargs.enable_thinking` in the
    /// OpenAI-compatible synthesis request body. Leave unset to use the
    /// provider default.
    #[serde(default)]
    pub synthesis_enable_thinking: Option<bool>,
    /// OpenAI-compatible reasoning effort for query planning.
    ///
    /// Serialized as `reasoning_effort` on chat-completions requests. The
    /// value `"off"` is accepted as a config alias for `"none"`.
    #[serde(default)]
    pub planning_reasoning_effort: Option<ReasoningEffort>,
    /// OpenAI-compatible reasoning effort for answer synthesis.
    ///
    /// Serialized as `reasoning_effort` on chat-completions requests. The
    /// value `"off"` is accepted as a config alias for `"none"`.
    #[serde(default)]
    pub synthesis_reasoning_effort: Option<ReasoningEffort>,
    /// Extra provider-specific `chat_template_kwargs` for query planning.
    ///
    /// These are merged with `planning_enable_thinking`; explicit keys in this
    /// map win over the shorthand boolean.
    #[serde(default)]
    pub planning_chat_template_kwargs: Option<BTreeMap<String, serde_json::Value>>,
    /// Extra provider-specific `chat_template_kwargs` for answer synthesis.
    ///
    /// These are merged with `synthesis_enable_thinking`; explicit keys in this
    /// map win over the shorthand boolean.
    #[serde(default)]
    pub synthesis_chat_template_kwargs: Option<BTreeMap<String, serde_json::Value>>,
}
