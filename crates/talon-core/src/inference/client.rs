//! Blocking HTTP client for the TEI-compatible sidecar.
//!
//! The CLI runs the embed pipeline inside [`tokio::task::spawn_blocking`] so
//! it can use the same sync `rusqlite::Connection` as the rest of the indexer.
//! That means the inference client must be blocking; using `reqwest::blocking`
//! avoids hand-rolling an `executor::block_on` bridge.

use std::convert::TryFrom;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::blocking::Client as HttpClient;

use super::error::{InferenceError, redact};
use super::types::{
    EmbedChunkedRequest, EmbedChunkedResponse, EmbedRequest, RerankRequest, RerankResult,
};
use crate::search::constants::RERANK_BATCH_SIZE;

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
    sleep: fn(Duration),
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
            sleep: std::thread::sleep,
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

        self.post_json_once(&url, &body)
            .map_or_else(|_| self.embed_chunked_fallback(&url, input), Ok)
    }

    /// Posts to `/rerank` and returns scored candidates.
    ///
    /// Sequence-length truncation is enforced server-side using the sidecar's
    /// default `max_length`; Talon keeps rerank texts small enough that no
    /// request-side `max_length` field is sent.
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

        for (batch_index, batch) in texts.chunks(RERANK_BATCH_SIZE).enumerate() {
            let body = RerankRequest {
                query: query.to_string(),
                texts: batch.to_vec(),
                return_text,
            };
            let mut batch_results: Vec<RerankResult> = self.post_json(&url, &body)?;
            let batch_offset = u32::try_from(batch_index * RERANK_BATCH_SIZE).map_err(|_| {
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
            }
            results.extend(batch_results);
        }

        Ok(results)
    }

    fn post_json<B, R>(&self, url: &str, body: &B) -> Result<R, InferenceError>
    where
        B: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        self.post_json_with_retry(url, body)
    }

    fn post_json_with_retry<B, R>(&self, url: &str, body: &B) -> Result<R, InferenceError>
    where
        B: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        // Algorithm ported verbatim from obsidian-hybrid-search (MIT) — embedder.ts:384-395
        for attempt in 0u32..=2 {
            match self.post_json_attempt(url, body) {
                Ok(value) => return Ok(value),
                Err(err) if err.retryable && attempt < 2 => {
                    (self.sleep)(Duration::from_secs(2_u64.pow(attempt + 1)));
                }
                Err(err) => return Err(err.error),
            }
        }
        unreachable!("retry loop always returns or errors")
    }

    fn post_json_once<B, R>(&self, url: &str, body: &B) -> Result<R, InferenceError>
    where
        B: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        self.post_json_attempt(url, body).map_err(|err| err.error)
    }

    fn embed_chunked_fallback(
        &self,
        url: &str,
        input: &[Vec<String>],
    ) -> Result<EmbedChunkedResponse, InferenceError> {
        let mut data = Vec::with_capacity(input.len());
        let mut model: Option<String> = None;

        for (index, group) in input.iter().enumerate() {
            let body = EmbedChunkedRequest {
                input: vec![group.clone()],
            };
            let mut response: EmbedChunkedResponse = self.post_json(url, &body)?;
            let group_index = u32::try_from(index).map_err(|_| InferenceError::Decode {
                message: "embed-chunked index overflow".to_owned(),
            })?;
            let Some(mut item) = response.data.pop() else {
                return Err(InferenceError::Decode {
                    message: "embed-chunked fallback returned no data".to_owned(),
                });
            };
            if !response.data.is_empty() {
                return Err(InferenceError::Decode {
                    message: "embed-chunked fallback returned unexpected response shape".to_owned(),
                });
            }
            item.index = group_index;
            data.push(item);
            if model.is_none() {
                model = Some(response.model);
            }
        }

        Ok(EmbedChunkedResponse {
            data,
            model: model.unwrap_or_default(),
        })
    }

    fn post_json_attempt<B, R>(&self, url: &str, body: &B) -> Result<R, PostJsonError>
    where
        B: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        let response = self.http.post(url).json(body).send().map_err(|err| {
            let retryable = err.is_connect() || err.is_timeout() || err.is_request();
            PostJsonError {
                error: InferenceError::Http {
                    status: None,
                    message: redact(&err.to_string()),
                },
                retryable,
            }
        })?;
        let status = response.status();
        if !status.is_success() {
            let snippet = response.text().unwrap_or_default();
            let error = InferenceError::Http {
                status: Some(status.as_u16()),
                message: redact(&snippet),
            };
            return Err(PostJsonError {
                retryable: should_retry_status(status),
                error,
            });
        }
        response.json::<R>().map_err(|err| PostJsonError {
            error: InferenceError::Decode {
                message: redact(&err.to_string()),
            },
            retryable: false,
        })
    }
}

#[derive(Debug)]
struct PostJsonError {
    error: InferenceError,
    retryable: bool,
}

fn should_retry_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS
        || status == StatusCode::BAD_GATEWAY
        || status == StatusCode::SERVICE_UNAVAILABLE
        || status.is_server_error()
}
#[cfg(test)]
mod tests;
