//! Blocking HTTP client for the TEI-compatible sidecar.
//!
//! The CLI runs the embed pipeline inside [`tokio::task::spawn_blocking`] so
//! it can use the same sync `rusqlite::Connection` as the rest of the indexer.
//! That means the inference client must be blocking; using `reqwest::blocking`
//! avoids hand-rolling an `executor::block_on` bridge.

use std::convert::TryFrom;
use std::time::Duration;

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
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

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

    #[test]
    fn rerank_batches_inputs_and_offsets_indices() {
        let runtime = runtime();
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(
            Mock::given(method("POST"))
                .and(path("/rerank"))
                .and(body_partial_json(json!({
                    "query": "query",
                    "texts": ["t0", "t1", "t2", "t3"],
                    "return_text": false
                })))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                    {"index": 1, "score": 0.4},
                    {"index": 0, "score": 0.9}
                ])))
                .mount(&server),
        );
        runtime.block_on(
            Mock::given(method("POST"))
                .and(path("/rerank"))
                .and(body_partial_json(json!({
                    "query": "query",
                    "texts": ["t4", "t5"],
                    "return_text": false
                })))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                    {"index": 0, "score": 0.2}
                ])))
                .mount(&server),
        );

        let client = InferenceClient::new(server.uri()).unwrap();
        let texts: Vec<String> = (0..6).map(|i| format!("t{i}")).collect();
        let result = client.rerank("query", &texts, false).unwrap();
        let got: Vec<(u32, f32)> = result.iter().map(|r| (r.index, r.score)).collect();
        assert_eq!(got, vec![(1, 0.4), (0, 0.9), (4, 0.2)]);

        let requests = runtime.block_on(server.received_requests()).unwrap();
        assert_eq!(requests.len(), 2);

        let first: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        let second: serde_json::Value = serde_json::from_slice(&requests[1].body).unwrap();
        assert!(first.get("max_length").is_none());
        assert!(second.get("max_length").is_none());
        assert_eq!(first["texts"].as_array().unwrap().len(), 4);
        assert_eq!(second["texts"].as_array().unwrap().len(), 2);
    }
}
