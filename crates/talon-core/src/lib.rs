//! Core types and contracts for Talon.
//!
//! The scaffold keeps parsing, configuration, constants, and response contracts
//! in the library so the CLI remains a thin process boundary.

pub mod ask;
pub mod cache;
pub mod config;
pub mod constants;
pub mod contracts;
pub mod embed;
pub mod error;
pub mod expansion;
pub mod glob_matcher;
pub mod graph;
pub mod indexer;
pub mod indexing;
pub mod inference;
pub mod links;
pub mod llm;
mod numeric;
pub mod query;
pub mod search;
pub mod store;
pub mod sync;
pub mod text;
pub mod vec_ext;

pub use ask::{AskClient, AskError, AskPlan, AskSynthesis};
pub use config::{
    AskConfig, ChunkerConfig, ExpansionConfig, InferenceConfig, InferenceModels, InspectConfig,
    McpConfig, McpHooksConfig, RerankConfig, RerankRequestShape, RerankScoreScale, Scope,
    ScopeFilter, ScopeGlob, ScopePriority, ScopeResolution, ScopesConfig, SearchConfig,
    TalonConfig,
};
pub use contracts::{
    ContainerPath, ErrorEnvelope, PositiveCount, ResponseMeta, TalonEnvelope, TalonInput,
    TalonResponseData, TalonResponseTrait, VaultPath,
};
pub use error::{ErrorCode, TalonError, TalonResult};
pub use expansion::{ExpansionClient, ExpansionError, LlmCache};
pub use glob_matcher::glob_match_case_insensitive;
pub use graph::{GraphBuildStats, GraphSuggestionClient};
pub use indexer::{
    IndexNoteOutcome, IndexerConfig, IndexerStats, NoteIndexConfig, build_ignore_globset,
    build_include_globset, extract_title, file_matches_ignore, file_matches_include,
    hash_file_content, index_one_note, index_one_note_with_config, load_notes_for_linking,
    matches_ignore_patterns, matches_include_patterns, reconcile_deletions,
    reconcile_ignored_notes, run_full_scan, run_full_scan_with_chunker, scan_vault_markdown,
};
pub use indexing::{
    ChangeFeed, ChangeIndex, ChangeTrackingEntry, ChangeTrackingTombstoneEntry, ChunkUpsertRow,
    DB_VERSION_KEY, FileChangeState, FileState, IndexMetadata, IndexStats, InspectCheck,
    InspectFinding, InspectInput, InspectResponse, NoteUpsertResult, REBUILD_MIGRATIONS,
    SCHEMA_MIGRATIONS, ScopeReport, StatusInput, StatusResponse, StatusState, SyncInput,
    SyncResponse, SyncStatus, TALON_SQLITE_BUSY_TIMEOUT_MS, TOMBSTONE_RETENTION_MS,
    TRIGGER_MIGRATIONS, UpsertNoteParams, bump_db_version, now_ms, parse_since,
    perform_note_deletion, read_db_version, run_migrations, upsert_aliases, upsert_chunks,
    upsert_frontmatter_fields, upsert_links, upsert_note, upsert_tags,
};
pub use links::{
    LinkEdge, LinkGraphStats, NoteReference, ResolvedLink, build_link_edges, compute_backlinks,
    compute_link_stats, find_unresolved_links, resolve_wiki_link_target, resolve_wiki_links,
};
pub use llm::{ChatClient, ChatError, ChatMessage, ReasoningEffort};
pub use query::{
    AskDiagnostics, AskLlmStageDiagnostics, AskResponse, AskSearchDiagnostics, AskSource,
    ChangeEntry, ChangesInput, ChangesResponse, LinkedNote, MetaEntry, MetaInput, MetaResponse,
    NoteExcerpt, ReadInput, ReadResponse, ReadResult, ReadSection, RecallFormat, RecallInput,
    RecallResponse, RelatedInput, RelatedResponse, RelatedResult, RelationKind, TombstoneEntry,
    VaultRecall, find_related, query_changes, query_inspect, query_meta, query_status, run_read,
    run_recall, run_search, run_search_with_expanded_queries,
};
pub use rusqlite::Connection;
pub use search::{
    AnchorKind, Direction, FrontmatterFilter, GraphSearchDiagnostics, MatchAnchor, MatchKind,
    SearchDiagnostics, SearchHooks, SearchInput, SearchMode, SearchResponse, SearchResult,
    WhereClause, WhereOperator,
};
pub use store::{open_database, open_database_read_only};
pub use sync::{
    SyncError, SyncLock, SyncLockError, acquire_sync_lock, is_sync_lock_held_by_live_process,
    refresh_index, refresh_index_locked, relink_unresolved, remove_index_files, run_sync,
    run_sync_with_chunker, run_sync_with_chunker_locked,
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
