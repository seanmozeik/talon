//! Match anchor construction for search results.
//!
//! Ports the `MatchAnchor` strategy from obsidian-hybrid-search (MIT licensed,
//! src/searcher.ts:460-515). Two strategies to find heading context for a BM25
//! snippet:
//!
//! - **Strategy 1** (chunk lookup): query `chunks` for the row whose text
//!   contains the snippet fragment; use that row's `heading_path`.
//! - **Strategy 2** (content scan): locate the snippet in `notes.content`, then
//!   walk preceding lines to reconstruct the heading chain.
//!
//! Semantic anchors are simpler: the chunk metadata (`heading_path`, `char_start`,
//! `char_end`) is already present in `RawSearchResult.semantic_heading/char_*`.

use rusqlite::Connection;
use rusqlite::OptionalExtension;

use crate::search::constants::{BM25_MIN_TOKENS, BM25_TOKENS_PER_CHAR_DIV, DEFAULT_SNIPPET_LENGTH};
use crate::search::text_fts::{FtsOperator, to_fts_query};
use crate::search::{AnchorKind, MatchAnchor};

use super::match_text::build_match_text;
use super::types::RawSearchResult;

/// Builds up to two anchors for a single result when `anchors=true`.
///
/// Returns at most `[BM25 anchor, Semantic anchor]`, deduplicated: if both
/// have the same `match_text` (same block matched by both strategies), only
/// the BM25 anchor is returned (positionally more precise, matching
/// obsidian-hybrid-search's searcher.ts:1227 dedup heuristic).
#[must_use]
pub fn build_anchors(conn: &Connection, raw: &RawSearchResult) -> Vec<MatchAnchor> {
    let mut anchors: Vec<MatchAnchor> = Vec::with_capacity(2);

    // ── BM25 anchor ──────────────────────────────────────────────────────────
    if !raw.snippet.is_empty() && raw.scores.bm25.is_some() {
        let snippet = raw.snippet.as_str();
        let heading = lookup_bm25_heading_strategy1(conn, &raw.path, snippet)
            .or_else(|| scan_content_for_heading(conn, &raw.path, snippet));
        let match_text = build_match_text(snippet);
        if !match_text.is_empty() {
            anchors.push(MatchAnchor {
                kind: AnchorKind::Bm25,
                heading_path: heading,
                match_text,
                char_start: None,
                char_end: None,
            });
        }
    }

    // ── Semantic anchor ───────────────────────────────────────────────────────
    let has_semantic = raw.semantic_heading.is_some()
        || raw.semantic_char_start.is_some()
        || raw.scores.semantic.is_some();
    if has_semantic {
        let match_text = build_match_text(raw.snippet.as_str());
        if !match_text.is_empty() {
            // Normalize heading: treat empty string as absent.
            let heading_opt = raw
                .semantic_heading
                .as_deref()
                .filter(|h| !h.is_empty())
                .map(ToOwned::to_owned);
            let sem_anchor = MatchAnchor {
                kind: AnchorKind::Semantic,
                heading_path: heading_opt,
                match_text,
                char_start: raw.semantic_char_start,
                char_end: raw.semantic_char_end,
            };
            // Dedup: if BM25 anchor exists with the same match_text, skip semantic.
            let duplicate = anchors
                .first()
                .is_some_and(|a| a.match_text == sem_anchor.match_text);
            if !duplicate {
                anchors.push(sem_anchor);
            }
        }
    }

    anchors
}

/// Expands a short BM25 snippet with a full-body FTS lookup when the current
/// excerpt is too short to be useful.
///
/// Algorithm ported verbatim from obsidian-hybrid-search (MIT) — searcher.ts:1195-1208.
#[must_use]
pub(crate) fn maybe_expand_bm25_snippet(
    conn: &Connection,
    note_id: i64,
    query: &str,
    snippet: &str,
) -> Option<String> {
    let snippet_chars = snippet.chars().count();
    if snippet_chars * 2 >= DEFAULT_SNIPPET_LENGTH as usize {
        return None;
    }

    if query.trim().is_empty() {
        return None;
    }

    let fts_query = to_fts_query(query, FtsOperator::Or);
    if fts_query.is_empty() {
        return None;
    }

    let num_tokens = BM25_MIN_TOKENS.max(DEFAULT_SNIPPET_LENGTH.div_ceil(BM25_TOKENS_PER_CHAR_DIV));
    let Ok(fallback) = conn.query_row(
        "SELECT snippet(notes_fts_bm25, 2, '', '', '...', ?) AS snippet
         FROM notes_fts_bm25
         JOIN notes n ON n.id = notes_fts_bm25.rowid
         WHERE n.id = ? AND n.active = 1
           AND notes_fts_bm25 MATCH ?
         LIMIT 1",
        rusqlite::params![num_tokens, note_id, fts_query],
        |row| row.get::<_, Option<String>>(0),
    ) else {
        return None;
    };

    let fallback = fallback?.trim().to_owned();
    if fallback.chars().count() > snippet_chars {
        Some(fallback)
    } else {
        None
    }
}

/// Looks up the `heading_path` of the chunk whose text best contains the BM25
/// snippet (Strategy 1: chunk lookup via DB).
///
/// Uses `LIKE` to find chunks whose text contains a ~40-char prefix of the
/// snippet (FTS snippets can have `...` prefix markers).
fn lookup_bm25_heading_strategy1(
    conn: &Connection,
    vault_path: &str,
    snippet: &str,
) -> Option<String> {
    // Strip leading `...` that FTS5 snippet() adds.
    let clean = snippet.trim_start_matches("...").trim();
    if clean.len() < 8 {
        return None;
    }
    // Use the first ~40 chars as a substring probe.
    let probe: String = clean.chars().take(40).collect();

    conn.query_row(
        "SELECT c.heading_path
         FROM chunks c
         JOIN notes n ON n.id = c.note_id
         WHERE n.vault_path = ? AND n.active = 1
           AND c.text LIKE '%' || ? || '%'
         ORDER BY c.chunk_index
         LIMIT 1",
        rusqlite::params![vault_path, probe],
        |row| row.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
    .filter(|s| !s.is_empty())
}

fn lookup_bm25_heading_or_scan_from_chunk(
    conn: &Connection,
    vault_path: &str,
    snippet: &str,
) -> Option<String> {
    let clean = snippet.trim_start_matches("...").trim();
    if clean.is_empty() {
        return None;
    }
    let probe: String = clean.chars().take(40).collect();

    let chunk = conn
        .query_row(
            "SELECT c.heading_path, c.char_start, n.content
             FROM chunks c
             JOIN notes n ON n.id = c.note_id
             WHERE n.vault_path = ? AND n.active = 1
               AND c.text LIKE '%' || ? || '%'
             ORDER BY c.chunk_index
             LIMIT 1",
            rusqlite::params![vault_path, probe],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .ok()??;

    if let Some(heading) = chunk.0.filter(|s| !s.is_empty()) {
        return Some(heading);
    }

    let char_start = usize::try_from(chunk.1?).ok()?;
    heading_breadcrumb_before(&chunk.2, char_start)
}

/// Reconstructs a heading breadcrumb by scanning note content for the snippet
/// and walking backwards through heading lines (Strategy 2: content scan).
///
/// Returns `None` when the snippet isn't found in the note or no heading
/// precedes it.
fn scan_content_for_heading(conn: &Connection, vault_path: &str, snippet: &str) -> Option<String> {
    let clean = snippet.trim_start_matches("...").trim();
    if clean.is_empty() {
        return None;
    }
    let probe: String = clean.chars().take(40).collect();

    let content: String = conn
        .query_row(
            "SELECT content FROM notes WHERE vault_path = ? AND active = 1",
            [vault_path],
            |row| row.get(0),
        )
        .ok()?;

    let pos = content.find(probe.as_str())?;
    heading_breadcrumb_before(&content, pos)
}

fn heading_breadcrumb_before(content: &str, byte_pos: usize) -> Option<String> {
    let before = content.get(..byte_pos)?;
    let mut headings = [const { None::<String> }; 4];
    let mut max_level = 4;

    for line in before.lines().rev() {
        let Some((level, text)) = parse_heading_line(line) else {
            continue;
        };
        if level > max_level {
            continue;
        }
        let index = level.saturating_sub(1);
        if headings[index].is_none() {
            headings[index] = Some(text.to_owned());
            max_level = index;
        }
        if index == 0 {
            break;
        }
    }

    let breadcrumb: Vec<&str> = headings
        .iter()
        .filter_map(std::option::Option::as_deref)
        .collect();
    (!breadcrumb.is_empty()).then(|| breadcrumb.join(" > "))
}

fn parse_heading_line(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let level = trimmed.bytes().take_while(|b| *b == b'#').count();
    if !(1..).contains(&level)
        || !trimmed
            .as_bytes()
            .get(level)
            .is_some_and(u8::is_ascii_whitespace)
    {
        return None;
    }
    let folded_level = level.min(4);
    let text = trimmed[level..].trim();
    (!text.is_empty()).then_some((folded_level, text))
}

/// Returns the `heading_path` for a note's best-matching chunk for display as a
/// breadcrumb (unconditional, independent of the `anchors` flag).
///
/// For semantic results, the `heading_path` is already in `raw.semantic_heading`.
/// For BM25/title results, uses the best-matching chunk heading or scans note
/// content from that chunk's `char_start` when `heading_path` is missing.
#[must_use]
pub fn resolve_snippet_heading(
    conn: &Connection,
    raw: &RawSearchResult,
    snippet: &str,
) -> Option<String> {
    if let Some(h) = raw.semantic_heading.as_deref().filter(|h| !h.is_empty()) {
        return Some(h.to_owned());
    }
    if snippet.is_empty() {
        return None;
    }
    lookup_bm25_heading_or_scan_from_chunk(conn, &raw.path, snippet)
}

#[cfg(test)]
mod tests;
