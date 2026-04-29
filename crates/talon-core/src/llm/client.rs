//! Blocking OpenAI-compatible chat-completions client.

use std::time::Duration;

use reqwest::blocking::Client as HttpClient;

use crate::inference::redact;

use super::error::ChatError;
use super::types::{
    ChatCompletionOutput, ChatCompletionRequest, ChatCompletionResponse, ChatMessage,
    ReasoningEffort,
};

/// Default HTTP timeout for LLM chat calls.
pub const DEFAULT_CHAT_TIMEOUT: Duration = Duration::from_secs(30);

/// Blocking client for OpenAI-compatible `/chat/completions`.
#[derive(Debug, Clone)]
pub struct ChatClient {
    base_url: String,
    model: String,
    max_tokens: Option<u32>,
    reasoning_effort: Option<ReasoningEffort>,
    chat_template_kwargs: Option<serde_json::Value>,
    http: HttpClient,
}

impl ChatClient {
    /// Builds a client targeting `base_url` with the default timeout.
    ///
    /// # Errors
    ///
    /// Returns [`ChatError::Build`] if the underlying `reqwest::Client` fails
    /// to build.
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Result<Self, ChatError> {
        Self::with_timeout(base_url, model, DEFAULT_CHAT_TIMEOUT)
    }

    /// Builds a client with a custom timeout.
    ///
    /// # Errors
    ///
    /// Returns [`ChatError::Build`] if the underlying `reqwest::Client` fails
    /// to build.
    pub fn with_timeout(
        base_url: impl Into<String>,
        model: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, ChatError> {
        Self::with_timeout_and_max_tokens(base_url, model, timeout, None)
    }

    /// Builds a client with the default timeout and optional completion token cap.
    ///
    /// # Errors
    ///
    /// Returns [`ChatError::Build`] if the underlying `reqwest::Client` fails
    /// to build.
    pub fn with_max_tokens(
        base_url: impl Into<String>,
        model: impl Into<String>,
        max_tokens: Option<u32>,
    ) -> Result<Self, ChatError> {
        Self::with_timeout_and_max_tokens(base_url, model, DEFAULT_CHAT_TIMEOUT, max_tokens)
    }

    /// Builds a client with a custom timeout and optional completion token cap.
    ///
    /// # Errors
    ///
    /// Returns [`ChatError::Build`] if the underlying `reqwest::Client` fails
    /// to build.
    pub fn with_timeout_and_max_tokens(
        base_url: impl Into<String>,
        model: impl Into<String>,
        timeout: Duration,
        max_tokens: Option<u32>,
    ) -> Result<Self, ChatError> {
        let http = HttpClient::builder()
            .timeout(timeout)
            .build()
            .map_err(|err| ChatError::Build {
                message: redact(&err.to_string()),
            })?;
        Ok(Self {
            base_url: base_url.into(),
            model: model.into(),
            max_tokens,
            reasoning_effort: None,
            chat_template_kwargs: None,
            http,
        })
    }

    /// Sets OpenAI-compatible reasoning effort for the request body.
    #[must_use]
    pub const fn with_reasoning_effort(mut self, effort: ReasoningEffort) -> Self {
        self.reasoning_effort = Some(effort);
        self
    }

    /// Sets provider-specific chat-template options.
    #[must_use]
    pub fn with_chat_template_kwargs(mut self, value: serde_json::Value) -> Self {
        self.chat_template_kwargs = Some(value);
        self
    }

    /// Returns the configured model identifier.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Returns the configured chat-completions base URL.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Sends a chat completion request and returns the first message content.
    ///
    /// # Errors
    ///
    /// Returns [`ChatError::Http`] for transport failures or non-2xx statuses.
    /// Returns [`ChatError::MalformedResponse`] when the response body cannot be
    /// decoded or the first choice has no content.
    pub fn complete(
        &self,
        messages: Vec<ChatMessage>,
        temperature: f32,
    ) -> Result<String, ChatError> {
        self.complete_raw(messages, temperature)
            .map(|output| output.content)
    }

    /// Sends a chat completion request and returns content plus raw response.
    ///
    /// # Errors
    ///
    /// Returns [`ChatError::Http`] for transport failures or non-2xx statuses.
    /// Returns [`ChatError::MalformedResponse`] when the response body cannot be
    /// decoded or the first choice has no visible content.
    pub fn complete_raw(
        &self,
        messages: Vec<ChatMessage>,
        temperature: f32,
    ) -> Result<ChatCompletionOutput, ChatError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = ChatCompletionRequest {
            model: self.model.clone(),
            messages,
            max_tokens: self.max_tokens,
            reasoning_effort: self.reasoning_effort,
            temperature,
            chat_template_kwargs: self.chat_template_kwargs.clone(),
        };

        let response = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .map_err(|err| ChatError::Http {
                status: None,
                message: redact(&err.to_string()),
            })?;

        let status = response.status();
        if !status.is_success() {
            let snippet = response.text().unwrap_or_default();
            return Err(ChatError::Http {
                status: Some(status.as_u16()),
                message: redact(&snippet),
            });
        }

        let text = response.text().map_err(|_| ChatError::MalformedResponse)?;
        let completion: ChatCompletionResponse =
            serde_json::from_str(&text).map_err(|_| ChatError::MalformedResponse)?;
        let message = completion
            .choices
            .first()
            .map(|choice| &choice.message)
            .ok_or(ChatError::MalformedResponse)?;
        let content = message
            .content
            .clone()
            .filter(|content| !content.trim().is_empty())
            .ok_or(ChatError::MalformedResponse)?;
        Ok(ChatCompletionOutput {
            content,
            reasoning_content: message.reasoning_content.clone(),
            raw_response: text,
        })
    }
}

/// Strips Markdown code fences and extracts the JSON object substring.
///
/// Ports `stripCodeFences` from `clients/sidecar-llm/local-llm.ts`.
#[must_use]
pub fn strip_code_fences(content: &str) -> String {
    let stripped = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    match (stripped.find('{'), stripped.rfind('}')) {
        (Some(start), Some(end)) if end > start => stripped[start..=end].to_owned(),
        _ => stripped.to_owned(),
    }
}

#[cfg(test)]
mod tests;
