//! Blocking HTTP client for the TEI-compatible sidecar.
//!
use std::time::Duration;

use reqwest::blocking::Client as HttpClient;

use super::error::{InferenceError, redact};
use super::types::{
    EmbedChunkedRequest, EmbedChunkedResponse, EmbedRequest, RerankRequest, RerankResult,
};
use crate::config::{RerankConfig, RerankRequestShape, RerankScoreScale};
use crate::search::constants::RERANK_BATCH_SIZE;

mod transport;

/// Default HTTP timeout for sidecar calls.
///
/// Embedding 5k tokens on CPU can take ~10s; reranking up to 100 candidates is
/// usually sub-second. 60s gives generous headroom without hanging forever on
/// a wedged sidecar.
pub const DEFAULT_INFERENCE_TIMEOUT: Duration = Duration::from_mins(1);

/// Blocking client for the Talon inference sidecar.
#[derive(Debug, Clone)]
pub struct InferenceClient {
    base_url: String,
    http: HttpClient,
    sleep: fn(Duration),
    rerank_batch_size: usize,
    _rerank_max_tokens: u32,
    rerank_config: RerankConfig,
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
        Self::with_timeout_and_rerank_options(
            base_url,
            timeout,
            RERANK_BATCH_SIZE,
            crate::search::constants::RERANK_MAX_TOKENS,
            RerankConfig::default(),
        )
    }

    /// Builds a client with custom rerank process tunables.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::Build`] on `reqwest::Client` build failure.
    pub fn with_rerank_options(
        base_url: impl Into<String>,
        rerank_batch_size: usize,
        rerank_max_tokens: u32,
    ) -> Result<Self, InferenceError> {
        Self::with_rerank_options_and_protocol(
            base_url,
            rerank_batch_size,
            rerank_max_tokens,
            RerankConfig::default(),
        )
    }

    /// Builds a client with custom rerank process and protocol tunables.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::Build`] on `reqwest::Client` build failure.
    pub fn with_rerank_options_and_protocol(
        base_url: impl Into<String>,
        rerank_batch_size: usize,
        rerank_max_tokens: u32,
        rerank_config: RerankConfig,
    ) -> Result<Self, InferenceError> {
        Self::with_timeout_and_rerank_options(
            base_url,
            DEFAULT_INFERENCE_TIMEOUT,
            rerank_batch_size,
            rerank_max_tokens,
            rerank_config,
        )
    }

    /// Builds a client with custom timeout, rerank process, and protocol tunables.
    ///
    /// # Errors
    /// Returns [`InferenceError::Build`] when the HTTP client cannot be built.
    pub fn with_timeout_and_rerank_options(
        base_url: impl Into<String>,
        timeout: Duration,
        rerank_batch_size: usize,
        rerank_max_tokens: u32,
        rerank_config: RerankConfig,
    ) -> Result<Self, InferenceError> {
        Self::with_optional_timeout_and_rerank_options(
            base_url,
            Some(timeout),
            rerank_batch_size,
            rerank_max_tokens,
            rerank_config,
        )
    }

    /// Builds a client with no HTTP request timeout and custom rerank options.
    ///
    /// # Errors
    /// Returns [`InferenceError::Build`] when the HTTP client cannot be built.
    pub fn with_no_timeout_and_rerank_options(
        base_url: impl Into<String>,
        rerank_batch_size: usize,
        rerank_max_tokens: u32,
        rerank_config: RerankConfig,
    ) -> Result<Self, InferenceError> {
        Self::with_optional_timeout_and_rerank_options(
            base_url,
            None,
            rerank_batch_size,
            rerank_max_tokens,
            rerank_config,
        )
    }

    fn with_optional_timeout_and_rerank_options(
        base_url: impl Into<String>,
        timeout: Option<Duration>,
        rerank_batch_size: usize,
        rerank_max_tokens: u32,
        rerank_config: RerankConfig,
    ) -> Result<Self, InferenceError> {
        let mut builder = HttpClient::builder();
        if let Some(timeout) = timeout {
            builder = builder.timeout(timeout);
        }
        let http = builder.build().map_err(|err| InferenceError::Build {
            message: redact(&err.to_string()),
        })?;
        Ok(Self {
            base_url: base_url.into(),
            http,
            sleep: std::thread::sleep,
            rerank_batch_size: rerank_batch_size.max(1),
            _rerank_max_tokens: rerank_max_tokens,
            rerank_config,
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
    /// On a transient batch failure, falls back to singleton requests so one
    /// bad note does not abort the whole embed pass.
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
        if input.len() <= 1 {
            return self.post_json(&url, &body);
        }

        self.post_json_with_retry(&url, &body)
            .map_or_else(|_| self.embed_chunked_fallback(&url, input), Ok)
    }

    /// Posts to `/rerank` and returns scored candidates.
    ///
    /// Talon expects normalized relevance scores from the sidecar and keeps
    /// rerank texts small enough to fit the model window.
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
        let mut results = Vec::with_capacity(texts.len());

        for (batch_index, batch) in texts.chunks(self.rerank_batch_size).enumerate() {
            let body = RerankRequest {
                query: query.to_string(),
                texts: batch.to_vec(),
                raw_scores: self.rerank_raw_scores_flag(),
                truncate: self.rerank_truncate_flag(),
                return_text,
            };
            let mut batch_results: Vec<RerankResult> = self.post_json(&url, &body)?;
            let batch_offset =
                u32::try_from(batch_index * self.rerank_batch_size).map_err(|_| {
                    InferenceError::Decode {
                        message: "rerank index overflow".to_owned(),
                    }
                })?;
            for result in &mut batch_results {
                result.index = result.index.checked_add(batch_offset).ok_or_else(|| {
                    InferenceError::Decode {
                        message: "rerank index overflow".to_owned(),
                    }
                })?;
                result.score = self.normalize_rerank_score(result.score);
            }
            results.extend(batch_results);
        }

        Ok(results)
    }

    fn rerank_raw_scores_flag(&self) -> Option<bool> {
        (self.rerank_config.request_shape == RerankRequestShape::Tei)
            .then_some(self.rerank_config.score_scale == RerankScoreScale::Logits)
    }

    fn rerank_truncate_flag(&self) -> Option<bool> {
        (self.rerank_config.request_shape == RerankRequestShape::Tei)
            .then_some(self.rerank_config.truncate)
    }

    fn normalize_rerank_score(&self, score: f32) -> f32 {
        match self.rerank_config.score_scale {
            RerankScoreScale::Normalized => score.clamp(0.0, 1.0),
            RerankScoreScale::Logits => 1.0 / (1.0 + (-score).exp()),
        }
    }
}
#[cfg(test)]
mod tests;
