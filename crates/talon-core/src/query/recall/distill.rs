use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::config::TalonConfig;
use crate::expansion::{ExpansionClient, ExpansionError};
use crate::text::{estimate_tokens, is_fence_line, nfd};

mod phrases;

use phrases::{WeightedPhrase, clean_phrase, extract_weighted_phrases, strip_code_blocks};

const DISTILLER_OVERHEAD_TOKENS: usize = 512;
const DEFAULT_QUERY_EMBEDDING_CONTEXT_TOKENS: usize = 512;
const MAX_QUERY_SET_SIZE: usize = 6;
const MAX_MAIN_QUERY_TOKENS: usize = 96;
const MAX_HINTS: usize = 16;
const DISTILLATION_MIN_REMAINING: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub(super) struct RecallQueryPlan {
    pub(super) main_query: String,
    pub(super) queries: Vec<String>,
    pub(super) input_tokens: usize,
    pub(super) query_tokens: usize,
    pub(super) phrase_count: usize,
    pub(super) distillation_ran: bool,
    pub(super) distillation_ms: Option<u64>,
    pub(super) distillation_succeeded: bool,
    pub(super) distillation_fallback_reason: Option<String>,
}

pub(super) fn plan_recall_queries(
    raw_query: &str,
    expansion: Option<&ExpansionClient>,
    config: Option<&TalonConfig>,
    deadline_at: Option<Instant>,
) -> RecallQueryPlan {
    let started = Instant::now();
    let phrases = extract_weighted_phrases(raw_query);
    let embedding_budget = query_embedding_budget(config);
    let query_tokens = estimate_tokens(raw_query);
    let noisy = noisy_prompt(raw_query);
    let should_distill = query_tokens > embedding_budget || noisy;

    let mut distillation_ran = false;
    let mut distillation_succeeded = false;
    let mut distillation_fallback_reason = None;
    let distilled = if should_distill && has_time_for_distillation(deadline_at) {
        if let Some(client) = expansion {
            distillation_ran = true;
            let view = budgeted_prompt_view(raw_query, config);
            let hints = phrase_hints(&phrases);
            match client.distill_recall_prompt(&view, &hints) {
                Ok(Some(body)) => {
                    distillation_succeeded = true;
                    Some(body)
                }
                Ok(None) => {
                    distillation_fallback_reason = Some("empty-or-malformed-response".to_owned());
                    None
                }
                Err(err) => {
                    distillation_fallback_reason = Some(classify_distillation_error(&err));
                    None
                }
            }
        } else {
            distillation_fallback_reason = Some("client-unavailable".to_owned());
            None
        }
    } else {
        if should_distill {
            distillation_fallback_reason = Some("deadline-too-close".to_owned());
        }
        None
    };
    let distillation_ms =
        distillation_ran.then(|| u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX));

    let main_query = distilled
        .as_ref()
        .map(|body| body.search_query.as_str())
        .filter(|query| !query.trim().is_empty())
        .map_or_else(
            || compact_main_query(raw_query, &phrases, embedding_budget),
            |query| cap_tokens(query, MAX_MAIN_QUERY_TOKENS),
        );

    let mut phrase_texts: Vec<String> = phrases.iter().map(|phrase| phrase.text.clone()).collect();
    if let Some(body) = distilled {
        phrase_texts.extend(body.phrases);
        phrase_texts.extend(body.identifiers);
    }

    let mut queries = vec![main_query];
    queries.extend(build_phrase_queries(&phrase_texts));
    let mut plan = dedupe_queries(queries);
    plan.input_tokens = query_tokens;
    plan.query_tokens = estimate_tokens(&plan.main_query);
    plan.phrase_count = phrases.len();
    plan.distillation_ran = distillation_ran;
    plan.distillation_ms = distillation_ms;
    plan.distillation_succeeded = distillation_succeeded;
    plan.distillation_fallback_reason = distillation_fallback_reason;
    plan
}

fn classify_distillation_error(error: &ExpansionError) -> String {
    match error {
        ExpansionError::Http {
            timed_out: true, ..
        } => "timeout".to_owned(),
        ExpansionError::Http {
            status: Some(status),
            ..
        } => format!("http-{status}"),
        ExpansionError::Http { .. } => "transport-error".to_owned(),
        ExpansionError::Build { .. } => "client-build-error".to_owned(),
    }
}

fn has_time_for_distillation(deadline_at: Option<Instant>) -> bool {
    deadline_at.is_none_or(|deadline| {
        deadline.saturating_duration_since(Instant::now()) > DISTILLATION_MIN_REMAINING
    })
}

fn query_embedding_budget(config: Option<&TalonConfig>) -> usize {
    config
        .map(|cfg| cfg.inference.models.query_embedding_context_tokens)
        .and_then(|tokens| usize::try_from(tokens).ok())
        .filter(|tokens| *tokens > 0)
        .unwrap_or(DEFAULT_QUERY_EMBEDDING_CONTEXT_TOKENS)
}

fn expansion_input_budget(config: Option<&TalonConfig>) -> usize {
    let Some(config) = config else {
        return 4_000;
    };
    let context = usize::try_from(config.expansion.context_tokens).unwrap_or(usize::MAX);
    let output = config
        .expansion
        .max_output_tokens
        .and_then(|tokens| usize::try_from(tokens).ok())
        .unwrap_or(768);
    context
        .saturating_sub(output + DISTILLER_OVERHEAD_TOKENS)
        .max(256)
}

fn noisy_prompt(query: &str) -> bool {
    let line_count = query.lines().count();
    let fence_count = query
        .lines()
        .filter(|line| is_fence_line(line.trim()))
        .count();
    line_count > 80 || fence_count >= 2 || query.contains("```") || query.contains("TRACE")
}

fn budgeted_prompt_view(query: &str, config: Option<&TalonConfig>) -> String {
    let stripped = strip_code_blocks(query);
    let budget = expansion_input_budget(config);
    if estimate_tokens(&stripped) <= budget {
        return stripped;
    }
    cap_tokens_head_tail(&stripped, budget)
}

fn compact_main_query(raw_query: &str, phrases: &[WeightedPhrase], budget: usize) -> String {
    if estimate_tokens(raw_query) <= budget {
        return raw_query.to_owned();
    }
    let phrase_query = phrase_hints(phrases)
        .into_iter()
        .take(8)
        .collect::<Vec<_>>()
        .join(" ");
    if !phrase_query.is_empty() {
        return cap_tokens(&phrase_query, MAX_MAIN_QUERY_TOKENS);
    }
    cap_tokens_tail(raw_query, budget.min(MAX_MAIN_QUERY_TOKENS))
}

fn phrase_hints(phrases: &[WeightedPhrase]) -> Vec<String> {
    phrases
        .iter()
        .take(MAX_HINTS)
        .map(|phrase| phrase.text.clone())
        .collect()
}

fn build_phrase_queries(phrases: &[String]) -> Vec<String> {
    let mut literals = Vec::new();
    let mut semantic_phrases = Vec::new();
    for phrase in phrases {
        let cleaned = clean_phrase(phrase);
        if cleaned.is_empty() {
            continue;
        }
        if looks_literal(&cleaned) {
            literals.push(cleaned);
        } else {
            semantic_phrases.push(cleaned);
        }
    }

    let mut queries = Vec::new();
    for chunk in semantic_phrases.chunks(4).take(3) {
        queries.push(chunk.join(" "));
    }
    if !literals.is_empty() {
        queries.push(literals.into_iter().take(8).collect::<Vec<_>>().join(" "));
    }
    queries
}

fn dedupe_queries(queries: Vec<String>) -> RecallQueryPlan {
    let mut seen = HashSet::new();
    let mut result = Vec::with_capacity(MAX_QUERY_SET_SIZE);
    for query in queries {
        let query = clean_phrase(&query);
        if query.is_empty() {
            continue;
        }
        let key = nfd::normalize(&query).to_lowercase();
        if seen.insert(key) {
            result.push(query);
            if result.len() >= MAX_QUERY_SET_SIZE {
                break;
            }
        }
    }
    let main_query = result.first().cloned().unwrap_or_default();
    RecallQueryPlan {
        main_query,
        queries: result,
        input_tokens: 0,
        query_tokens: 0,
        phrase_count: 0,
        distillation_ran: false,
        distillation_ms: None,
        distillation_succeeded: false,
        distillation_fallback_reason: None,
    }
}

fn looks_literal(value: &str) -> bool {
    value.contains('/')
        || value.contains('#')
        || value.contains('_')
        || value.contains("::")
        || value.chars().any(char::is_uppercase) && value.chars().any(char::is_lowercase)
}

fn cap_tokens(input: &str, budget: usize) -> String {
    if estimate_tokens(input) <= budget {
        return input.trim().to_owned();
    }
    let max_chars = budget
        .saturating_mul(usize::from(crate::text::TOKEN_CHAR_RATIO))
        .max(1);
    input
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn cap_tokens_tail(input: &str, budget: usize) -> String {
    if estimate_tokens(input) <= budget {
        return input.trim().to_owned();
    }
    let max_chars = budget
        .saturating_mul(usize::from(crate::text::TOKEN_CHAR_RATIO))
        .max(1);
    let mut chars: Vec<char> = input.chars().rev().take(max_chars).collect();
    chars.reverse();
    chars.into_iter().collect::<String>().trim().to_owned()
}

fn cap_tokens_head_tail(input: &str, budget: usize) -> String {
    let max_chars = budget
        .saturating_mul(usize::from(crate::text::TOKEN_CHAR_RATIO))
        .max(1);
    let half = max_chars / 2;
    let head: String = input.chars().take(half).collect();
    let mut tail_chars: Vec<char> = input
        .chars()
        .rev()
        .take(max_chars.saturating_sub(half))
        .collect();
    tail_chars.reverse();
    let tail: String = tail_chars.into_iter().collect();
    format!("{head}\n\n[...]\n\n{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_recall_queries_compacts_large_prompt() {
        let prompt = "context overflow ".repeat(800);
        let plan = plan_recall_queries(&prompt, None, None, None);
        assert!(estimate_tokens(&plan.main_query) <= MAX_MAIN_QUERY_TOKENS);
        assert!(!plan.queries.is_empty());
    }
}
