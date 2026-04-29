//! Error type for `talon ask` LLM calls.

use thiserror::Error;

use crate::llm::ChatError;

/// Errors returned by [`AskClient`].
///
/// [`AskClient`]: crate::ask::AskClient
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AskError {
    /// Underlying chat-completion failure.
    #[error(transparent)]
    Chat(#[from] ChatError),
}
