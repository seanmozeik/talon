//! Per-note indexing pipeline: parse → chunk → upsert.
//!
//! Ports `services/talon/indexer/wiring.ts`. The single entry point
//! [`index_one_note`] takes a vault-relative path plus the file's content
//! and stat, runs the full parse/chunk/upsert pipeline, and writes an
//! `'index'` row to `event_log`. All work happens inside a single
//! transaction so a partial failure does not leave the index inconsistent.

use rusqlite::{Connection, params};

use crate::TalonError;
use crate::config::ChunkerConfig;
use crate::indexing::{
    ChunkUpsertRow, NoteUpsertResult, UpsertNoteParams, upsert_aliases, upsert_chunks,
    upsert_frontmatter_fields, upsert_links, upsert_note, upsert_tags,
};
use crate::links::{NoteReference, find_unresolved_links, resolve_wiki_links};
use crate::text::chunker::chunk_markdown;
use crate::text::frontmatter::{extract_wikilinks, parse_frontmatter};
use crate::text::normalize_vault_path;

use super::prelude::{extract_title, merge_current_path_for_linking};

/// Outcome of [`index_one_note`].
#[derive(Debug, Clone)]
pub struct IndexNoteOutcome {
    /// Result of the note row upsert.
    pub note: NoteUpsertResult,
    /// Updated link-resolution cache reflecting this note's title/aliases.
    /// Callers iterating multiple notes should pass this as the `existing`
    /// argument on the next call to keep cross-note links resolving correctly
    /// without re-querying the DB.
    pub updated_links_cache: Vec<NoteReference>,
}

/// Runs the full per-note indexing pipeline against `conn`.
///
/// Steps:
///
/// 1. Parse frontmatter, body, aliases, tags, and wikilinks from `content`.
/// 2. Extract the display title (frontmatter `title` field or filename stem).
/// 3. Chunk the markdown body using [`chunk_markdown`].
/// 4. Resolve wikilinks against `existing_for_linking` plus the current note.
/// 5. Upsert the note row, its chunks, links, aliases, tags, and frontmatter
///    fields, all inside a single transaction.
/// 6. Append an `'index'` event-log row.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] for any database failure and
/// [`TalonError::Internal`] for serialization failures encountered by the
/// nested upsert helpers.
pub fn index_one_note(
    conn: &mut Connection,
    vault_path: &str,
    content: &str,
    mtime_ms: i64,
    size_bytes: i64,
    existing_for_linking: &[NoteReference],
) -> Result<IndexNoteOutcome, TalonError> {
    index_one_note_with_config(
        conn,
        vault_path,
        content,
        mtime_ms,
        size_bytes,
        existing_for_linking,
        &ChunkerConfig::default(),
    )
}

/// Like [`index_one_note`] but accepts an explicit [`ChunkerConfig`].
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] for any database failure and
/// [`TalonError::Internal`] for serialization failures encountered by the
/// nested upsert helpers.
pub fn index_one_note_with_config(
    conn: &mut Connection,
    vault_path: &str,
    content: &str,
    mtime_ms: i64,
    size_bytes: i64,
    existing_for_linking: &[NoteReference],
    chunker_config: &ChunkerConfig,
) -> Result<IndexNoteOutcome, TalonError> {
    // Normalize to NFD so NFC and NFD forms of the same Unicode filename map
    // to the same DB row (macOS HFS+ stores paths in NFD; Linux typically NFC).
    let vault_path_normalized = normalize_vault_path(vault_path);
    let vault_path = vault_path_normalized.as_str();
    let parsed = parse_frontmatter(content);
    let title = extract_title(vault_path, &parsed.frontmatter);

    // Chunker receives body only — frontmatter must not appear in chunk text.
    let chunks = chunk_markdown(&parsed.body, &title, vault_path, chunker_config);

    let updated_cache =
        merge_current_path_for_linking(existing_for_linking, vault_path, &title, &parsed.aliases);

    // Wikilink resolution sees both frontmatter and body so that links in
    // frontmatter fields (e.g. `related: "[[Other Note]]"`) are indexed.
    let full_for_links = if parsed.frontmatter_raw.is_empty() {
        parsed.links.clone()
    } else {
        extract_wikilinks(&format!("{}\n{}", parsed.frontmatter_raw, parsed.body))
    };
    let mut resolved = resolve_wiki_links(vault_path, &full_for_links, &updated_cache);
    // Record unresolved links too: the broken-link lint and `links.to_path`
    // schema both require a non-empty target. Using `raw_target` as the
    // placeholder keeps the row addressable while clearly signaling that no
    // matching note was found.
    for mut unresolved in find_unresolved_links(vault_path, &parsed.links, &updated_cache) {
        unresolved.to_path.clone_from(&unresolved.raw_target);
        resolved.push(unresolved);
    }

    let chunk_rows: Vec<ChunkUpsertRow> = chunks
        .iter()
        .enumerate()
        .map(|(idx, c)| ChunkUpsertRow {
            index: u32::try_from(idx).unwrap_or(u32::MAX),
            text: c.text.clone(),
            embedding_text: c.embedding_text.clone(),
            heading_path: if c.heading_path.is_empty() {
                None
            } else {
                Some(c.heading_path.clone())
            },
            char_start: u32::try_from(c.char_start).unwrap_or(u32::MAX),
            char_end: u32::try_from(c.char_end).unwrap_or(u32::MAX),
            line_start: c.line_start,
            line_end: c.line_end,
            chunk_hash: c.chunk_hash.clone(),
            token_estimate: u32::try_from(c.token_estimate).unwrap_or(u32::MAX),
        })
        .collect();

    let tx = conn.transaction().map_err(|source| TalonError::Sqlite {
        context: "begin index transaction",
        source,
    })?;

    let note = upsert_note(
        &tx,
        &UpsertNoteParams {
            vault_path,
            title: &title,
            content: &parsed.body,
            frontmatter: &parsed.frontmatter,
            aliases: &parsed.aliases,
            tags: &parsed.tags,
            mtime_ms,
            size_bytes,
        },
    )?;
    upsert_chunks(&tx, note.note_id, &chunk_rows)?;
    upsert_links(&tx, vault_path, &resolved)?;
    upsert_aliases(&tx, note.note_id, &parsed.aliases)?;
    upsert_tags(&tx, note.note_id, &parsed.tags)?;
    upsert_frontmatter_fields(&tx, note.note_id, &parsed.frontmatter)?;

    let timestamp = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| String::from("?"));
    tx.execute(
        "INSERT INTO event_log (action, path, timestamp) VALUES (?, ?, ?)",
        params!["index", vault_path, timestamp],
    )
    .map_err(|source| TalonError::Sqlite {
        context: "log index event",
        source,
    })?;

    tx.commit().map_err(|source| TalonError::Sqlite {
        context: "commit index transaction",
        source,
    })?;

    Ok(IndexNoteOutcome {
        note,
        updated_links_cache: updated_cache,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::store::open_database;
    use std::env::temp_dir;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_db(label: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        temp_dir().join(format!("talon-wiring-test-{label}-{pid}-{n}.sqlite"))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = fs_err::remove_file(path);
        let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
    }

    #[test]
    fn index_one_note_round_trip_writes_all_tables() {
        let path = unique_db("round");
        let mut conn = open_database(&path).unwrap();

        let body = "---
title: Atomic Notes
aliases: [Atomic, Zettel]
tags: [zettelkasten]
---

# Atomic Notes

Atomic notes link to [[Other Note]].

#inline-tag
";
        let outcome = index_one_note(&mut conn, "zone/atomic.md", body, 1000, 100, &[]).unwrap();
        assert!(outcome.note.is_new);

        // Note row.
        let title: String = conn
            .query_row(
                "SELECT title FROM notes WHERE vault_path = 'zone/atomic.md'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(title, "Atomic Notes");

        // Aliases.
        let alias_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM note_aliases WHERE note_id = ?",
                [outcome.note.note_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(alias_count, 2);

        // Tags include both frontmatter and inline.
        let tag_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM note_tags WHERE note_id = ?",
                [outcome.note.note_id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(tag_count >= 2);

        // Links row exists for the wikilink.
        let link_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM links WHERE from_path = 'zone/atomic.md'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(link_count, 1);

        // FTS index sees the note.
        let fts_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM notes_fts_bm25 WHERE notes_fts_bm25 MATCH 'atomic'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts_count, 1);

        // Event log entry.
        let log_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM event_log WHERE action = 'index' AND path = 'zone/atomic.md'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(log_count, 1);

        cleanup(&path);
    }

    #[test]
    fn index_one_note_second_pass_is_update_not_insert() {
        let path = unique_db("update");
        let mut conn = open_database(&path).unwrap();
        let first = index_one_note(&mut conn, "a.md", "hello v1", 100, 8, &[]).unwrap();
        let second = index_one_note(&mut conn, "a.md", "hello v2", 200, 8, &[]).unwrap();
        assert!(first.note.is_new);
        assert!(!second.note.is_new);
        assert_eq!(first.note.note_id, second.note.note_id);
        // Updated content visible.
        let content: String = conn
            .query_row(
                "SELECT content FROM notes WHERE id = ?",
                [second.note.note_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(content, "hello v2");
        cleanup(&path);
    }

    #[test]
    fn updated_links_cache_carries_current_note() {
        let path = unique_db("cache");
        let mut conn = open_database(&path).unwrap();
        let outcome = index_one_note(&mut conn, "a.md", "# Title A", 0, 10, &[]).unwrap();
        let in_cache = outcome
            .updated_links_cache
            .iter()
            .any(|n| n.vault_path == "a.md");
        assert!(in_cache);
        cleanup(&path);
    }
}
