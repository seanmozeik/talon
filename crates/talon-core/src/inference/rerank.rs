//! Blocking HTTP client for rerank endpoints.

use std::convert::TryFrom;

use crate::config::{
    CredentialsConfig, RerankAdapter, RerankConfig, RerankScoreScale, ResolvedAuth,
};
use crate::inference::error::InferenceError;
use crate::inference::http::{AuthenticatedHttp, DEFAULT_INFERENCE_TIMEOUT};
use crate::inference::types::{
    CohereRerankRequest, CohereRerankResponse, RerankRequest, RerankResult,
};

/// Blocking client for configured rerank endpoints.
#[derive(Debug, Clone)]
pub struct RerankClient {
    adapter: RerankAdapter,
    base_url: String,
    model: String,
    score_scale: RerankScoreScale,
    truncate: bool,
    rerank_batch_size: usize,
    http: AuthenticatedHttp,
}

impl RerankClient {
    /// Builds a client from config and resolved credentials.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::Build`] when the HTTP client cannot be built.
    /// Returns [`InferenceError::Config`] when auth resolution fails.
    pub fn from_config(
        config: &RerankConfig,
        credentials: &CredentialsConfig,
        rerank_batch_size: usize,
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
            score_scale: config.score_scale,
            truncate: config.truncate,
            rerank_batch_size: rerank_batch_size.max(1),
            http,
        })
    }

    /// Builds a minimal TEI rerank client for tests and wiremock fixtures.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::Build`] when the HTTP client cannot be built.
    pub fn tei_for_tests(
        base_url: impl Into<String>,
        rerank_batch_size: usize,
    ) -> Result<Self, InferenceError> {
        let http =
            AuthenticatedHttp::with_timeout(DEFAULT_INFERENCE_TIMEOUT, ResolvedAuth::default(), 3)?;
        Ok(Self {
            adapter: RerankAdapter::Minimal,
            base_url: base_url.into(),
            model: "rerank".to_owned(),
            score_scale: RerankScoreScale::Normalized,
            truncate: true,
            rerank_batch_size: rerank_batch_size.max(1),
            http,
        })
    }

    /// Reranks candidate texts against a query.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::Http`] or [`InferenceError::Decode`].
    pub fn rerank(
        &self,
        query: &str,
        texts: &[String],
        return_text: bool,
    ) -> Result<Vec<RerankResult>, InferenceError> {
        match self.adapter {
            RerankAdapter::Tei | RerankAdapter::Minimal => {
                self.rerank_tei_style(query, texts, return_text)
            }
            RerankAdapter::Cohere | RerankAdapter::Jina => self.rerank_cohere_style(query, texts),
        }
    }

    fn rerank_tei_style(
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
            let mut batch_results: Vec<RerankResult> = self.http.post_json(&url, &body)?;
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

    fn rerank_cohere_style(
        &self,
        query: &str,
        texts: &[String],
    ) -> Result<Vec<RerankResult>, InferenceError> {
        let url = format!("{}/rerank", self.base_url.trim_end_matches('/'));
        let mut results = Vec::with_capacity(texts.len());

        for (batch_index, batch) in texts.chunks(self.rerank_batch_size).enumerate() {
            let top_n = u32::try_from(batch.len()).ok();
            let body = CohereRerankRequest {
                model: self.model.clone(),
                query: query.to_string(),
                documents: batch.to_vec(),
                top_n,
            };
            let response: CohereRerankResponse = self.http.post_json(&url, &body)?;
            let batch_offset =
                u32::try_from(batch_index * self.rerank_batch_size).map_err(|_| {
                    InferenceError::Decode {
                        message: "rerank index overflow".to_owned(),
                    }
                })?;
            for row in response.results {
                let index =
                    row.index
                        .checked_add(batch_offset)
                        .ok_or_else(|| InferenceError::Decode {
                            message: "rerank index overflow".to_owned(),
                        })?;
                results.push(RerankResult {
                    index,
                    score: self.normalize_rerank_score(row.relevance_score),
                });
            }
        }

        Ok(results)
    }

    fn rerank_raw_scores_flag(&self) -> Option<bool> {
        (self.adapter == RerankAdapter::Tei).then_some(self.score_scale == RerankScoreScale::Logits)
    }

    fn rerank_truncate_flag(&self) -> Option<bool> {
        (self.adapter == RerankAdapter::Tei).then_some(self.truncate)
    }

    fn normalize_rerank_score(&self, score: f32) -> f32 {
        match self.score_scale {
            RerankScoreScale::Normalized => score.clamp(0.0, 1.0),
            RerankScoreScale::Logits => 1.0 / (1.0 + (-score).exp()),
        }
    }
}
