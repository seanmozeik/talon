//! Blocking HTTP client for the TEI-compatible sidecar.
//!
//! The CLI runs the embed pipeline inside [`tokio::task::spawn_blocking`] so
//! it can use the same sync `rusqlite::Connection` as the rest of the indexer.
//! That means the inference client must be blocking; using `reqwest::blocking`
//! avoids hand-rolling an `executor::block_on` bridge.

use std::time::Duration;

use reqwest::blocking::Client as HttpClient;

use super::error::{InferenceError, redact};
use super::types::{
    EmbedChunkedRequest, EmbedChunkedResponse, EmbedRequest, RerankRequest, RerankResult,
};

/// Default HTTP timeout for sidecar calls.
///
/// Embedding 5k tokens on CPU can take ~10s; reranking up to 100 candidates is
/// usually sub-second. 60s gives generous headroom without hanging forever on
/// a wedged sidecar.
pub const DEFAULT_INFERENCE_TIMEOUT: Duration = Duration::from_secs(60);

/// Blocking client for the Talon inference sidecar.
#[derive(Debug, Clone)]
pub struct InferenceClient {
    base_url: String,
    http: HttpClient,
}

impl InferenceClient {
    /// Builds a client targeting `base_url` with the default timeout.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::Build`] if the underlying `reqwest::Client`
    /// fails to build (typically a TLS configuration issue).
    pub fn new(base_url: impl Into<String>) -> Result<Self, InferenceError> {
        Self::with_timeout(base_url, DEFAULT_INFERENCE_TIMEOUT)
    }

    /// Builds a client with a custom timeout.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::Build`] on `reqwest::Client` build failure.
    pub fn with_timeout(
        base_url: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, InferenceError> {
        let http = HttpClient::builder()
            .timeout(timeout)
            .build()
            .map_err(|err| InferenceError::Build {
                message: redact(&err.to_string()),
            })?;
        Ok(Self {
            base_url: base_url.into(),
            http,
        })
    }

    /// Posts to `/embed` and returns one vector per input.
    ///
    /// # Errors
    ///
    /// - [`InferenceError::Http`] for transport failures or non-2xx responses
    ///   (the sidecar returns 413 for oversized inputs and 422 for malformed
    ///   bodies).
    /// - [`InferenceError::Decode`] if the JSON body cannot be parsed into
    ///   `Vec<Vec<f32>>`.
    pub fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, InferenceError> {
        let url = format!("{}/embed", self.base_url.trim_end_matches('/'));
        let body = EmbedRequest {
            inputs: inputs.to_vec(),
        };
        self.post_json(&url, &body)
    }

    /// Posts to `/embed-chunked` for batched per-note embedding.
    ///
    /// # Errors
    ///
    /// See [`InferenceClient::embed`].
    pub fn embed_chunked(
        &self,
        input: &[Vec<String>],
    ) -> Result<EmbedChunkedResponse, InferenceError> {
        let url = format!("{}/embed-chunked", self.base_url.trim_end_matches('/'));
        let body = EmbedChunkedRequest {
            input: input.to_vec(),
        };
        self.post_json(&url, &body)
    }

    /// Posts to `/rerank` and returns scored candidates.
    ///
    /// # Errors
    ///
    /// See [`InferenceClient::embed`].
    pub fn rerank(
        &self,
        query: &str,
        texts: &[String],
        return_text: bool,
    ) -> Result<Vec<RerankResult>, InferenceError> {
        let url = format!("{}/rerank", self.base_url.trim_end_matches('/'));
        let body = RerankRequest {
            query: query.to_string(),
            texts: texts.to_vec(),
            return_text,
        };
        self.post_json(&url, &body)
    }

    fn post_json<B, R>(&self, url: &str, body: &B) -> Result<R, InferenceError>
    where
        B: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        let response =
            self.http
                .post(url)
                .json(body)
                .send()
                .map_err(|err| InferenceError::Http {
                    status: None,
                    message: redact(&err.to_string()),
                })?;
        let status = response.status();
        if !status.is_success() {
            let snippet = response.text().unwrap_or_default();
            return Err(InferenceError::Http {
                status: Some(status.as_u16()),
                message: redact(&snippet),
            });
        }
        response.json::<R>().map_err(|err| InferenceError::Decode {
            message: redact(&err.to_string()),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn build_succeeds_with_default_timeout() {
        let client = InferenceClient::new("http://localhost:8080");
        assert!(client.is_ok());
    }

    #[test]
    fn build_succeeds_with_custom_timeout() {
        let client = InferenceClient::with_timeout("http://example", Duration::from_secs(5));
        assert!(client.is_ok());
    }

    #[test]
    fn url_concat_strips_trailing_slash() {
        // Indirect: trim happens inside embed/embed_chunked/rerank. We can at
        // least verify the constructor accepts both forms.
        let a = InferenceClient::new("http://localhost:8080").unwrap();
        let b = InferenceClient::new("http://localhost:8080/").unwrap();
        assert_eq!(a.base_url.trim_end_matches('/'), "http://localhost:8080");
        assert_eq!(b.base_url.trim_end_matches('/'), "http://localhost:8080");
    }
}
