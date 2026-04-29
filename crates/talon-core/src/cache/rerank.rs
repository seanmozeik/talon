//! Per-snippet rerank cache invalidated by the `SQLite` index content version.

use std::num::NonZeroUsize;
use std::sync::{LazyLock, Mutex, OnceLock};

use lru::LruCache;
use xxhash_rust::xxh3::xxh3_64;

use crate::search::constants::RERANK_CACHE_SIZE;

/// Opaque cache key for a `(chunk_text, query_text)` rerank pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RerankCacheKey {
    chunk_text_hash: u64,
    query_text_hash: u64,
}

/// LRU cache for normalized rerank scores.
#[derive(Debug)]
pub struct RerankCache {
    entries: Option<LruCache<RerankCacheKey, f64>>,
    db_version: u64,
}

impl RerankCache {
    /// Creates a rerank cache with the requested capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let entries = NonZeroUsize::new(capacity).map(LruCache::new);
        Self {
            entries,
            db_version: 0,
        }
    }

    /// Returns a normalized score when `db_version` is still current.
    pub fn get(&mut self, key: RerankCacheKey, db_version: u64) -> Option<f64> {
        self.invalidate_if_stale(db_version);
        self.entries.as_mut()?.get(&key).copied()
    }

    /// Inserts a normalized score computed against `db_version`.
    pub fn put(&mut self, key: RerankCacheKey, score: f64, db_version: u64) {
        self.invalidate_if_stale(db_version);
        let Some(entries) = self.entries.as_mut() else {
            return;
        };
        entries.put(key, score);
    }

    fn invalidate_if_stale(&mut self, db_version: u64) {
        if self.db_version == db_version {
            return;
        }
        if let Some(entries) = self.entries.as_mut() {
            entries.clear();
        }
        self.db_version = db_version;
    }

    /// Returns the current number of cached scores.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.as_ref().map_or(0, LruCache::len)
    }

    /// Returns whether the cache currently holds no scores.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Builds the stable cache key for a rerank request item.
#[must_use]
pub fn key_for(chunk_text: &str, query_text: &str) -> RerankCacheKey {
    RerankCacheKey {
        chunk_text_hash: xxh3_64(chunk_text.as_bytes()),
        query_text_hash: xxh3_64(query_text.as_bytes()),
    }
}

/// Looks up a normalized score in the process-global rerank cache.
pub fn lookup(chunk_text: &str, query_text: &str, db_version: u64) -> Option<f64> {
    let key = key_for(chunk_text, query_text);
    let Ok(mut cache) = RERANK_CACHE.lock() else {
        return None;
    };
    cache.get(key, db_version)
}

/// Stores a normalized score in the process-global rerank cache.
pub fn store(chunk_text: &str, query_text: &str, score: f64, db_version: u64) {
    let key = key_for(chunk_text, query_text);
    let Ok(mut cache) = RERANK_CACHE.lock() else {
        return;
    };
    cache.put(key, score, db_version);
}

/// Configures the process-global rerank cache capacity before first use.
pub fn configure_capacity(capacity: usize) {
    let _ = RERANK_CACHE_CAPACITY.set(capacity);
}

fn default_capacity() -> usize {
    RERANK_CACHE_CAPACITY
        .get()
        .copied()
        .unwrap_or(RERANK_CACHE_SIZE)
}

static RERANK_CACHE: LazyLock<Mutex<RerankCache>> =
    LazyLock::new(|| Mutex::new(RerankCache::new(default_capacity())));
static RERANK_CACHE_CAPACITY: OnceLock<usize> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_returns_score_when_db_version_matches() {
        let mut cache = RerankCache::new(2);
        let key = key_for("chunk text", "query");
        cache.put(key, 0.73, 1);
        assert_eq!(cache.get(key, 1), Some(0.73));
    }

    #[test]
    fn cache_clears_scores_when_db_version_changes() {
        let mut cache = RerankCache::new(2);
        let key = key_for("chunk text", "query");
        cache.put(key, 0.73, 1);
        assert_eq!(cache.get(key, 2), None);
        assert!(cache.is_empty());
    }

    #[test]
    fn cache_key_uses_chunk_text_not_path() {
        let first = key_for("same snippet", "query");
        let second = key_for("same snippet", "query");
        let different = key_for("other snippet", "query");
        assert_eq!(first, second);
        assert_ne!(first, different);
    }
}
