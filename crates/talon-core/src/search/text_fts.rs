//! Pure helpers for FTS5 query construction and BM25 score normalization.
//!
//! Ported from `services/talon/search/text-fts.ts`. The trigram routines
//! match the reference byte-for-byte for ASCII input; for multibyte text the
//! TS implementation indexes by UTF-16 code units while we slice by Unicode
//! scalar values. That divergence shows up only when a query straddles a
//! surrogate pair (rare in vault content) and the resulting ranking remains
//! within the score-tolerance bounds of the parity tests.

use super::constants::{LITERAL_EMPTY_FTS, TRIGRAM_LEN};
use crate::numeric::count_u32;
use crate::text::nfd;
use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

/// FTS5 boolean operator joining query terms.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FtsOperator {
    /// All terms must match (AND).
    #[default]
    And,
    /// Any term may match (OR). Used for BM25 retrieval so documents are
    /// ranked by *how many* query terms they hit.
    Or,
}

impl FtsOperator {
    const fn keyword(self) -> &'static str {
        match self {
            Self::And => "AND",
            Self::Or => "OR",
        }
    }
}

/// Returns the lowercase-trigram set for `text`, using NFD normalization.
///
/// Shorter text becomes a single trigram; longer text yields a sliding window.
/// NFD normalization handles accented and composed characters uniformly.
#[must_use]
pub fn get_trigrams(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let lower = nfd::normalize(text).to_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    if chars.len() < TRIGRAM_LEN {
        out.insert(lower);
        return out;
    }
    for window in chars.windows(TRIGRAM_LEN) {
        out.insert(window.iter().collect());
    }
    out
}

/// Returns the fraction of `query`'s trigrams that appear in `title`.
/// Returns `0.0` if `query` produces no trigrams.
#[must_use]
pub fn calculate_trigram_overlap(query: &str, title: &str) -> f64 {
    let q = get_trigrams(query);
    let q_len = count_u32(q.len());
    if q_len == 0 {
        return 0.0;
    }
    let t = get_trigrams(title);
    let matches = count_u32(q.iter().filter(|tg| t.contains(*tg)).count());
    f64::from(matches) / f64::from(q_len)
}

/// Strips characters that have special meaning to FTS5 and collapses runs of
/// whitespace into single spaces.
#[must_use]
pub fn sanitize_fts_query(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    let mut last_space = true;
    for ch in query.chars() {
        let mapped = if matches!(ch, '"' | '*' | '^' | '(' | ')') || ch.is_whitespace() {
            ' '
        } else {
            ch
        };
        if mapped == ' ' {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(mapped);
            last_space = false;
        }
    }
    out.trim().to_string()
}

/// Returns a static compiled regex matching hyphenated tokens:
/// `\b[a-zA-Z][a-zA-Z0-9]*(-[a-zA-Z0-9]+)+\b`.
///
/// Algorithm ported verbatim from qmd — store.ts:2959-2971.
#[allow(clippy::expect_used)]
fn hyphenated_token_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b[a-zA-Z][a-zA-Z0-9]*(-[a-zA-Z0-9]+)+\b")
            .expect("hyphenated token regex is valid")
    })
}

enum ParsedFtsTerm {
    Quoted(String),
    Bare(String),
    Negative(String),
}

/// Wraps each word in `query` as a quoted FTS5 prefix term, joined by
/// `operator`. Returns [`LITERAL_EMPTY_FTS`] for empty queries.
///
/// Hyphenated tokens (e.g. `gpt-4`) are rewritten as FTS5 phrases.
#[must_use]
pub fn to_fts_query(query: &str, operator: FtsOperator) -> String {
    let mut terms = Vec::new();
    let chars: Vec<char> = query.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        if chars[i] == '"' {
            let mut c = String::new();
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                c.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 1;
            }
            terms.push(ParsedFtsTerm::Quoted(c));
        } else {
            let mut t = String::new();
            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '"' {
                t.push(chars[i]);
                i += 1;
            }
            if !t.is_empty() {
                if let Some(negative) = t.strip_prefix('-').filter(|s| !s.is_empty()) {
                    terms.push(ParsedFtsTerm::Negative(negative.to_string()));
                } else {
                    terms.push(ParsedFtsTerm::Bare(t));
                }
            }
        }
    }
    if terms.is_empty() {
        return LITERAL_EMPTY_FTS.to_string();
    }

    let mut formatted = Vec::new();
    let mut negative_terms = Vec::new();
    let re = hyphenated_token_regex();
    for term in terms {
        match term {
            ParsedFtsTerm::Quoted(content) => {
                let s = sanitize_fts_query(&content);
                if !s.is_empty() {
                    formatted.push(format!("\"{s}\""));
                }
            }
            ParsedFtsTerm::Bare(content) => {
                if re.is_match(&content) {
                    let s = content.replace('-', " ");
                    formatted.push(format!("\"{s}\""));
                } else {
                    let s = sanitize_fts_query(&content);
                    for w in s.split_whitespace() {
                        formatted.push(format!("\"{w}\"*"));
                    }
                }
            }
            ParsedFtsTerm::Negative(content) => {
                if re.is_match(&content) {
                    let s = content.replace('-', " ");
                    negative_terms.push(format!("\"{s}\""));
                } else {
                    let s = sanitize_fts_query(&content);
                    for w in s.split_whitespace() {
                        negative_terms.push(format!("\"{w}\"*"));
                    }
                }
            }
        }
    }
    if formatted.is_empty() {
        if negative_terms.is_empty() {
            return LITERAL_EMPTY_FTS.to_string();
        }
        return String::new();
    }
    let joiner = format!(" {} ", operator.keyword());
    let mut query = formatted.join(&joiner);
    for negative in negative_terms {
        query.push_str(" NOT ");
        query.push_str(&negative);
    }
    query
}

/// Builds an FTS5 OR query of all trigrams in `text`, used against the
/// trigram-tokenized `notes_fts_fuzzy` index.
#[must_use]
pub fn build_trigram_or_query(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < TRIGRAM_LEN {
        return format!("\"{}\"", text.replace('"', ""));
    }
    let parts: Vec<String> = chars
        .windows(TRIGRAM_LEN)
        .map(|w| {
            let s: String = w.iter().collect();
            format!("\"{}\"", s.replace('"', ""))
        })
        .collect();
    parts.join(" OR ")
}

/// Maps a raw BM25 score (negative for stronger matches in FTS5) to `[0, 1]`.
///
/// `score = max(0, |raw| / (1 + |raw|))`.
#[must_use]
pub fn build_bm25_score(raw: f64) -> f64 {
    let abs = raw.abs();
    (abs / (1.0 + abs)).max(0.0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]
mod tests;
