//! Constrained ask-model missing-link suggestions.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::config::TalonConfig;
use crate::llm::ChatError;
use crate::llm::{ChatClient, ChatMessage};

use super::GraphSnapshot;
use super::suggest::{
    LinkSuggestion, PROVENANCE_LLM, active_note_bodies, existing_edges, line_mentions_term,
    target_dictionary,
};
use crate::TalonError;

const SYSTEM_PROMPT: &str = "Return only JSON. Suggest missing Obsidian wikilinks from source terms to existing target paths. Never suggest edits.";
const MAX_NOTES: usize = 12;
const MAX_LINES_PER_NOTE: usize = 40;
const MAX_TARGETS: usize = 80;
pub(super) const ASK_SUGGESTION_TIMEOUT: Duration = Duration::from_mins(2);

/// Ask-mode client used for read-only missing-link suggestions during sync.
#[derive(Debug, Clone)]
pub struct GraphSuggestionClient {
    chat: ChatClient,
}

impl GraphSuggestionClient {
    /// Builds a suggestion client from the configured ask-mode chat client.
    #[must_use]
    pub const fn new(chat: ChatClient) -> Self {
        Self { chat }
    }

    /// Builds a suggestion client when `[ask].model` is explicitly configured.
    ///
    /// # Errors
    ///
    /// Returns [`ChatError::Build`] if the underlying HTTP client cannot be built.
    pub fn from_config(config: &TalonConfig) -> Result<Option<Self>, ChatError> {
        let Some(model) = config.ask.model.as_deref() else {
            return Ok(None);
        };
        let mut chat = ChatClient::with_timeout_and_max_tokens(
            config.expansion.base_url.clone(),
            model,
            ASK_SUGGESTION_TIMEOUT,
            Some(config.ask.max_output_tokens.min(512)),
        )?;
        if let Some(reasoning_effort) = config.ask.planning_reasoning_effort {
            chat = chat.with_reasoning_effort(reasoning_effort);
        }
        if let Some(kwargs) = merged_chat_template_kwargs(
            config.ask.planning_enable_thinking,
            config.ask.planning_chat_template_kwargs.as_ref(),
        ) {
            chat = chat.with_chat_template_kwargs(kwargs);
        }
        Ok(Some(Self::new(chat)))
    }
}

fn merged_chat_template_kwargs(
    enable_thinking: Option<bool>,
    chat_template_kwargs: Option<&BTreeMap<String, Value>>,
) -> Option<Value> {
    let mut merged: Map<String, Value> = chat_template_kwargs
        .into_iter()
        .flat_map(|kwargs| {
            kwargs
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
        })
        .collect();
    if let Some(enable_thinking) = enable_thinking {
        merged.insert("enable_thinking".to_string(), Value::Bool(enable_thinking));
    }
    (!merged.is_empty()).then_some(Value::Object(merged))
}

/// Builds LLM-assisted suggestions, then validates each candidate deterministically.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] when note content cannot be read. Chat
/// failures produce no suggestions because deterministic suggestions are still
/// available and sync should remain best-effort for this read-only lint aid.
pub fn build_llm_link_suggestions(
    conn: &rusqlite::Connection,
    snapshot: &GraphSnapshot,
    client: &GraphSuggestionClient,
) -> Result<Vec<LinkSuggestion>, TalonError> {
    let dictionary = target_dictionary(snapshot);
    if dictionary.is_empty() {
        return Ok(Vec::new());
    }
    let note_bodies = active_note_bodies(conn)?;
    let prompt = build_prompt(&note_bodies, &dictionary);
    let Ok(content) = client.chat.complete(
        vec![
            ChatMessage {
                role: "system".into(),
                content: SYSTEM_PROMPT.into(),
            },
            ChatMessage {
                role: "user".into(),
                content: prompt,
            },
        ],
        0.0,
    ) else {
        return Ok(Vec::new());
    };
    Ok(validate_llm_candidates(
        parse_candidates(&content),
        snapshot,
        note_bodies.as_slice(),
    ))
}

fn build_prompt(
    note_bodies: &[(String, String)],
    dictionary: &[(String, String, String)],
) -> String {
    let targets = dictionary
        .iter()
        .take(MAX_TARGETS)
        .map(|(_, target, term)| format!(r#"{{"target":"{target}","term":"{term}"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let notes = note_bodies
        .iter()
        .take(MAX_NOTES)
        .map(|(path, body)| format_note(path, body))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Targets:[{targets}]\nNotes:\n{notes}\nReturn JSON array of {{\"path\",\"target\",\"term\",\"line\"}}."
    )
}

fn format_note(path: &str, body: &str) -> String {
    let lines = body
        .lines()
        .take(MAX_LINES_PER_NOTE)
        .enumerate()
        .map(|(index, line)| format!("{}: {}", index + 1, line))
        .collect::<Vec<_>>()
        .join("\n");
    format!("Path: {path}\n{lines}")
}

#[derive(Debug, Deserialize)]
pub(super) struct LlmCandidate {
    pub(super) path: String,
    pub(super) target: String,
    pub(super) term: String,
    pub(super) line: u32,
}

fn parse_candidates(content: &str) -> Vec<LlmCandidate> {
    let json = content
        .trim()
        .strip_prefix("```json")
        .or_else(|| content.trim().strip_prefix("```"))
        .unwrap_or_else(|| content.trim())
        .trim()
        .strip_suffix("```")
        .unwrap_or_else(|| content.trim())
        .trim();
    serde_json::from_str(json).unwrap_or_default()
}

pub(super) fn validate_llm_candidates(
    candidates: Vec<LlmCandidate>,
    snapshot: &GraphSnapshot,
    note_bodies: &[(String, String)],
) -> Vec<LinkSuggestion> {
    let body_by_path = note_bodies
        .iter()
        .map(|(path, body)| (path.as_str(), body.as_str()))
        .collect::<BTreeMap<_, _>>();
    let existing = existing_edges(snapshot);
    let dictionary = target_dictionary(snapshot)
        .into_iter()
        .map(|(_, target, term)| ((target, term.to_lowercase()), term))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut suggestions = Vec::new();
    for candidate in candidates {
        if !snapshot.nodes.contains_key(&candidate.target)
            || candidate.path == candidate.target
            || existing.contains(&(candidate.path.clone(), candidate.target.clone()))
            || !seen.insert((candidate.path.clone(), candidate.target.clone()))
        {
            continue;
        }
        let Some(term) = dictionary
            .get(&(candidate.target.clone(), candidate.term.to_lowercase()))
            .cloned()
        else {
            continue;
        };
        let Some(body) = body_by_path.get(candidate.path.as_str()) else {
            continue;
        };
        let Some(line) = line_at(body, candidate.line) else {
            continue;
        };
        if line_mentions_term(line, &term.to_lowercase()) {
            suggestions.push(LinkSuggestion {
                path: candidate.path,
                target: candidate.target,
                term,
                line: candidate.line,
                provenance: PROVENANCE_LLM.into(),
            });
        }
    }
    suggestions.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.target.cmp(&right.target))
            .then_with(|| left.term.cmp(&right.term))
    });
    suggestions
}

fn line_at(body: &str, line: u32) -> Option<&str> {
    let index = usize::try_from(line).ok()?.checked_sub(1)?;
    body.lines().nth(index)
}
