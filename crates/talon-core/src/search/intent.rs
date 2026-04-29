//! Intent helpers for disambiguating ambiguous search queries.

use serde::{Deserialize, Deserializer};

use crate::text::nfd;

const INTENT_STOP_WORDS: &[&str] = &[
    "am", "an", "as", "at", "be", "by", "do", "he", "if", "in", "is", "it", "me", "my", "no", "of",
    "on", "or", "so", "to", "up", "us", "we", "all", "and", "any", "are", "but", "can", "did",
    "for", "get", "has", "her", "him", "his", "how", "its", "let", "may", "not", "our", "out",
    "the", "too", "was", "who", "why", "you", "also", "does", "find", "from", "have", "into",
    "more", "need", "show", "some", "tell", "that", "them", "this", "want", "what", "when", "will",
    "with", "your", "about", "looking", "notes", "search", "where", "which",
];

/// Extracts meaningful lowercase terms from an intent string.
#[must_use]
pub fn extract_terms(intent: &str) -> Vec<String> {
    // Algorithm ported verbatim from qmd — store.ts:3820-3845
    nfd::normalize(intent)
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|term| term.len() > 1 && !INTENT_STOP_WORDS.contains(term))
        .map(ToOwned::to_owned)
        .collect()
}

/// Prepends intent context to a sidecar query string when present.
#[must_use]
pub fn prefix_query(intent: Option<&str>, query: &str) -> String {
    let Some(intent) = intent.and_then(normalize) else {
        return query.to_owned();
    };
    format!("Intent: {intent}\n\nQuery: {query}")
}

/// Trims and normalizes optional intent text.
#[must_use]
pub fn normalize_optional(intent: Option<String>) -> Option<String> {
    intent.and_then(|value| normalize(&value).map(ToOwned::to_owned))
}

pub(crate) fn deserialize_optional<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(normalize_optional)
}

fn normalize(intent: &str) -> Option<&str> {
    let trimmed = intent.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_terms_keeps_short_domain_terms_and_drops_stop_words() {
        assert_eq!(
            extract_terms("the API for SQL queries"),
            ["api", "sql", "queries"]
        );
    }

    #[test]
    fn prefix_query_prepends_non_empty_intent() {
        assert_eq!(
            prefix_query(Some("foo"), "bar"),
            "Intent: foo\n\nQuery: bar"
        );
    }

    #[test]
    fn prefix_query_returns_query_when_intent_is_none() {
        assert_eq!(prefix_query(None, "bar"), "bar");
    }

    #[test]
    fn prefix_query_returns_query_when_intent_is_blank() {
        assert_eq!(prefix_query(Some("  "), "bar"), "bar");
    }

    #[test]
    fn normalize_optional_trims_and_drops_empty_values() {
        assert_eq!(
            normalize_optional(Some("  web load  ".to_owned())),
            Some("web load".to_owned())
        );
        assert_eq!(normalize_optional(Some("  ".to_owned())), None);
        assert_eq!(normalize_optional(None), None);
    }
}
