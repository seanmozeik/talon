//! Search algorithms ported from `services/talon/search/*.ts`.
//!
//! This module is split into:
//!
//! - [`constants`] — magic numbers (RRF k, weight tables, thresholds).
//! - [`types`] — internal result/score types used by the pipeline.
//! - [`text_fts`] — pure helpers for FTS query construction and BM25 score
//!   normalization.
//! - [`bm25`] — BM25 retrieval against `notes_fts_bm25` (DB-backed).
//! - [`fuzzy_title`] — trigram fuzzy title/alias retrieval against
//!   `notes_fts_fuzzy` (DB-backed).
//! - [`vector`] — `sqlite-vec` cosine search against `vec_chunks` (DB-backed).
//! - [`rrf`] — Reciprocal Rank Fusion across signal lists.
//! - [`fuse`] — strong-signal detection, hybrid result fusion, rerank blending.
//! - [`cache`] — in-process LRU cache used by the hybrid pipeline.

pub mod anchor;
pub mod bm25;
pub mod cache;
pub mod constants;
pub mod fuse;
pub mod fuzzy_title;
pub mod hybrid_pipeline;
pub mod hybrid_single;
pub mod input;
pub mod match_text;
pub mod output;
pub mod pool;
pub mod rerank_pipeline;
pub mod rrf;
pub mod text_fts;
pub mod types;
pub mod vector;

pub use cache::SearchCache;
pub use constants::{
    BM25_FTS_SCORES, BM25_MIN_TOKENS, BM25_TOKENS_PER_CHAR_DIV, CANDIDATE_FLOOR,
    COSINE_DISTANCE_MAX, DEFAULT_SNIPPET_LENGTH, FUZZY_ALIAS_MIN_LEN, GLOBAL_HYBRID_CACHE_SIZE,
    HYBRID_PROBE_LEXICAL_LIMIT, HYBRID_PROBE_TITLE_LIMIT, LITERAL_EMPTY_FTS, LLM_CACHE_LIMIT,
    RERANK_MID_RANK_THRESHOLD, RERANK_TOP_K, RERANK_TOP_RANK_THRESHOLD, RERANK_WEIGHT_LOW,
    RERANK_WEIGHT_MID, RERANK_WEIGHT_TOP, RRF_K, RRF_WEIGHTS, RrfWeights, STRONG_SIGNAL_MIN_GAP,
    STRONG_SIGNAL_MIN_SCORE, TRIGRAM_LEN,
};
pub use fuse::{
    blend_rerank_candidates, clamp01, estimate_strong_signal, fuse_hybrid_result_lists, sigmoid,
};
pub use fuzzy_title::TitleSearchParts;
pub use hybrid_pipeline::{HybridPipelineOptions, run_hybrid_pipeline};
pub use hybrid_single::{HybridSingleResult, run_hybrid_single};
pub use input::{
    Direction, FrontmatterFilter, FrontmatterValue, FrontmatterValueType, SearchInput, SearchMode,
    WhereClause, WhereOperator,
};
pub use output::{AnchorKind, MatchAnchor, MatchKind, SearchResponse, SearchResult};
pub use rerank_pipeline::rerank_candidates;
pub use rrf::{RrfList, RrfScoreAccumulator, normalize_and_merge_rrf_results};
pub use text_fts::{
    FtsOperator, build_bm25_score, build_trigram_or_query, calculate_trigram_overlap, get_trigrams,
    sanitize_fts_query, to_fts_query,
};
pub use types::{HybridScoreData, RawSearchResult, SearchScores};
pub use vector::distance_to_score;
