//! Error type for the LLM expansion client.

use thiserror::Error;

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
    },
}
