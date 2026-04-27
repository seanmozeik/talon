//! Pool sizing helpers for retriever over-fetch.
//!
//! Each retriever needs to fetch more candidates than the final limit
//! to account for post-filter deduplication and scoring spread. These
//! helpers compute per-retriever pool sizes based on the desired limit.

use std::cmp::max;

/// Compute the pool size for BM25 lexical retrieval.
///
/// Returns `max(limit * 2, max(candidate_floor, 50))`.
#[must_use]
pub fn bm25_pool(limit: u32, candidate_floor: u32) -> u32 {
    max(limit.saturating_mul(2), max(candidate_floor, 50))
}

/// Compute the pool size for vector semantic retrieval.
///
/// Vector deduplication happens at the note level, so a wider pool is needed.
/// Returns `max(limit * 5, max(candidate_floor * 2, 100))`.
#[must_use]
pub fn vector_pool(limit: u32, candidate_floor: u32) -> u32 {
    max(
        limit.saturating_mul(5),
        max(candidate_floor.saturating_mul(2), 100),
    )
}

/// Compute the pool size for fuzzy title/alias retrieval.
///
/// Returns `max(limit * 2, max(candidate_floor, 50))`.
#[must_use]
pub fn fuzzy_pool(limit: u32, candidate_floor: u32) -> u32 {
    max(limit.saturating_mul(2), max(candidate_floor, 50))
}

/// Compute the final pre-rerank pool size for RRF fusion.
///
/// This is the size of the merged candidate list after RRF, used
/// before passing to the cross-encoder reranker.
/// Returns `max(limit, candidate_floor)`.
#[must_use]
pub fn rrf_pool(limit: u32, candidate_floor: u32) -> u32 {
    max(limit, candidate_floor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bm25_pool() {
        // limit=1, candidate_floor=40
        assert_eq!(bm25_pool(1, 40), 50);
        // limit=10, candidate_floor=40
        assert_eq!(bm25_pool(10, 40), 50);
        // limit=1000, candidate_floor=40
        assert_eq!(bm25_pool(1000, 40), 2000);

        // limit=1, candidate_floor=100
        assert_eq!(bm25_pool(1, 100), 100);
        // limit=10, candidate_floor=100
        assert_eq!(bm25_pool(10, 100), 100);
        // limit=1000, candidate_floor=100
        assert_eq!(bm25_pool(1000, 100), 2000);
    }

    #[test]
    fn test_vector_pool() {
        // limit=1, candidate_floor=40
        assert_eq!(vector_pool(1, 40), 100);
        // limit=10, candidate_floor=40
        assert_eq!(vector_pool(10, 40), 100);
        // limit=1000, candidate_floor=40
        assert_eq!(vector_pool(1000, 40), 5000);

        // limit=1, candidate_floor=100
        assert_eq!(vector_pool(1, 100), 200);
        // limit=10, candidate_floor=100
        assert_eq!(vector_pool(10, 100), 200);
        // limit=1000, candidate_floor=100
        assert_eq!(vector_pool(1000, 100), 5000);
    }

    #[test]
    fn test_fuzzy_pool() {
        // limit=1, candidate_floor=40
        assert_eq!(fuzzy_pool(1, 40), 50);
        // limit=10, candidate_floor=40
        assert_eq!(fuzzy_pool(10, 40), 50);
        // limit=1000, candidate_floor=40
        assert_eq!(fuzzy_pool(1000, 40), 2000);

        // limit=1, candidate_floor=100
        assert_eq!(fuzzy_pool(1, 100), 100);
        // limit=10, candidate_floor=100
        assert_eq!(fuzzy_pool(10, 100), 100);
        // limit=1000, candidate_floor=100
        assert_eq!(fuzzy_pool(1000, 100), 2000);
    }

    #[test]
    fn test_rrf_pool() {
        // limit=1, candidate_floor=40
        assert_eq!(rrf_pool(1, 40), 40);
        // limit=10, candidate_floor=40
        assert_eq!(rrf_pool(10, 40), 40);
        // limit=1000, candidate_floor=40
        assert_eq!(rrf_pool(1000, 40), 1000);

        // limit=1, candidate_floor=100
        assert_eq!(rrf_pool(1, 100), 100);
        // limit=10, candidate_floor=100
        assert_eq!(rrf_pool(10, 100), 100);
        // limit=1000, candidate_floor=100
        assert_eq!(rrf_pool(1000, 100), 1000);
    }
}
