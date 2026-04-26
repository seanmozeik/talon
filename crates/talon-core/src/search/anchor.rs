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

use crate::tool::{AnchorKind, MatchAnchor};

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

/// Reconstructs a heading breadcrumb by scanning note content for the snippet
/// and walking backwards through heading lines (Strategy 2: content scan).
///
/// Returns `None` when the snippet isn't found in the note or no heading
/// precedes it.
fn scan_content_for_heading(conn: &Connection, vault_path: &str, snippet: &str) -> Option<String> {
    let clean = snippet.trim_start_matches("...").trim();
    if clean.len() < 8 {
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
    let before = &content[..pos];

    // Walk lines in reverse, collecting headings to build the chain.
    let mut headings: Vec<String> = Vec::new();
    for line in before.lines().rev() {
        if let Some(stripped) = line.strip_prefix("# ") {
            headings.push(stripped.trim().to_string());
            break; // top-level heading found — stop
        } else if let Some(stripped) = line.strip_prefix("## ").filter(|_| headings.is_empty()) {
            headings.push(stripped.trim().to_string());
        } else if let Some(stripped) = line.strip_prefix("### ").filter(|_| headings.is_empty()) {
            headings.push(stripped.trim().to_string());
        }
    }

    if headings.is_empty() {
        None
    } else {
        headings.reverse();
        Some(headings.join(" > "))
    }
}

/// Returns the `heading_path` for a note's best-matching chunk for display as a
/// breadcrumb (unconditional, independent of the `anchors` flag).
///
/// For semantic results, the `heading_path` is already in `raw.semantic_heading`.
/// For BM25/title results, falls back to Strategy 1 → Strategy 2 DB lookup.
#[must_use]
pub fn resolve_snippet_heading(conn: &Connection, raw: &RawSearchResult) -> Option<String> {
    if let Some(h) = raw.semantic_heading.as_deref().filter(|h| !h.is_empty()) {
        return Some(h.to_owned());
    }
    if raw.snippet.is_empty() {
        return None;
    }
    lookup_bm25_heading_strategy1(conn, &raw.path, &raw.snippet)
        .or_else(|| scan_content_for_heading(conn, &raw.path, &raw.snippet))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::search::types::SearchScores;
    use crate::store::open_database;
    use rusqlite::params;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_path() -> std::path::PathBuf {
        static C: AtomicU64 = AtomicU64::new(0);
        let n = C.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "talon-anchor-test-{}-{n}.sqlite",
            std::process::id()
        ))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = fs_err::remove_file(path);
        let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
    }

    fn raw(path: &str, snippet: &str, bm25: bool, sem_heading: Option<&str>) -> RawSearchResult {
        RawSearchResult {
            path: path.into(),
            title: "Test".into(),
            tags: vec![],
            aliases: vec![],
            snippet: snippet.into(),
            score: 0.9,
            scores: SearchScores {
                bm25: if bm25 { Some(0.9) } else { None },
                semantic: if sem_heading.is_some() {
                    Some(0.8)
                } else {
                    None
                },
                ..Default::default()
            },
            semantic_heading: sem_heading.map(ToOwned::to_owned),
            semantic_char_start: sem_heading.map(|_| 100),
            semantic_char_end: sem_heading.map(|_| 200),
        }
    }

    fn insert_note_with_content(conn: &Connection, vault_path: &str, content: &str) -> i64 {
        conn.execute(
            "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active) VALUES (?, ?, '[]', '[]', ?, 0, 0, 'h', 'd', 1)",
            params![vault_path, "Title", content],
        ).unwrap();
        conn.last_insert_rowid()
    }

    fn insert_chunk(conn: &Connection, note_id: i64, text: &str, heading: &str) {
        conn.execute(
            "INSERT INTO chunks (note_id, chunk_index, text, embedding_text, heading_path, char_start, char_end, line_start, line_end, chunk_hash, token_estimate, embedding_status) VALUES (?, 0, ?, '', ?, 0, 100, 0, 5, 'h', 10, 'pending')",
            params![note_id, text, heading],
        ).unwrap();
    }

    #[test]
    fn bm25_anchor_resolved_via_strategy1_chunk_lookup() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        let note_id = insert_note_with_content(
            &conn,
            "notes/test.md",
            "## Results\n\nThis is a matching snippet for tests.",
        );
        insert_chunk(
            &conn,
            note_id,
            "This is a matching snippet for tests.",
            "Results",
        );
        let r = raw(
            "notes/test.md",
            "This is a matching snippet for tests.",
            true,
            None,
        );
        let anchors = build_anchors(&conn, &r);
        assert!(!anchors.is_empty());
        let bm25 = anchors.iter().find(|a| a.kind == AnchorKind::Bm25).unwrap();
        assert_eq!(bm25.heading_path.as_deref(), Some("Results"));
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn semantic_anchor_built_from_chunk_metadata() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        let r = raw(
            "notes/sem.md",
            "semantic chunk text",
            false,
            Some("Methods > Setup"),
        );
        let anchors = build_anchors(&conn, &r);
        let sem = anchors
            .iter()
            .find(|a| a.kind == AnchorKind::Semantic)
            .unwrap();
        assert_eq!(sem.heading_path.as_deref(), Some("Methods > Setup"));
        assert_eq!(sem.char_start, Some(100));
        assert_eq!(sem.char_end, Some(200));
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn dedup_suppresses_semantic_when_match_text_equals_bm25() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        // Same snippet for both BM25 and semantic → only BM25 kept.
        let note_id =
            insert_note_with_content(&conn, "notes/both.md", "## Intro\n\nshared block text here");
        insert_chunk(&conn, note_id, "shared block text here", "Intro");
        let mut r = raw(
            "notes/both.md",
            "shared block text here",
            true,
            Some("Intro"),
        );
        // Make char offsets present
        r.semantic_char_start = Some(10);
        r.semantic_char_end = Some(30);
        let anchors = build_anchors(&conn, &r);
        // match_text for both would be identical → semantic is suppressed
        let bm25_count = anchors
            .iter()
            .filter(|a| a.kind == AnchorKind::Bm25)
            .count();
        let sem_count = anchors
            .iter()
            .filter(|a| a.kind == AnchorKind::Semantic)
            .count();
        assert_eq!(bm25_count, 1);
        assert_eq!(
            sem_count, 0,
            "dedup should remove duplicate semantic anchor"
        );
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn content_scan_fallback_finds_heading() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        insert_note_with_content(
            &conn,
            "notes/scan.md",
            "# Top Level\n\n## Sub Section\n\nThis fragment is scannable from context.",
        );
        // No chunk inserted — strategy 1 fails, strategy 2 should succeed.
        let heading = scan_content_for_heading(
            &conn,
            "notes/scan.md",
            "This fragment is scannable from context.",
        );
        assert!(heading.is_some(), "strategy 2 should find the heading");
        drop(conn);
        cleanup(&path);
    }
}
