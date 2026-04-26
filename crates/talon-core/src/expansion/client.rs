//! Blocking HTTP client for the OpenAI-compatible LLM expansion sidecar.
//!
//! Ports `clients/sidecar-llm/local-llm.ts`.  The sidecar exposes
//! `POST /chat/completions`; this module sends the system prompt, parses
//! `{"queries":[…]}` from the model's content field, and normalises the
//! result (deduplication, cap, original-query exclusion).
//!
//! Malformed LLM responses (bad JSON, missing keys, empty choices) return
//! `Ok(Vec::new())` rather than an error so the hybrid pipeline can fall
//! back to the original query without interruption.

use std::collections::HashSet;
use std::time::Duration;

use reqwest::blocking::Client as HttpClient;

use crate::inference::redact;

use super::error::ExpansionError;
use super::types::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage, ExpansionBody};

/// Default HTTP timeout for LLM expansion calls.
///
/// 30 s gives comfortable headroom for local models while avoiding indefinite
/// hangs when the sidecar is stalled.
pub const DEFAULT_EXPANSION_TIMEOUT: Duration = Duration::from_secs(30);

/// Token budget for each expansion response.
const EXPANSION_MAX_TOKENS: u32 = 256;

/// Sampling temperature — low for consistent, near-deterministic reformulations.
const EXPANSION_TEMPERATURE: f32 = 0.2;

/// System prompt ported from `clients/sidecar-llm/local-llm.ts`.
const SYSTEM_PROMPT: &str = "Return only valid JSON of the form \
    {\"queries\":[\"...\"]}. Generate 2 to 4 short search reformulations. \
    Do not repeat the original query. Prefer terse, concrete terms that \
    would help Obsidian search.";

/// Blocking HTTP client for the OpenAI-compatible LLM expansion endpoint.
///
/// Uses the same sync `reqwest::blocking` strategy as [`InferenceClient`] so
/// it can run inside `tokio::task::spawn_blocking` alongside the `SQLite`
/// connection.
///
/// [`InferenceClient`]: crate::inference::InferenceClient
#[derive(Debug, Clone)]
pub struct ExpansionClient {
    base_url: String,
    model: String,
    http: HttpClient,
}

impl ExpansionClient {
    /// Builds a client targeting `base_url` with the default timeout.
    ///
    /// # Errors
    ///
    /// Returns [`ExpansionError::Build`] if the underlying `reqwest::Client`
    /// fails to build (typically a TLS configuration issue).
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, ExpansionError> {
        Self::with_timeout(base_url, model, DEFAULT_EXPANSION_TIMEOUT)
    }

    /// Builds a client with a custom timeout.
    ///
    /// # Errors
    ///
    /// Returns [`ExpansionError::Build`] on `reqwest::Client` build failure.
    pub fn with_timeout(
        base_url: impl Into<String>,
        model: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, ExpansionError> {
        let http = HttpClient::builder()
            .timeout(timeout)
            .build()
            .map_err(|err| ExpansionError::Build {
                message: redact(&err.to_string()),
            })?;
        Ok(Self {
            base_url: base_url.into(),
            model: model.into(),
            http,
        })
    }

    /// Requests up to `n_variants` search reformulations for `query`.
    ///
    /// The original query is excluded from the returned list.  Case-insensitive
    /// duplicates are filtered out.  Any malformed or empty LLM response
    /// returns `Ok(Vec::new())` — callers should treat that as "use the
    /// original query".
    ///
    /// # Errors
    ///
    /// Returns [`ExpansionError::Http`] for transport failures or non-2xx
    /// HTTP responses from the sidecar.
    pub fn expand(&self, query: &str, n_variants: u8) -> Result<Vec<String>, ExpansionError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = ChatCompletionRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_owned(),
                    content: SYSTEM_PROMPT.to_owned(),
                },
                ChatMessage {
                    role: "user".to_owned(),
                    content: format!("Query: {query}"),
                },
            ],
            max_tokens: EXPANSION_MAX_TOKENS,
            temperature: EXPANSION_TEMPERATURE,
        };

        let response =
            self.http
                .post(&url)
                .json(&body)
                .send()
                .map_err(|err| ExpansionError::Http {
                    status: None,
                    message: redact(&err.to_string()),
                })?;

        let status = response.status();
        if !status.is_success() {
            let snippet = response.text().unwrap_or_default();
            return Err(ExpansionError::Http {
                status: Some(status.as_u16()),
                message: redact(&snippet),
            });
        }

        // All failures below are LLM-response quality issues → graceful empty.
        let Ok(text) = response.text() else {
            return Ok(vec![]);
        };
        let completion: ChatCompletionResponse = match serde_json::from_str(&text) {
            Ok(c) => c,
            Err(_) => return Ok(vec![]),
        };
        let Some(content) = completion
            .choices
            .first()
            .and_then(|c| c.message.content.as_deref())
        else {
            return Ok(vec![]);
        };
        let cleaned = strip_code_fences(content);
        let expansion: ExpansionBody = match serde_json::from_str(&cleaned) {
            Ok(e) => e,
            Err(_) => return Ok(vec![]),
        };

        Ok(normalize_queries(query, expansion.queries, n_variants))
    }
}

/// Strips Markdown code fences and extracts the JSON object substring.
///
/// Ports `stripCodeFences` from `clients/sidecar-llm/local-llm.ts`.
fn strip_code_fences(content: &str) -> String {
    let stripped = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    match (stripped.find('{'), stripped.rfind('}')) {
        (Some(start), Some(end)) if end > start => stripped[start..=end].to_owned(),
        _ => stripped.to_owned(),
    }
}

/// Deduplicates and caps expansion queries, excluding the original query.
///
/// Ports `normalizeQueries` + `capExpansionQueries` from local-llm.ts.
fn normalize_queries(original: &str, queries: Vec<String>, limit: u8) -> Vec<String> {
    let normalized_original = original.trim().to_lowercase();
    let limit = usize::from(limit);
    let mut seen: HashSet<String> = HashSet::new();
    let mut result = Vec::with_capacity(limit);
    for candidate in queries {
        let trimmed = candidate.trim().to_owned();
        if trimmed.is_empty() {
            continue;
        }
        let normalized = trimmed.to_lowercase();
        if normalized != normalized_original && seen.insert(normalized) {
            result.push(trimmed);
            if result.len() >= limit {
                break;
            }
        }
    }
    result
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn start_client(uri: String) -> ExpansionClient {
        ExpansionClient::new(uri, "test-model").unwrap()
    }

    #[test]
    fn happy_path_returns_variants() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "choices": [{
                        "message": {
                            "content": "{\"queries\":[\"rust async patterns\",\"tokio futures guide\",\"async await rust\"]}"
                        }
                    }]
                })))
                .mount(&server),
        );
        let client = start_client(server.uri());
        let result = client.expand("async rust", 4).unwrap();
        assert_eq!(result.len(), 3);
        assert!(result.contains(&"rust async patterns".to_owned()));
        assert!(result.contains(&"tokio futures guide".to_owned()));
    }

    #[test]
    fn malformed_json_body_returns_empty_vec() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(ResponseTemplate::new(200).set_body_string("not json at all!!!"))
                .mount(&server),
        );
        let client = start_client(server.uri());
        let result = client.expand("anything", 4).unwrap();
        assert!(result.is_empty(), "malformed body must return empty Vec");
    }

    #[test]
    fn http_5xx_maps_to_expansion_error() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
                .mount(&server),
        );
        let client = start_client(server.uri());
        let err = client.expand("query", 2).unwrap_err();
        assert!(
            matches!(
                err,
                ExpansionError::Http {
                    status: Some(500),
                    ..
                }
            ),
            "expected Http(500), got {err}"
        );
    }

    #[test]
    fn original_query_excluded_from_variants() {
        let queries = vec![
            "Async Rust".to_owned(),
            "rust async patterns".to_owned(),
            "tokio".to_owned(),
        ];
        let result = normalize_queries("async rust", queries, 4);
        assert!(!result.iter().any(|q| q.to_lowercase() == "async rust"));
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn n_variants_cap_respected() {
        let queries = vec![
            "a".to_owned(),
            "b".to_owned(),
            "c".to_owned(),
            "d".to_owned(),
            "e".to_owned(),
        ];
        let result = normalize_queries("original", queries, 3);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn strip_code_fences_removes_markdown_wrapper() {
        let wrapped = "```json\n{\"queries\":[\"a\",\"b\"]}\n```";
        let cleaned = strip_code_fences(wrapped);
        assert_eq!(cleaned, "{\"queries\":[\"a\",\"b\"]}");
    }
}
