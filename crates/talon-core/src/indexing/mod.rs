//! Indexing tool input and output types.

pub mod change_tracking;
pub mod input;
pub mod migrations;
pub mod output;
pub mod upsert;

pub use change_tracking::{
    ChangeEntry as ChangeTrackingEntry, ChangeFeed, ChangeIndex, FileChangeState, FileState,
    IndexMetadata, TOMBSTONE_RETENTION_MS, TombstoneEntry as ChangeTrackingTombstoneEntry, now_ms,
    parse_since,
};
pub use input::{LintCheck, LintInput, StatusInput, SyncInput};
pub use migrations::{
    DB_VERSION_KEY, REBUILD_MIGRATIONS, SCHEMA_MIGRATIONS, TALON_SQLITE_BUSY_TIMEOUT_MS,
    TRIGGER_MIGRATIONS, run_migrations,
};
pub use output::{
    IndexStats, LintFinding, LintResponse, ScopeReport, StatusResponse, StatusState, SyncResponse,
    SyncStatus,
};
pub use upsert::{
    ChunkUpsertRow, NoteUpsertResult, UpsertNoteParams, perform_note_deletion, upsert_aliases,
    upsert_chunks, upsert_frontmatter_fields, upsert_links, upsert_note, upsert_tags,
};
