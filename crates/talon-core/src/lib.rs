//! Core types and contracts for Talon.
//!
//! The scaffold keeps parsing, configuration, constants, and response contracts
//! in the library so the CLI remains a thin process boundary.

pub mod cache;
pub mod config;
pub mod constants;
pub mod contracts;
pub mod embed;
pub mod error;
pub mod expansion;
pub mod indexer;
pub mod indexing;
pub mod inference;
pub mod links;
mod numeric;
pub mod query;
pub mod search;
pub mod store;
pub mod sync;
pub mod text;
pub mod vec_ext;

pub use config::{
    ChunkerConfig, ExpansionConfig, InferenceConfig, InferenceModels, Scope, ScopeGlob,
    ScopePriority, ScopeResolution, ScopesConfig, SearchConfig, TalonConfig,
};
pub use contracts::{
    ContainerPath, ErrorEnvelope, PositiveCount, ResponseMeta, TalonEnvelope, TalonInput,
    TalonResponseData, TalonResponseTrait, VaultPath,
};
pub use error::{ErrorCode, TalonError, TalonResult};
pub use expansion::{ExpansionClient, ExpansionError, LlmCache};
pub use indexer::{
    IndexNoteOutcome, IndexerConfig, IndexerStats, extract_title, hash_file_content,
    index_one_note, index_one_note_with_config, load_notes_for_linking, matches_ignore_patterns,
    matches_include_patterns, reconcile_deletions, run_full_scan, run_full_scan_with_chunker,
    scan_vault_markdown,
};
pub use indexing::{
    ChangeFeed, ChangeIndex, ChangeTrackingEntry, ChangeTrackingTombstoneEntry, ChunkUpsertRow,
    DB_VERSION_KEY, FileChangeState, FileState, IndexMetadata, IndexStats, LintCheck, LintFinding,
    LintInput, LintResponse, NoteUpsertResult, REBUILD_MIGRATIONS, SCHEMA_MIGRATIONS, ScopeReport,
    StatusInput, StatusResponse, StatusState, SyncInput, SyncResponse, SyncStatus,
    TALON_SQLITE_BUSY_TIMEOUT_MS, TOMBSTONE_RETENTION_MS, TRIGGER_MIGRATIONS, UpsertNoteParams,
    bump_db_version, now_ms, parse_since, perform_note_deletion, read_db_version, run_migrations,
    upsert_aliases, upsert_chunks, upsert_frontmatter_fields, upsert_links, upsert_note,
    upsert_tags,
};
pub use links::{
    LinkEdge, LinkGraphStats, NoteReference, ResolvedLink, build_link_edges, compute_backlinks,
    compute_link_stats, find_unresolved_links, resolve_wiki_link_target, resolve_wiki_links,
};
pub use query::{
    ChangeEntry, ChangesInput, ChangesResponse, EditedNote, FrontmatterFact, FuzzyAnchor,
    LinkedNote, MetaEntry, MetaInput, MetaResponse, NoteExcerpt, ReadInput, ReadResponse,
    ReadResult, RecallFormat, RecallInput, RecallResponse, RelatedInput, RelatedResponse,
    RelatedResult, RelationKind, TombstoneEntry, VaultRecall, find_related, query_changes,
    query_lint, query_meta, query_status, run_read, run_recall, run_search,
    run_search_with_expanded_queries,
};
pub use search::{
    AnchorKind, Direction, FrontmatterFilter, MatchAnchor, MatchKind, SearchHooks, SearchInput,
    SearchMode, SearchResponse, SearchResult, WhereClause, WhereOperator,
};
pub use store::open_database;
pub use sync::{
    SyncError, SyncLock, SyncLockError, acquire_sync_lock, run_sync, run_sync_with_chunker,
};
pub use text::chunker::{
    NoteChunk, build_embedding_text, build_heading_path, chunk_markdown, make_chunk_hash,
};
pub use text::frontmatter::{
    FrontmatterEntry, FrontmatterExtract, FrontmatterReverseIndex, FrontmatterValue,
    FrontmatterValueType, ReverseSourceIndex, WikiLink, extract_wikilinks, normalize_keyword,
    normalize_vault_path, parse_frontmatter,
};

pub use text::nfd::normalize as normalize_text_nfd;
pub use text::{
    LineSpan, ParsedWikiLink, TOKEN_CHAR_RATIO, estimate_tokens, is_fence_line, is_heading_line,
    parse_wikilink, split_lines, strip_heading_text, strip_outer_quotes,
};
