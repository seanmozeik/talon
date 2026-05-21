//! HTTP capability endpoint configuration.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::auth::EndpointAuthConfig;
use crate::llm::ReasoningEffort;

/// Wire protocol for embedding HTTP calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EmbeddingAdapter {
    /// TEI-compatible `/embed` and `/embed-chunked` routes.
    Tei,
    /// OpenAI-compatible `POST /embeddings`.
    OpenAi,
}

/// Embedding endpoint configuration (`[embedding]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingConfig {
    pub base_url: String,
    #[serde(flatten)]
    pub auth: EndpointAuthConfig,
    pub adapter: EmbeddingAdapter,
    /// Model slug for query vectors and single-chunk notes.
    pub model: String,
    /// Model slug for multi-chunk notes; defaults to [`Self::model`].
    #[serde(default)]
    pub document_model: Option<String>,
    /// Prompt budget hint for query embedding and recall distillation.
    #[serde(default = "default_embedding_context_tokens")]
    pub context_tokens: u32,
}

impl EmbeddingConfig {
    /// Model slug persisted for multi-chunk document embeddings.
    #[must_use]
    pub fn document_model(&self) -> &str {
        self.document_model.as_deref().unwrap_or(&self.model)
    }
}

/// Wire protocol for rerank HTTP calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RerankAdapter {
    /// TEI-compatible `/rerank` with `raw_scores` and `truncate`.
    Tei,
    /// Common minimal `/rerank` with `{ query, texts, return_text }`.
    Minimal,
    /// Cohere-style `/rerank` with `{ query, documents, top_n }`.
    Cohere,
    /// Jina-style `/rerank` (same response mapping as [`Self::Cohere`]).
    Jina,
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

/// Rerank endpoint configuration (`[rerank]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RerankConfig {
    pub base_url: String,
    #[serde(flatten)]
    pub auth: EndpointAuthConfig,
    pub adapter: RerankAdapter,
    pub model: String,
    #[serde(default)]
    pub score_scale: RerankScoreScale,
    /// Whether to ask TEI-style servers to truncate overlong inputs.
    #[serde(default = "default_rerank_truncate")]
    pub truncate: bool,
}

/// Wire protocol for chat HTTP calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ChatAdapter {
    /// OpenAI-compatible `POST /chat/completions`.
    #[default]
    OpenAi,
}

/// Query expansion chat endpoint (`[chat.expansion]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatExpansionConfig {
    pub base_url: String,
    #[serde(flatten)]
    pub auth: EndpointAuthConfig,
    #[serde(default)]
    pub adapter: ChatAdapter,
    pub model: String,
    #[serde(default = "default_chat_context_tokens")]
    pub context_tokens: u32,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
}

/// Ask chat endpoint overrides (`[chat.ask]`).
///
/// Unset transport fields inherit from [`ChatExpansionConfig`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ChatAskConfig {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(flatten)]
    pub auth: EndpointAuthConfig,
    #[serde(default)]
    pub adapter: Option<ChatAdapter>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_ask_context_tokens")]
    pub context_tokens: u32,
    #[serde(default = "default_ask_max_output_tokens")]
    pub max_output_tokens: u32,
    #[serde(default)]
    pub planning_enable_thinking: Option<bool>,
    #[serde(default)]
    pub synthesis_enable_thinking: Option<bool>,
    #[serde(default)]
    pub planning_reasoning_effort: Option<ReasoningEffort>,
    #[serde(default)]
    pub synthesis_reasoning_effort: Option<ReasoningEffort>,
    #[serde(default)]
    pub planning_chat_template_kwargs: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub synthesis_chat_template_kwargs: Option<BTreeMap<String, serde_json::Value>>,
}

impl ChatAskConfig {
    /// Effective chat-completions base URL.
    #[must_use]
    pub fn resolved_base_url<'a>(&'a self, expansion: &'a ChatExpansionConfig) -> &'a str {
        self.base_url
            .as_deref()
            .filter(|url| !url.is_empty())
            .unwrap_or(expansion.base_url.as_str())
    }

    /// Effective ask model name.
    #[must_use]
    pub fn resolved_model<'a>(&'a self, expansion: &'a ChatExpansionConfig) -> &'a str {
        self.model
            .as_deref()
            .filter(|model| !model.is_empty())
            .unwrap_or(expansion.model.as_str())
    }

    /// Effective chat adapter.
    #[must_use]
    pub fn resolved_adapter(&self, expansion: &ChatExpansionConfig) -> ChatAdapter {
        self.adapter.unwrap_or(expansion.adapter)
    }

    /// Merged auth: ask overrides win when set, otherwise expansion auth applies.
    #[must_use]
    pub fn resolved_auth(&self, expansion: &ChatExpansionConfig) -> EndpointAuthConfig {
        EndpointAuthConfig {
            credential: self
                .auth
                .credential
                .clone()
                .or_else(|| expansion.auth.credential.clone()),
            api_key: self
                .auth
                .api_key
                .clone()
                .or_else(|| expansion.auth.api_key.clone()),
            api_key_env: self
                .auth
                .api_key_env
                .clone()
                .or_else(|| expansion.auth.api_key_env.clone()),
            extra_headers: merge_headers(&expansion.auth.extra_headers, &self.auth.extra_headers),
        }
    }
}

/// Chat capability group (`[chat]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatSection {
    pub expansion: ChatExpansionConfig,
    #[serde(default)]
    pub ask: ChatAskConfig,
}

/// MCP runtime configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpConfig {
    /// Hook-specific runtime budgets.
    #[serde(default)]
    pub hooks: McpHooksConfig,
}

/// Synchronous MCP hook budgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpHooksConfig {
    /// Wall-clock deadline for the recall hook, in milliseconds.
    #[serde(default = "default_recall_deadline_ms")]
    pub recall_deadline_ms: u64,
}

impl Default for McpHooksConfig {
    fn default() -> Self {
        Self {
            recall_deadline_ms: default_recall_deadline_ms(),
        }
    }
}

fn merge_headers(
    base: &BTreeMap<String, String>,
    override_headers: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut merged = base.clone();
    merged.extend(override_headers.iter().map(|(k, v)| (k.clone(), v.clone())));
    merged
}

const fn default_embedding_context_tokens() -> u32 {
    512
}

const fn default_rerank_truncate() -> bool {
    true
}

const fn default_chat_context_tokens() -> u32 {
    32_768
}

const fn default_ask_context_tokens() -> u32 {
    65_536
}

const fn default_ask_max_output_tokens() -> u32 {
    2_048
}

const fn default_recall_deadline_ms() -> u64 {
    20_000
}
