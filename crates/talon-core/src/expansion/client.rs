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

use crate::llm::{ChatClient, ChatError, ChatMessage, strip_code_fences};
use crate::text::nfd;

use super::error::ExpansionError;
use super::types::{ExpansionBody, RecallDistillationBody};

/// Default HTTP timeout for LLM expansion calls.
///
/// 30 s gives comfortable headroom for local models while avoiding indefinite
/// hangs when the sidecar is stalled.
pub const DEFAULT_EXPANSION_TIMEOUT: Duration = Duration::from_secs(30);

/// Sampling temperature — deterministic reformulations are required so the
/// same search does not produce a different candidate pool on each process run.
const EXPANSION_TEMPERATURE: f32 = 0.0;

/// System prompt ported from `clients/sidecar-llm/local-llm.ts`, extended
/// with intent-aware guidance modeled on qmd's `expandQuery` (`src/llm.ts`).
///
/// The prompt instructs the model to honor an optional `Query intent:` line
/// in the user message — when present, reformulations should stay consistent
/// with that intent rather than treating the bare query as ambiguous.
const SYSTEM_PROMPT: &str = "Return only valid JSON of the form \
    {\"queries\":[\"...\"]}. Generate 2 to 4 short search reformulations. \
    Do not repeat the original query. Prefer terse, concrete terms that \
    would help Obsidian search. If the user message includes a \
    \"Query intent:\" line, every reformulation must stay consistent with \
    that intent and avoid unrelated senses of the original query.";

const DISTILLER_SYSTEM_PROMPT: &str = "Return only valid JSON of the form \
    {\"search_query\":\"...\",\"phrases\":[\"...\"],\"identifiers\":[\"...\"]}. \
    Given a user prompt for an Obsidian memory system, extract the retrieval \
    intent. Return one compact semantic query plus concrete project, decision, \
    concept, person, place, artifact, path, tag, wikilink, and identifier \
    phrases worth searching. Ignore tool chatter, code blocks, logs, \
    boilerplate, and unrelated implementation detail.";

/// Blocking HTTP client for the OpenAI-compatible LLM expansion endpoint.
///
/// Uses the same sync `reqwest::blocking` strategy as [`InferenceClient`] so
/// it can run inside `tokio::task::spawn_blocking` alongside the `SQLite`
/// connection.
///
/// [`InferenceClient`]: crate::inference::InferenceClient
#[derive(Debug, Clone)]
pub struct ExpansionClient {
    chat: ChatClient,
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
        Self::with_timeout_and_max_tokens(base_url, model, timeout, None)
    }

    /// Builds a client with the default timeout and optional completion token cap.
    ///
    /// # Errors
    ///
    /// Returns [`ExpansionError::Build`] on `reqwest::Client` build failure.
    pub fn with_max_tokens(
        base_url: impl Into<String>,
        model: impl Into<String>,
        max_tokens: Option<u32>,
    ) -> Result<Self, ExpansionError> {
        Self::with_timeout_and_max_tokens(base_url, model, DEFAULT_EXPANSION_TIMEOUT, max_tokens)
    }

    /// Builds a client with a custom timeout and optional completion token cap.
    ///
    /// # Errors
    ///
    /// Returns [`ExpansionError::Build`] on `reqwest::Client` build failure.
    pub fn with_timeout_and_max_tokens(
        base_url: impl Into<String>,
        model: impl Into<String>,
        timeout: Duration,
        max_tokens: Option<u32>,
    ) -> Result<Self, ExpansionError> {
        let chat = ChatClient::with_timeout_and_max_tokens(base_url, model, timeout, max_tokens)
            .map_err(ExpansionError::from)?;
        Ok(Self { chat })
    }

    /// Builds a client with no HTTP request timeout and optional completion token cap.
    ///
    /// # Errors
    ///
    /// Returns [`ExpansionError::Build`] on `reqwest::Client` build failure.
    pub fn with_no_timeout_and_max_tokens(
        base_url: impl Into<String>,
        model: impl Into<String>,
        max_tokens: Option<u32>,
    ) -> Result<Self, ExpansionError> {
        let chat = ChatClient::with_no_timeout_and_max_tokens(base_url, model, max_tokens)
            .map_err(ExpansionError::from)?;
        Ok(Self { chat })
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
        self.expand_with_intent(query, None, n_variants)
    }

    /// Requests reformulations with an optional disambiguating intent.
    ///
    /// When `intent` is present, the user message includes a `Query intent:`
    /// line so the LLM can keep variants consistent with the caller's domain
    /// hint. Mirrors qmd's `expandQuery({ intent })` shape (`src/llm.ts:1131`)
    /// minus the typed lex/vec/hyde grammar.
    ///
    /// # Errors
    ///
    /// Returns [`ExpansionError::Http`] for transport failures or non-2xx
    /// HTTP responses from the sidecar.
    pub fn expand_with_intent(
        &self,
        query: &str,
        intent: Option<&str>,
        n_variants: u8,
    ) -> Result<Vec<String>, ExpansionError> {
        let user_content = build_user_message(query, intent);
        let messages = vec![
            ChatMessage::new("system", SYSTEM_PROMPT),
            ChatMessage::new("user", user_content),
        ];
        let content = match self.chat.complete(messages, EXPANSION_TEMPERATURE) {
            Ok(content) => content,
            Err(ChatError::MalformedResponse) => return Ok(vec![]),
            Err(err) => return Err(ExpansionError::from(err)),
        };
        let cleaned = strip_code_fences(&content);
        let expansion: ExpansionBody = match serde_json::from_str(&cleaned) {
            Ok(e) => e,
            Err(_) => return Ok(vec![]),
        };

        Ok(normalize_queries(query, expansion.queries, n_variants))
    }

    /// Distills an oversized recall prompt into a compact retrieval query.
    ///
    /// # Errors
    ///
    /// Returns [`ExpansionError::Http`] for transport failures or non-2xx
    /// HTTP responses from the sidecar.
    pub fn distill_recall_prompt(
        &self,
        prompt_view: &str,
        extraction_hints: &[String],
    ) -> Result<Option<RecallDistillationBody>, ExpansionError> {
        let mut user_content = String::from("Prompt view:\n");
        user_content.push_str(prompt_view);
        if !extraction_hints.is_empty() {
            user_content.push_str("\n\nExtraction hints:\n");
            for hint in extraction_hints {
                user_content.push_str("- ");
                user_content.push_str(hint);
                user_content.push('\n');
            }
        }
        let messages = vec![
            ChatMessage::new("system", DISTILLER_SYSTEM_PROMPT),
            ChatMessage::new("user", user_content),
        ];
        let content = match self.chat.complete(messages, EXPANSION_TEMPERATURE) {
            Ok(content) => content,
            Err(ChatError::MalformedResponse) => return Ok(None),
            Err(err) => return Err(ExpansionError::from(err)),
        };
        let cleaned = strip_code_fences(&content);
        let mut body: RecallDistillationBody = match serde_json::from_str(&cleaned) {
            Ok(body) => body,
            Err(_) => return Ok(None),
        };
        let search_query = body.search_query.trim().to_owned();
        body.search_query = search_query;
        body.phrases = normalize_items(body.phrases, 12);
        body.identifiers = normalize_items(body.identifiers, 12);
        if body.search_query.is_empty() {
            Ok(None)
        } else {
            Ok(Some(body))
        }
    }
}

/// Builds the user message for an expansion request.
///
/// Without intent: `Query: {query}` (preserves prior wire format).
/// With intent: appends a `Query intent: {intent}` line so the LLM can scope
/// variants to the caller's hint. Mirrors qmd's `expandQuery` prompt body in
/// `src/llm.ts:1152-1154`.
fn build_user_message(query: &str, intent: Option<&str>) -> String {
    intent.map(str::trim).filter(|s| !s.is_empty()).map_or_else(
        || format!("Query: {query}"),
        |intent| format!("Query: {query}\nQuery intent: {intent}"),
    )
}

/// Deduplicates and caps expansion queries, excluding the original query.
///
/// Ports `normalizeQueries` + `capExpansionQueries` from local-llm.ts.
fn normalize_queries(original: &str, queries: Vec<String>, limit: u8) -> Vec<String> {
    let normalized_original = nfd::normalize(original.trim()).to_lowercase();
    let limit = usize::from(limit);
    let mut seen: HashSet<String> = HashSet::new();
    let mut result = Vec::with_capacity(limit);
    for candidate in queries {
        let trimmed = candidate.trim().to_owned();
        if trimmed.is_empty() {
            continue;
        }
        let normalized = nfd::normalize(&trimmed).to_lowercase();
        if normalized != normalized_original && seen.insert(normalized) {
            result.push(trimmed);
            if result.len() >= limit {
                break;
            }
        }
    }
    result
}

fn normalize_items(items: Vec<String>, limit: usize) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut result = Vec::with_capacity(limit);
    for item in items {
        let trimmed = item.trim().to_owned();
        if trimmed.is_empty() {
            continue;
        }
        let normalized = nfd::normalize(&trimmed).to_lowercase();
        if seen.insert(normalized) {
            result.push(trimmed);
            if result.len() >= limit {
                break;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests;
