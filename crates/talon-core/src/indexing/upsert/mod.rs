//! `SQL` upsert helpers for indexed notes and their associated rows.
//!
//! Ports `services/talon/indexer/{note-upsert,chunk-upsert,note-meta}.ts`.
//! All functions take a `&Connection` and assume the schema from
//! [`crate::indexing::migrations`] is in place.

use std::collections::BTreeMap;

use crate::text::frontmatter::FrontmatterValue;

mod chunks;
mod delete;
mod metadata;
mod note;

pub use chunks::upsert_chunks;
pub use delete::perform_note_deletion;
pub use metadata::{upsert_aliases, upsert_frontmatter_fields, upsert_links, upsert_tags};
pub use note::upsert_note;

/// Outcome of [`upsert_note`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteUpsertResult {
    /// Database `notes.id` of the upserted row.
    pub note_id: i64,
    /// `true` if a new row was inserted, `false` if an existing one was updated.
    pub is_new: bool,
}

/// Parameters for [`upsert_note`].
///
/// The `frontmatter` is serialized to `JSON` and stored verbatim in
/// `notes.frontmatter`; `aliases` and `tags` are also stored as `JSON`
/// arrays. The body content's `SHA-256` hash is computed and stored.
#[derive(Debug)]
pub struct UpsertNoteParams<'a> {
    /// Vault-relative path (e.g. `"zone/note.md"`).
    pub vault_path: &'a str,
    /// Display title.
    pub title: &'a str,
    /// Note body (post-frontmatter).
    pub content: &'a str,
    /// Parsed frontmatter map.
    pub frontmatter: &'a BTreeMap<String, FrontmatterValue>,
    /// Aliases, in declaration order.
    pub aliases: &'a [String],
    /// Tags (frontmatter + inline), deduplicated.
    pub tags: &'a [String],
    /// File modification time, milliseconds since epoch.
    pub mtime_ms: i64,
    /// File size in bytes.
    pub size_bytes: i64,
}

/// Per-chunk upsert payload. Mirrors [`crate::text::chunker::NoteChunk`] but
/// flattened into the column shape that the `chunks` table expects.
#[derive(Debug, Clone)]
pub struct ChunkUpsertRow {
    /// 0-indexed position within the note.
    pub index: u32,
    /// Raw chunk text.
    pub text: String,
    /// Embedding-friendly text (chunker's `build_embedding_text` output).
    pub embedding_text: String,
    /// Heading path (`"H1 > H2"`), if any.
    pub heading_path: Option<String>,
    /// Character span in the parent note.
    pub char_start: u32,
    /// Character end (exclusive) in the parent note.
    pub char_end: u32,
    /// 1-based line span start.
    pub line_start: u32,
    /// 1-based line span end.
    pub line_end: u32,
    /// `SHA-256` of `text`, used for dedup.
    pub chunk_hash: String,
    /// Token-count estimate (chars/4 with floor).
    pub token_estimate: u32,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
