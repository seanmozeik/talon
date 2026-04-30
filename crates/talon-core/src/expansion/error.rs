//! Error type for the LLM expansion client.

use thiserror::Error;

use crate::llm::ChatError;

/// Errors returned by [`ExpansionClient`].
///
/// JSON decode failures are treated as graceful degradation (empty result)
/// rather than errors; only transport-level problems reach this type.
///
/// [`ExpansionClient`]: crate::expansion::ExpansionClient
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ExpansionError {
    /// `reqwest::Client` could not be constructed.
    #[error("expansion client build failed: {message}")]
    Build {
        /// Redacted error detail.
        message: String,
    },

    /// HTTP transport failure or non-2xx status from the sidecar.
    #[error(
        "expansion HTTP error{}: {message}",
        .status.map(|s| format!(" ({s})")).unwrap_or_default()
    )]
    Http {
        /// HTTP status code when a response was received, `None` for transport failures.
        status: Option<u16>,
        /// Redacted detail (URL or response body snippet).
        message: String,
        /// Whether the underlying transport failure was a timeout.
        timed_out: bool,
    },
}

impl From<ChatError> for ExpansionError {
    fn from(value: ChatError) -> Self {
        match value {
            ChatError::Build { message } => Self::Build { message },
            ChatError::Http {
                status,
                message,
                timed_out,
            } => Self::Http {
                status,
                message,
                timed_out,
            },
            ChatError::MalformedResponse => Self::Http {
                status: None,
                message: "malformed chat response".to_string(),
                timed_out: false,
            },
        }
    }
}
