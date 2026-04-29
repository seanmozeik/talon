//! Shared OpenAI-compatible chat-completions client.

mod client;
mod error;
mod types;

pub use client::{ChatClient, DEFAULT_CHAT_TIMEOUT, strip_code_fences};
pub use error::ChatError;
pub use types::{ChatCompletionOutput, ChatMessage, ReasoningEffort};
