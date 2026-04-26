//! Indexing pipeline: parse → chunk → upsert.
//!
//! Ports `services/talon/indexer/*.ts` from the `TypeScript` reference. Layout:
//!
//! - [`prelude`] — file utilities (hashing, glob filters, vault scan, title
//!   extraction, link-graph cache loading).
//! - [`upsert`] — `SQL` upsert helpers for notes, chunks, links, aliases,
//!   tags, frontmatter fields, and soft-delete.
//! - [`wiring`] — per-note pipeline that ties the chunker, frontmatter, and
//!   link resolver to the upsert helpers.
//! - [`scan`] — full vault scan that walks the filesystem, applies
//!   include/ignore filters, skips unchanged files via mtime+size, and
//!   reconciles deletions.

pub mod prelude;
pub mod scan;
pub mod upsert;
pub mod wiring;

pub use prelude::{
    DEFAULT_IGNORE_PATHS, extract_title, hash_file_content, load_notes_for_linking,
    matches_ignore_patterns, matches_include_patterns, merge_current_path_for_linking,
    scan_vault_markdown,
};
pub use scan::{IndexerConfig, IndexerStats, reconcile_deletions, run_full_scan};
pub use upsert::{
    ChunkUpsertRow, NoteUpsertResult, UpsertNoteParams, perform_note_deletion, upsert_aliases,
    upsert_chunks, upsert_frontmatter_fields, upsert_links, upsert_note, upsert_tags,
};
pub use wiring::{IndexNoteOutcome, index_one_note};
