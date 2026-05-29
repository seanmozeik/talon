//! Read-only missing-link suggestions built during graph sync.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::TalonError;

use super::GraphSnapshot;

/// Persisted read-only link suggestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkSuggestion {
    /// Source note path.
    pub path: String,
    /// Existing target note path.
    pub target: String,
    /// Matched title or alias term.
    pub term: String,
    /// 1-based body line number.
    pub line: Option<u32>,
}

/// Builds deterministic missing-link suggestions for active notes.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] when note content cannot be read.
pub fn build_missing_link_suggestions(
    conn: &Connection,
    snapshot: &GraphSnapshot,
) -> Result<Vec<LinkSuggestion>, TalonError> {
    let dictionary = target_dictionary(snapshot);
    if dictionary.is_empty() {
        return Ok(Vec::new());
    }
    let existing = existing_edges(snapshot);
    let mut suggestions = Vec::new();
    for (path, content) in active_note_bodies(conn)? {
        let mut per_target = BTreeSet::new();
        let mut in_fence = false;
        let salient_terms = salient_terms(&content);
        for (line_index, line) in content.lines().enumerate() {
            if toggles_fence(line) {
                in_fence = !in_fence;
                continue;
            }
            if in_fence || line.trim_start().starts_with("---") {
                continue;
            }
            for (term_norm, target, term) in &dictionary {
                if path == *target
                    || existing.contains(&(path.clone(), target.clone()))
                    || per_target.contains(target)
                {
                    continue;
                }
                if line_mentions_term(line, term_norm, &salient_terms) {
                    per_target.insert(target.clone());
                    suggestions.push(LinkSuggestion {
                        path: path.clone(),
                        target: target.clone(),
                        term: term.clone(),
                        line: Some(u32::try_from(line_index + 1).unwrap_or(u32::MAX)),
                    });
                }
            }
        }
    }
    suggestions.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.target.cmp(&right.target))
            .then_with(|| left.term.cmp(&right.term))
    });
    Ok(suggestions)
}

pub(super) fn target_dictionary(snapshot: &GraphSnapshot) -> Vec<(String, String, String)> {
    let mut terms = BTreeMap::new();
    for node in snapshot.nodes.values().filter(|node| !node.structural) {
        for term in
            std::iter::once(node.title.as_str()).chain(node.aliases.iter().map(String::as_str))
        {
            let normalized = normalize_term(term);
            if normalized.len() >= 4 {
                terms
                    .entry((normalized, node.vault_path.clone()))
                    .or_insert_with(|| term.to_string());
            }
        }
    }
    terms
        .into_iter()
        .map(|((normalized, target), term)| (normalized, target, term))
        .collect()
}

pub(super) fn active_note_bodies(conn: &Connection) -> Result<Vec<(String, String)>, TalonError> {
    let mut stmt = conn
        .prepare("SELECT vault_path, content FROM notes WHERE active = 1 ORDER BY vault_path")
        .map_err(|source| TalonError::Sqlite {
            context: "load graph suggestion bodies",
            source,
        })?;
    stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|source| TalonError::Sqlite {
            context: "load graph suggestion bodies",
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| TalonError::Sqlite {
            context: "load graph suggestion bodies",
            source,
        })
}

pub(super) fn existing_edges(snapshot: &GraphSnapshot) -> BTreeSet<(String, String)> {
    snapshot
        .edges
        .iter()
        .map(|edge| (edge.from_path.clone(), edge.to_path.clone()))
        .collect()
}

pub(super) fn line_mentions_term(
    line: &str,
    term_norm: &str,
    salient_terms: &BTreeSet<String>,
) -> bool {
    let searchable = mask_excluded_spans(line);
    let lower = searchable.to_lowercase();
    lower.match_indices(term_norm).any(|(start, _)| {
        let end = start + term_norm.len();
        has_boundaries(&lower, start, end)
            && eligible_surface_match(&searchable[start..end], term_norm, salient_terms)
    })
}

fn eligible_surface_match(
    surface: &str,
    term_norm: &str,
    salient_terms: &BTreeSet<String>,
) -> bool {
    if !has_case_signal(surface) {
        return false;
    }
    if is_multi_word(term_norm) {
        return true;
    }
    term_norm.chars().count() >= 8 && salient_terms.contains(term_norm)
}

fn has_case_signal(surface: &str) -> bool {
    surface.chars().any(char::is_uppercase)
}

fn is_multi_word(term_norm: &str) -> bool {
    term_norm
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .nth(1)
        .is_some()
}

fn salient_terms(content: &str) -> BTreeSet<String> {
    let visible = visible_text(content);
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
    let bounded =
        crate::text::truncate_on_char_boundary(&visible, crate::text::YAKE_INPUT_MAX_BYTES);
    let best = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        yake_rust::get_n_best(32, bounded, &stop_words, &config)
    }))
    .unwrap_or_default();
    best.into_iter()
        .map(|item| normalize_term(&item.raw))
        .collect()
}

fn visible_text(content: &str) -> String {
    let mut visible = String::with_capacity(content.len());
    let mut in_fence = false;
    for line in content.lines() {
        if toggles_fence(line) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || line.trim_start().starts_with("---") {
            continue;
        }
        visible.push_str(&mask_excluded_spans(line));
        visible.push('\n');
    }
    visible
}

fn mask_excluded_spans(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if ch == '`' {
            out.push(' ');
            for next in chars.by_ref() {
                out.push(' ');
                if next == '`' {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    mask_between(
        &mask_between(&mask_markdown_links(&out), "[[", "]]"),
        "<",
        ">",
    )
}

fn mask_markdown_links(line: &str) -> String {
    let mut masked = line.to_string();
    let mut offset = 0;
    while let Some(start) = masked[offset..].find('[') {
        let absolute = offset + start;
        let Some(close_text) = masked[absolute..].find("](") else {
            break;
        };
        let link_start = absolute + close_text;
        let Some(close_link) = masked[link_start..].find(')') else {
            break;
        };
        let range_end = link_start + close_link + 1;
        masked.replace_range(absolute..range_end, &" ".repeat(range_end - absolute));
        offset = range_end;
    }
    masked
}

fn mask_between(line: &str, open: &str, close: &str) -> String {
    let mut masked = line.to_string();
    let mut offset = 0;
    while let Some(start) = masked[offset..].find(open) {
        let absolute = offset + start;
        let Some(end) = masked[absolute + open.len()..].find(close) else {
            break;
        };
        let range_end = absolute + open.len() + end + close.len();
        masked.replace_range(absolute..range_end, &" ".repeat(range_end - absolute));
        offset = range_end;
    }
    masked
}

fn has_boundaries(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '-'
}

fn toggles_fence(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

fn normalize_term(term: &str) -> String {
    term.trim().to_lowercase()
}
