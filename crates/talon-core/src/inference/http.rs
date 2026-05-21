//! Authenticated blocking HTTP helpers for inference clients.

use std::time::Duration;

use reqwest::StatusCode;
use reqwest::blocking::{Client as HttpClient, RequestBuilder};

use crate::config::ResolvedAuth;
use crate::inference::error::{InferenceError, redact};

/// Default HTTP timeout for embedding and rerank calls.
pub const DEFAULT_INFERENCE_TIMEOUT: Duration = Duration::from_mins(1);

/// Blocking HTTP client with optional bearer auth and retry support.
#[derive(Debug, Clone)]
pub struct AuthenticatedHttp {
    http: HttpClient,
    auth: ResolvedAuth,
    sleep: fn(Duration),
    max_attempts: u32,
}

impl AuthenticatedHttp {
    /// Builds an HTTP client with the given timeout and auth material.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::Build`] when the underlying client cannot be
    /// constructed.
    pub fn with_timeout(
        timeout: Duration,
        auth: ResolvedAuth,
        max_attempts: u32,
    ) -> Result<Self, InferenceError> {
        Self::with_optional_timeout(Some(timeout), auth, max_attempts)
    }

    /// Builds an HTTP client without a request timeout.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::Build`] when the underlying client cannot be
    /// constructed.
    pub fn with_no_timeout(auth: ResolvedAuth, max_attempts: u32) -> Result<Self, InferenceError> {
        Self::with_optional_timeout(None, auth, max_attempts)
    }

    fn with_optional_timeout(
        timeout: Option<Duration>,
        auth: ResolvedAuth,
        max_attempts: u32,
    ) -> Result<Self, InferenceError> {
        let mut builder = HttpClient::builder();
        if let Some(timeout) = timeout {
            builder = builder.timeout(timeout);
        }
        let http = builder.build().map_err(|err| InferenceError::Build {
            message: redact(&err.to_string()),
        })?;
        Ok(Self {
            http,
            auth,
            sleep: std::thread::sleep,
            max_attempts: max_attempts.max(1),
        })
    }

    /// Returns a POST builder with auth headers applied.
    pub fn post(&self, url: &str) -> RequestBuilder {
        let mut request = self.http.post(url);
        if let Some(key) = &self.auth.api_key {
            request = request.bearer_auth(key);
        }
        for (name, value) in &self.auth.extra_headers {
            request = request.header(name.as_str(), value.as_str());
        }
        request
    }

    /// POSTs JSON and deserializes the response body.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::Http`] or [`InferenceError::Decode`].
    pub fn post_json<B, R>(&self, url: &str, body: &B) -> Result<R, InferenceError>
    where
        B: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        self.post_json_with_retry(url, body)
    }

    /// POSTs JSON with transient retry for retryable HTTP failures.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::Http`] or [`InferenceError::Decode`].
    pub fn post_json_with_retry<B, R>(&self, url: &str, body: &B) -> Result<R, InferenceError>
    where
        B: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        // Algorithm ported verbatim from obsidian-hybrid-search (MIT) — embedder.ts:384-395
        for attempt in 0..self.max_attempts {
            match self.post_json_attempt(url, body) {
                Ok(value) => return Ok(value),
                Err(err) if err.retryable && attempt + 1 < self.max_attempts => {
                    (self.sleep)(Duration::from_secs(2_u64.pow(attempt + 1)));
                }
                Err(err) => return Err(err.error),
            }
        }
        unreachable!("retry loop always returns or errors")
    }

    fn post_json_attempt<B, R>(&self, url: &str, body: &B) -> Result<R, PostJsonError>
    where
        B: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        let response = self.post(url).json(body).send().map_err(|err| {
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
            return Err(PostJsonError {
                retryable: should_retry_status(status),
                error: InferenceError::Http {
                    status: Some(status.as_u16()),
                    message: redact(&snippet),
                },
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
