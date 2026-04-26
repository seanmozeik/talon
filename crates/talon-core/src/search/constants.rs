//! Magic numbers for the search pipeline.
//!
//! Ported verbatim from `services/talon/search/constants.ts`. Keeping the
//! same names (and tier multipliers) is what makes the parity tests
//! between the Rust and `TypeScript` implementations meaningful.

/// Default snippet length, in characters.
pub const DEFAULT_SNIPPET_LENGTH: u32 = 300;

/// Divisor used to convert snippet length (chars) to a token-count budget for
/// `SQLite` `FTS5`'s `snippet()` function.
pub const BM25_TOKENS_PER_CHAR_DIV: u32 = 4;

/// Minimum token budget passed to `FTS5`'s `snippet()` function.
pub const BM25_MIN_TOKENS: u32 = 10;

/// Trigram length used by fuzzy retrieval and overlap scoring.
pub const TRIGRAM_LEN: usize = 3;

/// Minimum alias length for trigram overlap scoring.
pub const FUZZY_ALIAS_MIN_LEN: usize = 3;

/// Strong-signal: top result score ≥ this implies high confidence.
pub const STRONG_SIGNAL_MIN_SCORE: f64 = 0.85;

/// Strong-signal: gap between top and second result must be ≥ this.
pub const STRONG_SIGNAL_MIN_GAP: f64 = 0.15;

/// LRU eviction threshold for the on-disk LLM cache table.
pub const LLM_CACHE_LIMIT: u32 = 1000;

/// Reciprocal Rank Fusion constant.
pub const RRF_K: f64 = 60.0;

/// Per-list RRF weighting.
#[derive(Debug, Clone, Copy)]
pub struct RrfWeights {
    /// BM25 lexical signal.
    pub bm25: f64,
    /// Exact alias match signal.
    pub exact_alias: f64,
    /// Fuzzy title/alias signal.
    pub fuzzy: f64,
    /// Semantic (vector) signal.
    pub semantic: f64,
}

/// Default RRF weights as specified in the design doc and TS reference.
pub const RRF_WEIGHTS: RrfWeights = RrfWeights {
    bm25: 2.0,
    exact_alias: 2.0,
    fuzzy: 0.5,
    semantic: 1.0,
};

/// Rerank blend weight for top-ranked candidates (rank < 10).
pub const RERANK_WEIGHT_TOP: f64 = 0.75;

/// Rerank blend weight for mid-ranked candidates (10 ≤ rank < 20).
pub const RERANK_WEIGHT_MID: f64 = 0.6;

/// Rerank blend weight for low-ranked candidates (rank ≥ 20).
pub const RERANK_WEIGHT_LOW: f64 = 0.4;

/// Rank threshold separating top from mid candidates.
pub const RERANK_TOP_RANK_THRESHOLD: usize = 10;

/// Rank threshold separating mid from low candidates.
pub const RERANK_MID_RANK_THRESHOLD: usize = 20;

/// FTS5 BM25 column weights. Order matches the schema:
/// `bm25(notes_fts_bm25, title, aliases, content)`.
#[derive(Debug, Clone, Copy)]
pub struct Bm25FtsWeights {
    /// Title column weight.
    pub title: f64,
    /// Aliases column weight.
    pub alias: f64,
    /// Content column weight.
    pub content: f64,
}

/// Default BM25 OHS weights: title=10, alias=5, content=1.
pub const BM25_FTS_SCORES: Bm25FtsWeights = Bm25FtsWeights {
    title: 10.0,
    alias: 5.0,
    content: 1.0,
};

/// Maximum cosine distance value used for distance→score normalization.
pub const COSINE_DISTANCE_MAX: f64 = 2.0;

/// Sentinel FTS query used when the input query reduces to nothing.
pub const LITERAL_EMPTY_FTS: &str = "\"\"";

/// Strong-match probe: BM25 result count in hybrid.
pub const HYBRID_PROBE_LEXICAL_LIMIT: u32 = 2;

/// Strong-match probe: title result count in hybrid.
pub const HYBRID_PROBE_TITLE_LIMIT: u32 = 1;

/// Maximum candidates sent to the cross-encoder reranker per call.
///
/// Mirrors `RERANK_CANDIDATE_LIMIT` from the root constants and the TS reference.
pub const RERANK_TOP_K: u32 = 40;

/// Default LRU size for the in-process hybrid result cache.
pub const GLOBAL_HYBRID_CACHE_SIZE: usize = 100;
