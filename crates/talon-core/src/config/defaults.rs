use crate::constants::DEFAULT_LIMIT;
use crate::search::constants::{
    CANDIDATE_FLOOR_U16, GLOBAL_HYBRID_CACHE_SIZE, RERANK_BATCH_SIZE, RERANK_CACHE_SIZE,
    RERANK_MAX_TOKENS,
};

pub(super) const fn default_candidate_limit() -> u16 {
    CANDIDATE_FLOOR_U16
}

pub(super) const fn default_limit() -> u16 {
    DEFAULT_LIMIT
}

pub(super) const fn default_search_cache_size() -> usize {
    GLOBAL_HYBRID_CACHE_SIZE
}

pub(super) const fn default_rerank_cache_size() -> usize {
    RERANK_CACHE_SIZE
}

pub(super) const fn default_rerank_batch_size() -> usize {
    RERANK_BATCH_SIZE
}

pub(super) const fn default_rerank_max_tokens() -> u32 {
    RERANK_MAX_TOKENS
}
