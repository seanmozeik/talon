//! In-process LRU cache used by the hybrid search pipeline.
//!
//! Ports `services/talon/search/cache-lru.ts`. The on-disk LLM cache (table
//! `llm_cache`) is wired separately by the query layer; this module only
//! provides the in-memory eviction structure and the deduplication helper.

use std::collections::HashMap;
use std::collections::VecDeque;

use super::constants::GLOBAL_HYBRID_CACHE_SIZE;
use crate::text::nfd;

/// Generic LRU cache. `get` is `&mut self` because access reorders the
/// recency list; `set` evicts the oldest entry when over capacity.
#[derive(Debug)]
pub struct SearchCache<V> {
    map: HashMap<String, V>,
    order: VecDeque<String>,
    max_size: usize,
}

impl<V> SearchCache<V> {
    /// Creates a new cache with the default capacity ([`GLOBAL_HYBRID_CACHE_SIZE`]).
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(GLOBAL_HYBRID_CACHE_SIZE)
    }

    /// Creates a new cache with a custom capacity.
    ///
    /// A capacity of `0` disables caching: every `set` is immediately evicted.
    #[must_use]
    pub fn with_capacity(max_size: usize) -> Self {
        Self {
            map: HashMap::with_capacity(max_size.max(1)),
            order: VecDeque::with_capacity(max_size.max(1)),
            max_size,
        }
    }

    /// Returns the cached value for `key`, marking it as most-recently-used.
    pub fn get(&mut self, key: &str) -> Option<&V> {
        if !self.map.contains_key(key) {
            return None;
        }
        // Move key to back of recency queue.
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            let k = self.order.remove(pos)?;
            self.order.push_back(k);
        }
        self.map.get(key)
    }

    /// Inserts `value` for `key`, evicting the least-recently-used entry if
    /// the cache is over capacity.
    pub fn set(&mut self, key: String, value: V) {
        if self.map.contains_key(&key)
            && let Some(pos) = self.order.iter().position(|k| k == &key)
        {
            self.order.remove(pos);
        }
        self.map.insert(key.clone(), value);
        self.order.push_back(key);
        while self.order.len() > self.max_size {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            } else {
                break;
            }
        }
    }

    /// Clears all cached entries.
    pub fn invalidate(&mut self) {
        self.map.clear();
        self.order.clear();
    }

    /// Returns the current number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl<V> Default for SearchCache<V> {
    fn default() -> Self {
        Self::new()
    }
}

/// Builds a cache key that embeds `db_version` and `model` so a stale
/// in-process cache automatically misses when either changes.
///
/// `base_key` is typically the serialised search input. This function makes
/// the key opaque to callers: they do not need to remember to include version
/// information when constructing keys.
///
/// The `db_version` value should be read from `settings WHERE key = 'db_version'`.
/// Reference: obsidian-hybrid-search searcher.ts:982.
#[must_use]
pub fn make_versioned_key(base_key: &str, db_version: &str, model: &str) -> String {
    format!("{db_version}:{model}:{base_key}")
}

/// Reads `db_version` from the `settings` table.
///
/// Returns `"0"` if the settings table is missing or the row is absent
/// (matches the migration seed value).
#[must_use]
pub fn read_db_version(conn: &rusqlite::Connection) -> String {
    conn.query_row(
        "SELECT value FROM settings WHERE key = 'db_version'",
        [],
        |r| r.get::<_, String>(0),
    )
    .unwrap_or_else(|_| "0".to_string())
}

/// Trims, lowercases, and dedupes a list of query variants. Empty strings
/// are dropped. Order is preserved (first occurrence wins).
#[must_use]
pub fn dedupe_query_variants(variants: &[String]) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for v in variants {
        let normalized: String = v.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() {
            continue;
        }
        let key = nfd::normalize(&normalized).to_lowercase();
        if seen.insert(key) {
            out.push(normalized);
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn empty_cache_returns_none() {
        let mut c: SearchCache<u32> = SearchCache::with_capacity(2);
        assert!(c.get("missing").is_none());
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn set_then_get_returns_value() {
        let mut c: SearchCache<u32> = SearchCache::with_capacity(2);
        c.set("k1".into(), 1);
        assert_eq!(c.get("k1"), Some(&1));
    }

    #[test]
    fn lru_evicts_least_recently_used() {
        let mut c: SearchCache<u32> = SearchCache::with_capacity(2);
        c.set("a".into(), 1);
        c.set("b".into(), 2);
        c.set("c".into(), 3); // evicts a
        assert!(c.get("a").is_none());
        assert_eq!(c.get("b"), Some(&2));
        assert_eq!(c.get("c"), Some(&3));
    }

    #[test]
    fn get_marks_entry_as_recent() {
        let mut c: SearchCache<u32> = SearchCache::with_capacity(2);
        c.set("a".into(), 1);
        c.set("b".into(), 2);
        let _ = c.get("a"); // a is now most-recent
        c.set("c".into(), 3); // should evict b, not a
        assert_eq!(c.get("a"), Some(&1));
        assert!(c.get("b").is_none());
    }

    #[test]
    fn set_overwrites_existing_value_without_growing() {
        let mut c: SearchCache<u32> = SearchCache::with_capacity(2);
        c.set("a".into(), 1);
        c.set("a".into(), 2);
        assert_eq!(c.len(), 1);
        assert_eq!(c.get("a"), Some(&2));
    }

    #[test]
    fn invalidate_clears_everything() {
        let mut c: SearchCache<u32> = SearchCache::with_capacity(2);
        c.set("a".into(), 1);
        c.set("b".into(), 2);
        c.invalidate();
        assert!(c.is_empty());
    }

    #[test]
    fn dedupe_drops_empty_and_duplicates_case_insensitive() {
        let input = vec![
            "  zettelkasten ".into(),
            "Zettelkasten".into(),
            String::new(),
            "  ".into(),
            "atomic notes".into(),
            "atomic   notes".into(),
        ];
        let out = dedupe_query_variants(&input);
        assert_eq!(out, vec!["zettelkasten", "atomic notes"]);
    }

    #[test]
    fn dedupe_preserves_first_occurrence_form() {
        let input = vec!["Foo Bar".into(), "foo bar".into()];
        let out = dedupe_query_variants(&input);
        assert_eq!(out, vec!["Foo Bar"]);
    }

    #[test]
    fn make_versioned_key_embeds_version_and_model() {
        let k1 = make_versioned_key("search:foo", "1", "embed-v1");
        let k2 = make_versioned_key("search:foo", "2", "embed-v1");
        let k3 = make_versioned_key("search:foo", "1", "embed-v2");
        // Same base key with different db_version or model must differ.
        assert_ne!(k1, k2, "db_version change must invalidate key");
        assert_ne!(k1, k3, "model change must invalidate key");
        // Same inputs must produce the same key.
        assert_eq!(k1, make_versioned_key("search:foo", "1", "embed-v1"));
    }

    #[test]
    fn read_db_version_returns_seeded_value() {
        use crate::store::open_database;
        use std::env::temp_dir;
        use std::sync::atomic::{AtomicU64, Ordering};

        static CTR: AtomicU64 = AtomicU64::new(0);
        let n = CTR.fetch_add(1, Ordering::Relaxed);
        let path = temp_dir().join(format!("talon-cache-ver-{}-{n}.sqlite", std::process::id()));
        let conn = open_database(&path).unwrap();
        let ver = read_db_version(&conn);
        assert_eq!(ver, "0", "migration seeds db_version as '0'");
        drop(conn);
        let _ = fs_err::remove_file(&path);
        let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
    }

    #[test]
    fn read_db_version_falls_back_on_missing_table() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        // No settings table → falls back to "0".
        assert_eq!(read_db_version(&conn), "0");
    }
}
