//! Wire types for the OpenAI-compatible `/chat/completions` endpoint.
//!
//! Mirrors the request/response shapes from
//! `clients/sidecar-llm/local-llm.ts`.  Only the fields Talon reads are
//! declared; `serde` silently drops any extra keys the sidecar may send.

use serde::{Deserialize, Serialize};

/// One message in a chat completion conversation.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    /// Conversation role (`"system"` or `"user"`).
    pub role: String,
    /// Message text.
    pub content: String,
}

/// `POST /chat/completions` request body (OpenAI-compatible subset).
#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionRequest {
    /// Model identifier forwarded to the sidecar.
    pub model: String,
    /// Ordered conversation messages.
    pub messages: Vec<ChatMessage>,
    /// Token budget for the generated response.
    pub max_tokens: u32,
    /// Sampling temperature; lower values are more deterministic.
    pub temperature: f32,
}

/// Message payload inside a chat completion choice.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatChoiceMessage {
    /// Generated text; `None` when the model produced an empty finish reason.
    pub content: Option<String>,
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

/// Expected JSON shape inside the model's content field.
///
/// The system prompt instructs the LLM to return exactly this structure.
#[derive(Debug, Clone, Deserialize)]
pub struct ExpansionBody {
    /// Reformulated search queries proposed by the model.
    pub queries: Vec<String>,
}
