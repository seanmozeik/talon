//! Shared OpenAI-compatible chat-completion wire types.

use serde::{Deserialize, Serialize};

/// One message in a chat completion conversation.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    /// Conversation role (`"system"`, `"user"`, or `"assistant"`).
    pub role: String,
    /// Message text.
    pub content: String,
}

impl ChatMessage {
    /// Builds a message.
    #[must_use]
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }
}

/// `POST /chat/completions` request body (OpenAI-compatible subset).
#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionRequest {
    /// Model identifier forwarded to the sidecar.
    pub model: String,
    /// Ordered conversation messages.
    pub messages: Vec<ChatMessage>,
    /// Optional total completion cap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Optional reasoning effort for OpenAI-compatible reasoning models.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
    /// Sampling temperature; lower values are more deterministic.
    pub temperature: f32,
    /// Provider-specific chat-template options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_template_kwargs: Option<serde_json::Value>,
}

/// Reasoning effort level for OpenAI-compatible chat models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    /// Disable reasoning where supported.
    #[serde(alias = "off")]
    None,
    /// Minimal reasoning effort.
    Minimal,
    /// Low reasoning effort.
    Low,
    /// Medium reasoning effort.
    Medium,
    /// High reasoning effort.
    High,
    /// Extra-high reasoning effort.
    Xhigh,
}

impl ReasoningEffort {
    /// Returns whether this setting requests visible/thinking mode from Qwen
    /// chat templates that expose an `enable_thinking` boolean.
    #[must_use]
    pub const fn enables_thinking(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Message payload inside a chat completion choice.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatChoiceMessage {
    /// Generated text; `None` when the model produced an empty finish reason.
    pub content: Option<String>,
    /// Optional hidden/thinking trace returned by some local servers.
    #[serde(default)]
    pub reasoning_content: Option<String>,
}

/// One completion choice from `/chat/completions`.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatChoice {
    /// The generated message.
    pub message: ChatChoiceMessage,
}

/// `/chat/completions` response envelope (subset of the `OpenAI` schema).
#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionResponse {
    /// Ordered completion alternatives; Talon always reads the first.
    pub choices: Vec<ChatChoice>,
}

/// First chat-completion message plus raw response text for diagnostics.
#[derive(Debug, Clone)]
pub struct ChatCompletionOutput {
    /// Generated visible content.
    pub content: String,
    /// Optional hidden/thinking trace returned by some local servers.
    pub reasoning_content: Option<String>,
    /// Raw JSON response body.
    pub raw_response: String,
}
