//! Blocking HTTP client for embedding endpoints.

use std::convert::TryFrom;

use crate::config::{CredentialsConfig, EmbeddingAdapter, EmbeddingConfig, ResolvedAuth};
use crate::inference::error::InferenceError;
use crate::inference::http::{AuthenticatedHttp, DEFAULT_INFERENCE_TIMEOUT};
use crate::inference::types::{
    EmbedChunkedDataItem, EmbedChunkedRequest, EmbedChunkedResponse, EmbedRequest,
    OpenAiEmbeddingRequest, OpenAiEmbeddingResponse,
};

/// Blocking client for configured embedding endpoints.
#[derive(Debug, Clone)]
pub struct EmbeddingClient {
    adapter: EmbeddingAdapter,
    base_url: String,
    model: String,
    document_model: String,
    http: AuthenticatedHttp,
}

impl EmbeddingClient {
    /// Builds a client from config and resolved credentials.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::Build`] when the HTTP client cannot be built.
    /// Returns [`InferenceError::Config`] when auth resolution fails.
    pub fn from_config(
        config: &EmbeddingConfig,
        credentials: &CredentialsConfig,
    ) -> Result<Self, InferenceError> {
        let auth = config
            .auth
            .resolve(credentials)
            .map_err(|err| InferenceError::Config {
                message: err.to_string(),
            })?;
        let http = AuthenticatedHttp::with_timeout(DEFAULT_INFERENCE_TIMEOUT, auth, 3)?;
        Ok(Self {
            adapter: config.adapter,
            base_url: config.base_url.clone(),
            model: config.model.clone(),
            document_model: config.document_model().to_owned(),
            http,
        })
    }

    /// Builds a TEI client for tests and wiremock fixtures.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::Build`] when the HTTP client cannot be built.
    pub fn tei_for_tests(
        base_url: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, InferenceError> {
        let model = model.into();
        let http =
            AuthenticatedHttp::with_timeout(DEFAULT_INFERENCE_TIMEOUT, ResolvedAuth::default(), 3)?;
        Ok(Self {
            adapter: EmbeddingAdapter::Tei,
            base_url: base_url.into(),
            document_model: model.clone(),
            model,
            http,
        })
    }

    /// Model slug written to vector metadata for single-chunk notes.
    #[must_use]
    pub fn chunk_model(&self) -> &str {
        &self.model
    }

    /// Model slug written to vector metadata for multi-chunk notes.
    #[must_use]
    pub fn document_model(&self) -> &str {
        &self.document_model
    }

    /// Embeds a batch of texts and returns one vector per input.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::Http`] or [`InferenceError::Decode`].
    pub fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, InferenceError> {
        match self.adapter {
            EmbeddingAdapter::Tei => self.embed_tei(inputs),
            EmbeddingAdapter::OpenAi => self.embed_openai(inputs, &self.model),
        }
    }

    /// Embeds grouped chunks (one group per note).
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::Http`] or [`InferenceError::Decode`].
    pub fn embed_chunked(
        &self,
        input: &[Vec<String>],
    ) -> Result<EmbedChunkedResponse, InferenceError> {
        match self.adapter {
            EmbeddingAdapter::Tei => self.embed_chunked_tei(input),
            EmbeddingAdapter::OpenAi => self.embed_chunked_openai(input),
        }
    }

    fn embed_tei(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, InferenceError> {
        let url = format!("{}/embed", self.base_url.trim_end_matches('/'));
        let body = EmbedRequest {
            inputs: inputs.to_vec(),
        };
        self.http.post_json(&url, &body)
    }

    fn embed_openai(
        &self,
        inputs: &[String],
        model: &str,
    ) -> Result<Vec<Vec<f32>>, InferenceError> {
        let url = format!("{}/embeddings", self.base_url.trim_end_matches('/'));
        let body = OpenAiEmbeddingRequest {
            model: model.to_owned(),
            input: inputs.to_vec(),
        };
        let response: OpenAiEmbeddingResponse = self.http.post_json(&url, &body)?;
        let mut rows = response.data;
        rows.sort_by_key(|row| row.index);
        Ok(rows.into_iter().map(|row| row.embedding).collect())
    }

    fn embed_chunked_tei(
        &self,
        input: &[Vec<String>],
    ) -> Result<EmbedChunkedResponse, InferenceError> {
        let url = format!("{}/embed-chunked", self.base_url.trim_end_matches('/'));
        let body = EmbedChunkedRequest {
            input: input.to_vec(),
        };
        if input.len() <= 1 {
            return self.http.post_json(&url, &body);
        }
        self.http
            .post_json_with_retry(&url, &body)
            .map_or_else(|_| self.embed_chunked_tei_fallback(&url, input), Ok)
    }

    fn embed_chunked_tei_fallback(
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
            let mut response: EmbedChunkedResponse = self.http.post_json(url, &body)?;
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

    fn embed_chunked_openai(
        &self,
        input: &[Vec<String>],
    ) -> Result<EmbedChunkedResponse, InferenceError> {
        let mut data = Vec::with_capacity(input.len());
        for (index, group) in input.iter().enumerate() {
            let embeddings = self.embed_openai(group, self.document_model())?;
            data.push(EmbedChunkedDataItem {
                embeddings,
                index: u32::try_from(index).map_err(|_| InferenceError::Decode {
                    message: "embed-chunked index overflow".to_owned(),
                })?,
            });
        }
        Ok(EmbedChunkedResponse {
            data,
            model: self.document_model().to_owned(),
        })
    }
}
