use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;

use crate::text::{is_fence_line, nfd, strip_heading_text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PhraseSource {
    Literal,
    Identifier,
    Heading,
    ProperPhrase,
    RepeatedPhrase,
}

#[derive(Debug, Clone)]
pub(super) struct WeightedPhrase {
    pub(super) text: String,
    pub(super) weight: f64,
    pub(super) source: PhraseSource,
}

pub(super) fn extract_weighted_phrases(input: &str) -> Vec<WeightedPhrase> {
    let visible = strip_code_blocks(input);
    let mut weighted: HashMap<String, WeightedPhrase> = HashMap::new();
    collect_regex(
        &visible,
        quoted_re(),
        PhraseSource::Literal,
        1.5,
        &mut weighted,
    );
    collect_regex(
        &visible,
        wikilink_re(),
        PhraseSource::Literal,
        1.5,
        &mut weighted,
    );
    collect_regex(
        &visible,
        tag_re(),
        PhraseSource::Literal,
        1.5,
        &mut weighted,
    );
    collect_regex(
        &visible,
        path_re(),
        PhraseSource::Literal,
        1.5,
        &mut weighted,
    );
    collect_regex(
        &visible,
        identifier_re(),
        PhraseSource::Identifier,
        1.5,
        &mut weighted,
    );

    for line in visible.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            let heading = strip_heading_text(trimmed).trim().to_owned();
            push_phrase(&mut weighted, &heading, PhraseSource::Heading, 1.4);
        }
    }
    collect_yake_phrases(&visible, &mut weighted);
    collect_regex(
        &visible,
        proper_phrase_re(),
        PhraseSource::ProperPhrase,
        1.2,
        &mut weighted,
    );
    collect_repeated_phrases(&visible, &mut weighted);

    let mut phrases: Vec<WeightedPhrase> = weighted.into_values().collect();
    phrases.sort_by(|a, b| {
        b.weight
            .total_cmp(&a.weight)
            .then_with(|| source_rank(b.source).cmp(&source_rank(a.source)))
            .then_with(|| a.text.cmp(&b.text))
    });
    phrases.truncate(24);
    phrases
}

pub(super) fn strip_code_blocks(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    let mut in_fence = false;
    for line in query.lines() {
        if is_fence_line(line.trim()) {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

pub(super) fn clean_phrase(text: &str) -> String {
    text.trim()
        .trim_matches(|ch: char| ch.is_ascii_punctuation() && ch != '#' && ch != '/')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn collect_regex(
    input: &str,
    regex: &Regex,
    source: PhraseSource,
    weight: f64,
    weighted: &mut HashMap<String, WeightedPhrase>,
) {
    for captures in regex.captures_iter(input) {
        let text = captures.get(1).or_else(|| captures.get(0));
        if let Some(text) = text {
            push_phrase(weighted, text.as_str(), source, weight);
        }
    }
}

fn collect_repeated_phrases(input: &str, weighted: &mut HashMap<String, WeightedPhrase>) {
    let normalized = nfd::normalize(input).to_lowercase();
    let words: Vec<&str> = normalized
        .split(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_')
        .filter(|word| word.len() > 2 && !is_stop_word(word))
        .collect();
    let mut counts: HashMap<String, u32> = HashMap::new();
    for window in words.windows(2) {
        let phrase = window.join(" ");
        *counts.entry(phrase).or_default() += 1;
    }
    for (phrase, count) in counts {
        if count > 1 {
            push_phrase(
                weighted,
                &phrase,
                PhraseSource::RepeatedPhrase,
                repetition_weight(count),
            );
        }
    }
}

fn collect_yake_phrases(input: &str, weighted: &mut HashMap<String, WeightedPhrase>) {
    let config = yake_rust::Config {
        ngrams: 3,
        remove_duplicates: true,
        minimum_chars: 4,
        only_alphanumeric_and_hyphen: false,
        ..yake_rust::Config::default()
    };
    let stop_words = yake_rust::StopWords::predefined("en").unwrap_or_default();
    // yake -> segtok -> fancy-regex blows its backtrack limit (panic) on very
    // large inputs. Bound the input first so the backtrack never triggers, and
    // keep catch_unwind as a backstop for pathological smaller inputs (TOO-44).
    let bounded = crate::text::truncate_on_char_boundary(input, crate::text::YAKE_INPUT_MAX_BYTES);
    let best = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        yake_rust::get_n_best(12, bounded, &stop_words, &config)
    }))
    .unwrap_or_default();
    for item in best {
        push_phrase(
            weighted,
            &item.raw,
            PhraseSource::ProperPhrase,
            yake_weight(item.score),
        );
    }
}

fn yake_weight(score: f64) -> f64 {
    let normalized = (1.0 / (1.0 + score.clamp(0.0, 100.0))).clamp(0.0, 1.0);
    0.8 + normalized
}

fn repetition_weight(count: u32) -> f64 {
    let bounded = u16::try_from(count.min(20)).unwrap_or(20);
    f64::from(bounded).mul_add(0.1, 1.0)
}

fn push_phrase(
    weighted: &mut HashMap<String, WeightedPhrase>,
    text: &str,
    source: PhraseSource,
    weight: f64,
) {
    let text = clean_phrase(text);
    if text.len() < 3 || text.split_whitespace().count() > 8 {
        return;
    }
    let key = nfd::normalize(&text).to_lowercase();
    weighted
        .entry(key)
        .and_modify(|phrase| phrase.weight += weight)
        .or_insert(WeightedPhrase {
            text,
            weight,
            source,
        });
}

fn is_stop_word(word: &str) -> bool {
    matches!(
        word,
        "the" | "and" | "for" | "with" | "that" | "this" | "from" | "into" | "have" | "will"
    )
}

const fn source_rank(source: PhraseSource) -> u8 {
    match source {
        PhraseSource::Literal => 5,
        PhraseSource::Identifier => 4,
        PhraseSource::Heading => 3,
        PhraseSource::ProperPhrase => 2,
        PhraseSource::RepeatedPhrase => 1,
    }
}

fn quoted_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| compile_static(r#""([^"\n]{3,120})"|'([^'\n]{3,120})'"#))
}

fn wikilink_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| compile_static(r"\[\[([^\]|#]{3,120})(?:[#|][^\]]*)?\]\]"))
}

fn tag_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| compile_static(r"(?m)(?:^|\s)(#[[:alnum:]_/-]{2,80})\b"))
}

fn path_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| compile_static(r"\b([[:alnum:]_.-]+(?:/[[:alnum:]_. -]+)+\.md)\b"))
}

fn identifier_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        compile_static(r"\b([A-Za-z][A-Za-z0-9]*(?:_[A-Za-z0-9]+|::[A-Za-z0-9]+|[A-Z][a-z0-9]+)[A-Za-z0-9_:]*)\b")
    })
}

fn proper_phrase_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| compile_static(r"\b([A-Z][a-zA-Z0-9]+(?:\s+[A-Z][a-zA-Z0-9]+){1,5})\b"))
}

fn compile_static(pattern: &str) -> Regex {
    match Regex::new(pattern) {
        Ok(re) => re,
        Err(err) => unreachable!("static regex {pattern:?} did not compile: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_weighted_phrases_keeps_literals_and_identifiers() {
        let phrases = extract_weighted_phrases(
            r#"Recall [[MCP hook recall]] for "context overflow" in talon_hook_recall and #memory/retrieval."#,
        );
        let texts: Vec<&str> = phrases.iter().map(|phrase| phrase.text.as_str()).collect();
        assert!(texts.contains(&"MCP hook recall"));
        assert!(texts.contains(&"context overflow"));
        assert!(texts.contains(&"talon_hook_recall"));
        assert!(texts.contains(&"#memory/retrieval"));
    }

    #[test]
    fn strip_code_blocks_removes_bulk() {
        let stripped = strip_code_blocks("Keep this\n```rust\nfn noisy() {}\n```\nAnd this");
        assert!(stripped.contains("Keep this"));
        assert!(stripped.contains("And this"));
        assert!(!stripped.contains("noisy"));
    }
}
