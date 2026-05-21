//! Search and inspect configuration tables.

use serde::{Deserialize, Serialize};

use super::defaults::{
    default_candidate_limit, default_limit, default_rerank_batch_size, default_rerank_cache_size,
    default_rerank_max_tokens, default_search_cache_size,
};

/// Lint settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectConfig {
    /// Glob-style patterns of file paths to skip when reporting inspect findings.
    /// Takes precedence over per-scope `inspect = true`. Files matching these
    /// globs are still indexed for link resolution.
    #[serde(default)]
    pub ignore: Vec<String>,
}

/// Search defaults and process-level cache/client tunables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchConfig {
    /// Candidate pool size used when no CLI flag is provided.
    #[serde(default = "default_candidate_limit")]
    pub candidate_limit: u16,
    /// Result limit used when no CLI flag is provided.
    #[serde(default = "default_limit")]
    pub limit: u16,
    /// Search-response LRU capacity.
    #[serde(default = "default_search_cache_size")]
    pub cache_size: usize,
    /// Rerank score LRU capacity.
    #[serde(default = "default_rerank_cache_size")]
    pub rerank_cache_size: usize,
    /// Reranker HTTP request batch size.
    #[serde(default = "default_rerank_batch_size")]
    pub rerank_batch_size: usize,
    /// Approximate reranker text budget.
    #[serde(default = "default_rerank_max_tokens")]
    pub rerank_max_tokens: u32,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            candidate_limit: default_candidate_limit(),
            limit: default_limit(),
            cache_size: default_search_cache_size(),
            rerank_cache_size: default_rerank_cache_size(),
            rerank_batch_size: default_rerank_batch_size(),
            rerank_max_tokens: default_rerank_max_tokens(),
        }
    }
}
