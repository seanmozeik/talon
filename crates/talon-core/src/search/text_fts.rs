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
            terms.push(("quoted", c));
        } else {
            let mut t = String::new();
            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '"' {
                t.push(chars[i]);
                i += 1;
            }
            if !t.is_empty() {
                terms.push(("bare", t));
            }
        }
    }
    if terms.is_empty() {
        return LITERAL_EMPTY_FTS.to_string();
    }

    let mut formatted = Vec::new();
    let re = hyphenated_token_regex();
    for (kind, content) in terms {
        if kind == "quoted" {
            let s = sanitize_fts_query(&content);
            if !s.is_empty() {
                formatted.push(format!("\"{s}\""));
            }
        } else if re.is_match(&content) {
            let s = content.replace('-', " ");
            formatted.push(format!("\"{s}\""));
        } else {
            let s = sanitize_fts_query(&content);
            for w in s.split_whitespace() {
                formatted.push(format!("\"{w}\"*"));
            }
        }
    }
    if formatted.is_empty() {
        return LITERAL_EMPTY_FTS.to_string();
    }
    let joiner = format!(" {} ", operator.keyword());
    formatted.join(&joiner)
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
mod tests {
    use super::*;

    #[test]
    fn get_trigrams_returns_lowercase_string_for_short_input() {
        let t = get_trigrams("AB");
        assert!(t.contains("ab"));
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn get_trigrams_slides_window_for_long_input() {
        let t = get_trigrams("Hello");
        // "hel", "ell", "llo"
        assert!(t.contains("hel"));
        assert!(t.contains("ell"));
        assert!(t.contains("llo"));
        assert_eq!(t.len(), 3);
    }

    #[test]
    fn trigram_overlap_full_match_returns_one() {
        assert_eq!(calculate_trigram_overlap("hello", "hello"), 1.0);
    }

    #[test]
    fn trigram_overlap_no_match_returns_zero() {
        assert_eq!(calculate_trigram_overlap("xyz", "abc"), 0.0);
    }

    #[test]
    fn trigram_overlap_partial_match() {
        // "hello" has 3 trigrams (hel, ell, llo); "help" has 2 (hel, elp).
        // Overlap = matches in query trigrams (hel, ell, llo) found in title trigrams (hel, elp).
        // Only "hel" matches → 1/3.
        let overlap = calculate_trigram_overlap("hello", "help");
        assert!((overlap - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn sanitize_strips_special_chars_and_collapses_whitespace() {
        // Special chars become spaces, then runs of whitespace collapse to one.
        assert_eq!(sanitize_fts_query("  foo \"bar*  baz^ "), "foo bar baz");
    }

    #[test]
    fn sanitize_empty_returns_empty_string() {
        assert_eq!(sanitize_fts_query("   "), "");
    }

    #[test]
    fn to_fts_query_and_default() {
        assert_eq!(
            to_fts_query("foo bar", FtsOperator::And),
            "\"foo\"* AND \"bar\"*"
        );
    }

    #[test]
    fn to_fts_query_or() {
        assert_eq!(
            to_fts_query("foo bar baz", FtsOperator::Or),
            "\"foo\"* OR \"bar\"* OR \"baz\"*"
        );
    }

    #[test]
    fn to_fts_query_empty_returns_literal_empty() {
        assert_eq!(to_fts_query("", FtsOperator::And), LITERAL_EMPTY_FTS);
        assert_eq!(to_fts_query("   ", FtsOperator::And), LITERAL_EMPTY_FTS);
    }

    #[test]
    fn to_fts_query_strips_special_chars_before_quoting() {
        assert_eq!(
            to_fts_query("foo*bar", FtsOperator::And),
            "\"foo\"* AND \"bar\"*"
        );
    }

    #[test]
    fn build_trigram_or_query_quotes_each_trigram() {
        assert_eq!(
            build_trigram_or_query("hello"),
            "\"hel\" OR \"ell\" OR \"llo\""
        );
    }

    #[test]
    fn build_trigram_or_query_short_input_returns_quoted_text() {
        assert_eq!(build_trigram_or_query("ab"), "\"ab\"");
    }

    #[test]
    fn build_trigram_or_query_strips_inner_quotes() {
        // `r#"a"b"# = `a"b`; with "ab" stripped of quotes inside → `"a"`,`"ab"`
        assert!(!build_trigram_or_query("a\"bc").contains("\"a\"b\""));
    }

    #[test]
    fn build_bm25_score_zero_returns_zero() {
        assert_eq!(build_bm25_score(0.0), 0.0);
    }

    #[test]
    fn build_bm25_score_negative_input_normalizes_to_unit_interval() {
        // FTS5 returns negative scores (more negative = better match).
        let s = build_bm25_score(-2.0);
        assert!((s - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn build_bm25_score_positive_input_also_works() {
        let s = build_bm25_score(3.0);
        assert!((s - 0.75).abs() < 1e-9);
    }

    #[test]
    fn build_bm25_score_is_bounded_below_one() {
        // As |raw| → ∞, score → 1 but never reaches it.
        assert!(build_bm25_score(1000.0) < 1.0);
        assert!(build_bm25_score(1000.0) > 0.99);
    }

    #[test]
    fn to_fts_query_hyphenated_tokens() {
        assert_eq!(to_fts_query("gpt-4", FtsOperator::And), "\"gpt 4\"");
        assert_eq!(to_fts_query("a-b-c-d", FtsOperator::And), "\"a b c d\"");
        assert_eq!(to_fts_query("DEC-0054", FtsOperator::And), "\"DEC 0054\"");
        assert_eq!(
            to_fts_query("gpt-4 foo", FtsOperator::And),
            "\"gpt 4\" AND \"foo\"*"
        );
    }

    #[test]
    fn to_fts_query_preserves_quotes_and_hyphenated() {
        assert_eq!(
            to_fts_query("foo \"bar baz\"", FtsOperator::And),
            "\"foo\"* AND \"bar baz\""
        );
        assert_eq!(
            to_fts_query("gpt-4 \"bar baz\" multi-agent", FtsOperator::Or),
            "\"gpt 4\" OR \"bar baz\" OR \"multi agent\""
        );
    }
}
