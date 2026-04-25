//! Product constants inherited from the TypeScript Talon implementation.

/// Default snippet length returned by search and related-note queries.
pub const DEFAULT_SNIPPET_LENGTH: u16 = 300;

/// Default number of search results.
pub const DEFAULT_LIMIT: u16 = 10;

/// Hybrid search candidate floor before reranking.
pub const CANDIDATE_POOL_FLOOR: u16 = 20;

/// Maximum candidates passed to reranking.
pub const RERANK_CANDIDATE_LIMIT: u16 = 40;

/// Reciprocal-rank-fusion constant.
pub const RRF_K: u16 = 60;

/// Strong-signal absolute score threshold.
pub const STRONG_SIGNAL_MIN_SCORE: f32 = 0.85;

/// Strong-signal gap threshold between top candidates.
pub const STRONG_SIGNAL_MIN_GAP: f32 = 0.15;

/// Hybrid search cache size.
pub const GLOBAL_HYBRID_CACHE_SIZE: usize = 100;

/// Query expansion and rerank cache size.
pub const LLM_CACHE_LIMIT: usize = 1_000;

/// Target token estimate per indexed chunk.
pub const CHUNK_TOKEN_TARGET: u16 = 900;

/// Chunk overlap ratio.
pub const CHUNK_OVERLAP_RATIO: f32 = 0.15;

/// Filesystem watcher debounce window in seconds.
pub const WATCHER_DEBOUNCE_SECONDS: u64 = 60;

/// Default related-note graph depth.
pub const RELATED_DEFAULT_DEPTH: u8 = 1;

/// Maximum related-note graph depth accepted by the tool.
pub const RELATED_MAX_DEPTH: u8 = 3;
