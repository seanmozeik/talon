//! Core types and contracts for Talon.
//!
//! The scaffold keeps parsing, configuration, constants, and response contracts
//! in the library so the CLI remains a thin process boundary.

pub mod change_tracking;
pub mod chunker;
pub mod config;
pub mod constants;
pub mod embed;
pub mod error;
pub mod expansion;
pub mod frontmatter;
pub mod indexer;
pub mod inference;
pub mod links;
pub mod migrations;
pub mod query;
pub mod search;
pub mod store;
pub mod sync;
pub mod text;
pub mod tool;
pub mod vec_ext;

pub use change_tracking::{
    ChangeEntry, ChangeFeed, ChangeIndex, FileChangeState, FileState, IndexMetadata,
    TOMBSTONE_RETENTION_MS, TombstoneEntry, now_ms, parse_since,
};
pub use chunker::{
    BlockKind, ChunkContext, MarkdownBlock, NoteChunk, build_embedding_text, build_heading_path,
    chunk_blocks, chunk_markdown, collect_blocks, make_chunk_hash,
};
pub use config::{
    ExpansionConfig, InferenceConfig, InferenceModels, Scope, ScopeGlob, ScopePriority,
    ScopeResolution, ScopesConfig, TalonConfig,
};
pub use error::{ErrorCode, TalonError, TalonResult};
pub use expansion::{ExpansionClient, ExpansionError, LlmCache};
pub use frontmatter::{
    FrontmatterEntry, FrontmatterExtract, FrontmatterReverseIndex, FrontmatterValue,
    FrontmatterValueType, ReverseSourceIndex, WikiLink, normalize_keyword, normalize_vault_path,
    parse_frontmatter,
};
pub use indexer::{
    ChunkUpsertRow, IndexNoteOutcome, IndexerConfig, IndexerStats, NoteUpsertResult,
    UpsertNoteParams, extract_title, hash_file_content, index_one_note, load_notes_for_linking,
    matches_ignore_patterns, matches_include_patterns, perform_note_deletion, reconcile_deletions,
    run_full_scan, scan_vault_markdown, upsert_aliases, upsert_chunks, upsert_frontmatter_fields,
    upsert_links, upsert_note, upsert_tags,
};
pub use links::{
    LinkEdge, LinkGraphStats, NoteReference, ResolvedLink, build_link_edges, compute_backlinks,
    compute_link_stats, find_unresolved_links, resolve_wiki_link_target, resolve_wiki_links,
};
pub use migrations::{
    DB_VERSION_KEY, REBUILD_MIGRATIONS, SCHEMA_MIGRATIONS, TALON_SQLITE_BUSY_TIMEOUT_MS,
    TRIGGER_MIGRATIONS, run_migrations,
};
pub use query::{
    find_related, query_changes, query_lint, query_meta, query_status, run_read, run_search,
};
pub use store::open_database;
pub use sync::{SyncError, SyncLock, SyncLockError, acquire_sync_lock, run_sync};

pub use text::{
    LineSpan, ParsedWikiLink, TOKEN_CHAR_RATIO, estimate_tokens, is_fence_line, is_heading_line,
    parse_wikilink, split_lines, strip_heading_text, strip_outer_quotes,
};
pub use tool::{
    ChangesInput, ChangesResponse, ContainerPath, Direction, ErrorEnvelope, FrontmatterFilter,
    IndexStats, LintCheck, LintFinding, LintInput, LintResponse, MatchKind, MetaEntry, MetaInput,
    MetaResponse, PositiveCount, ReadInput, ReadResponse, ReadResult, RelatedInput,
    RelatedResponse, RelatedResult, RelationKind, ResponseMeta, ScopeReport, SearchInput,
    SearchMode, SearchResponse, SearchResult, StatusInput, StatusResponse, StatusState, SyncInput,
    SyncResponse, SyncStatus, TalonEnvelope, TalonInput, TalonResponseData, TalonResponseTrait,
    VaultPath, WhereClause, WhereOperator,
};
