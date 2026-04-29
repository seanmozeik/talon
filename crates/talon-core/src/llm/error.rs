//! Error type for OpenAI-compatible chat-completion calls.

use thiserror::Error;

/// Errors returned by [`ChatClient`].
///
/// [`ChatClient`]: crate::llm::ChatClient
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ChatError {
    /// `reqwest::Client` could not be constructed.
    #[error("chat client build failed: {message}")]
    Build {
        /// Redacted error detail.
        message: String,
    },

    /// HTTP transport failure or non-2xx status from the sidecar.
    #[error(
        "chat HTTP error{}: {message}",
        .status.map(|s| format!(" ({s})")).unwrap_or_default()
    )]
    Http {
        /// HTTP status code when a response was received, `None` for transport failures.
        status: Option<u16>,
        /// Redacted detail (URL or response body snippet).
        message: String,
    },

    /// The server returned a response that did not match the expected schema.
    #[error("chat response was malformed")]
    MalformedResponse,
}
