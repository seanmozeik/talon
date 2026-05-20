use std::convert::TryFrom;
use std::time::Duration;

use reqwest::StatusCode;

use super::InferenceClient;
use crate::inference::error::{InferenceError, redact};
use crate::inference::types::{EmbedChunkedRequest, EmbedChunkedResponse};

impl InferenceClient {
    pub(super) fn post_json<B, R>(&self, url: &str, body: &B) -> Result<R, InferenceError>
    where
        B: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        self.post_json_with_retry(url, body)
    }

    pub(super) fn post_json_with_retry<B, R>(
        &self,
        url: &str,
        body: &B,
    ) -> Result<R, InferenceError>
    where
        B: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        // Algorithm ported verbatim from obsidian-hybrid-search (MIT) — embedder.ts:384-395
        let max_attempts = self.max_attempts.max(1);
        for attempt in 0..max_attempts {
            match self.post_json_attempt(url, body) {
                Ok(value) => return Ok(value),
                Err(err) if err.retryable && attempt + 1 < max_attempts => {
                    (self.sleep)(Duration::from_secs(2_u64.pow(attempt + 1)));
                }
                Err(err) => return Err(err.error),
            }
        }
        unreachable!("retry loop always returns or errors")
    }

    pub(super) fn embed_chunked_fallback(
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
