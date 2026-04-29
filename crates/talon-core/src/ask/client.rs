//! Ask-mode LLM client built on the shared chat-completions client.

use std::collections::HashSet;
use std::fmt::Write as _;

use crate::llm::{ChatClient, ChatCompletionOutput, ChatMessage, strip_code_fences};
use crate::query::AskSource;
use crate::text::nfd;

use super::error::AskError;
use super::types::AskPlanBody;

const ASK_TEMPERATURE: f32 = 0.0;

const PLAN_SYSTEM_PROMPT: &str = "Return only valid JSON of the form \
    {\"queries\":[\"...\"]}. Generate 3 to 6 concise search queries for an \
    Obsidian vault. Prefer concrete domain terms, likely note titles, aliases, \
    and useful synonyms. Do not explain.";

const ANSWER_SYSTEM_PROMPT: &str = "Answer the user's question using the vault \
    snippets provided. Keep the answer compact and practical. Do not invent \
    facts that are not supported by the snippets; if the snippets are thin or \
    conflicting, say so briefly. Do not use a forced citation style.";

/// High-level client for `talon ask` planning and synthesis.
#[derive(Debug, Clone)]
pub struct AskClient {
    planning_chat: ChatClient,
    synthesis_chat: ChatClient,
}

/// Query-planning result with raw LLM output for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskPlan {
    /// Normalized search queries.
    pub queries: Vec<String>,
    /// Raw visible content returned by the model.
    pub content: String,
    /// Optional hidden/thinking trace returned by the model server.
    pub reasoning_content: Option<String>,
    /// Raw JSON response body returned by the model server.
    pub raw_response: String,
}

/// Answer synthesis result with raw LLM output for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskSynthesis {
    /// Trimmed answer text.
    pub answer: String,
    /// Raw visible content returned by the model.
    pub content: String,
    /// Optional hidden/thinking trace returned by the model server.
    pub reasoning_content: Option<String>,
    /// Raw JSON response body returned by the model server.
    pub raw_response: String,
}

impl AskClient {
    /// Builds an ask client from an existing chat client.
    #[must_use]
    pub fn new(chat: ChatClient) -> Self {
        Self {
            planning_chat: chat.clone(),
            synthesis_chat: chat,
        }
    }

    /// Builds an ask client with distinct planner and synthesis clients.
    #[must_use]
    pub const fn with_stage_clients(planning_chat: ChatClient, synthesis_chat: ChatClient) -> Self {
        Self {
            planning_chat,
            synthesis_chat,
        }
    }

    /// Plans search queries for a broad natural-language question.
    ///
    /// # Errors
    ///
    /// Returns [`AskError`] for transport failures. Malformed planner JSON
    /// gracefully falls back to the original question.
    pub fn plan_queries(&self, question: &str, limit: u8) -> Result<Vec<String>, AskError> {
        self.plan_queries_detailed(question, limit)
            .map(|plan| plan.queries)
    }

    /// Plans search queries and returns raw model output for diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`AskError`] for transport failures. Malformed planner JSON
    /// gracefully falls back to the original question.
    pub fn plan_queries_detailed(&self, question: &str, limit: u8) -> Result<AskPlan, AskError> {
        let output = self.planning_chat.complete_raw(
            vec![
                ChatMessage::new("system", PLAN_SYSTEM_PROMPT),
                ChatMessage::new("user", format!("Question: {question}")),
            ],
            ASK_TEMPERATURE,
        )?;
        let cleaned = strip_code_fences(&output.content);
        let body: AskPlanBody = match serde_json::from_str(&cleaned) {
            Ok(body) => body,
            Err(_) => {
                return Ok(AskPlan {
                    queries: Vec::new(),
                    content: output.content,
                    reasoning_content: output.reasoning_content,
                    raw_response: output.raw_response,
                });
            }
        };
        Ok(AskPlan {
            queries: normalize_queries(question, body.queries, limit),
            content: output.content,
            reasoning_content: output.reasoning_content,
            raw_response: output.raw_response,
        })
    }

    /// Synthesizes an answer from ranked vault snippets.
    ///
    /// # Errors
    ///
    /// Returns [`AskError`] for chat transport or response-shape failures.
    pub fn synthesize(
        &self,
        question: &str,
        queries: &[String],
        sources: &[AskSource],
    ) -> Result<String, AskError> {
        self.synthesize_detailed(question, queries, sources)
            .map(|synthesis| synthesis.answer)
    }

    /// Synthesizes an answer and returns raw model output for diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`AskError`] for chat transport or response-shape failures.
    pub fn synthesize_detailed(
        &self,
        question: &str,
        queries: &[String],
        sources: &[AskSource],
    ) -> Result<AskSynthesis, AskError> {
        let user_message = build_answer_user_message(question, queries, sources);
        let output: ChatCompletionOutput = self.synthesis_chat.complete_raw(
            vec![
                ChatMessage::new("system", ANSWER_SYSTEM_PROMPT),
                ChatMessage::new("user", user_message),
            ],
            ASK_TEMPERATURE,
        )?;
        Ok(AskSynthesis {
            answer: output.content.trim().to_owned(),
            content: output.content,
            reasoning_content: output.reasoning_content,
            raw_response: output.raw_response,
        })
    }

    /// Returns the configured ask model.
    #[must_use]
    pub fn model(&self) -> &str {
        self.planning_chat.model()
    }

    /// Returns the configured ask endpoint.
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.planning_chat.base_url()
    }
}

fn build_answer_user_message(question: &str, queries: &[String], sources: &[AskSource]) -> String {
    let mut message = format!("Question:\n{question}\n\nSearch queries:\n");
    for query in queries {
        message.push_str("- ");
        message.push_str(query);
        message.push('\n');
    }
    message.push_str("\nVault snippets:\n");
    for (index, source) in sources.iter().enumerate() {
        let _ = writeln!(
            message,
            "[{}] {}\nTitle: {}\nScore: {:.3}\nSnippet: {}\n",
            index + 1,
            source.vault_path.as_str(),
            source.title.as_str(),
            source.score,
            source.snippet.as_str()
        );
    }
    message
}

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

#[cfg(test)]
mod tests;
