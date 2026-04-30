//! Search-response cache invalidated by the `SQLite` index content version.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::{LazyLock, Mutex, OnceLock};

use lru::LruCache;
use rusqlite::Connection;

use crate::config::TalonConfig;
use crate::indexing::migrations::read_db_version;
use crate::search::constants::GLOBAL_HYBRID_CACHE_SIZE;
use crate::search::{SearchInput, SearchMode, SearchResponse, WhereClause, WhereOperator};
use crate::text::nfd;

const SEARCH_CACHE_SIZE_ENV: &str = "TALON_SEARCH_CACHE_SIZE";

static SEARCH_CACHE: LazyLock<Mutex<SearchCache>> =
    LazyLock::new(|| Mutex::new(SearchCache::new(default_capacity())));
static SEARCH_CACHE_CAPACITY: OnceLock<usize> = OnceLock::new();

/// Opaque hash of the stable inputs that influence a search response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CacheKey(u64);

/// Cached response plus the index content version it was computed against.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheEntry {
    /// Cached search response.
    pub response: SearchResponse,
    /// `db_meta.db_version` observed when the response was computed.
    pub db_version: u64,
}

/// LRU cache for search responses.
#[derive(Debug)]
pub struct SearchCache {
    entries: Option<LruCache<CacheKey, CacheEntry>>,
}

impl SearchCache {
    /// Creates a search cache with the requested capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let entries = NonZeroUsize::new(capacity).map(LruCache::new);
        Self { entries }
    }

    /// Returns a cached response when the stored `db_version` is still current.
    pub fn get(&mut self, key: CacheKey, db_version: u64) -> Option<SearchResponse> {
        let entries = self.entries.as_mut()?;
        let hit = entries.get(&key).cloned();
        match hit {
            Some(entry) if entry.db_version == db_version => Some(entry.response),
            Some(_) => {
                let _ = entries.pop(&key);
                None
            }
            None => None,
        }
    }

    /// Inserts a response computed against `db_version`.
    pub fn put(&mut self, key: CacheKey, response: SearchResponse, db_version: u64) {
        let Some(entries) = self.entries.as_mut() else {
            return;
        };
        entries.put(
            key,
            CacheEntry {
                response,
                db_version,
            },
        );
    }

    /// Returns the current number of cached responses.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.as_ref().map_or(0, LruCache::len)
    }

    /// Returns whether the cache currently holds no responses.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Looks up a response in the process-global search cache.
pub fn lookup(
    conn: &Connection,
    input: &SearchInput,
    config: Option<&TalonConfig>,
) -> Option<SearchResponse> {
    configure_from(config);
    let db_version = read_db_version(conn);
    let key = key_for(conn, input, config);
    let Ok(mut cache) = SEARCH_CACHE.lock() else {
        return None;
    };
    cache.get(key, db_version)
}

/// Stores a response in the process-global search cache.
pub fn store(
    conn: &Connection,
    input: &SearchInput,
    config: Option<&TalonConfig>,
    response: &SearchResponse,
) {
    configure_from(config);
    let db_version = read_db_version(conn);
    let key = key_for(conn, input, config);
    let Ok(mut cache) = SEARCH_CACHE.lock() else {
        return;
    };
    cache.put(key, response.clone(), db_version);
}

/// Builds the stable cache key for a search request.
#[must_use]
pub fn key_for(conn: &Connection, input: &SearchInput, config: Option<&TalonConfig>) -> CacheKey {
    let mut hasher = DefaultHasher::new();
    database_identity(conn).hash(&mut hasher);
    normalized_query(input.query.as_deref().unwrap_or_default()).hash(&mut hasher);
    input
        .intent
        .as_deref()
        .map(normalized_query)
        .hash(&mut hasher);
    input.queries.iter().for_each(|query| {
        normalized_query(query).hash(&mut hasher);
    });
    mode_key(input.mode).hash(&mut hasher);
    input.fast.hash(&mut hasher);
    input.limit.get().hash(&mut hasher);
    input.candidate_limit.get().hash(&mut hasher);
    sorted_where_clauses(&input.where_).hash(&mut hasher);
    input.since.hash(&mut hasher);
    input.anchors.hash(&mut hasher);
    input.path.hash(&mut hasher);
    input.tag.hash(&mut hasher);
    input.scope.hash(&mut hasher);
    input.scope_only.hash(&mut hasher);
    input.scope_all.hash(&mut hasher);
    config.map(config_fingerprint).hash(&mut hasher);
    CacheKey(hasher.finish())
}

fn default_capacity() -> usize {
    if let Some(capacity) = SEARCH_CACHE_CAPACITY.get() {
        return *capacity;
    }
    std::env::var(SEARCH_CACHE_SIZE_ENV)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(GLOBAL_HYBRID_CACHE_SIZE)
}

fn configure_from(config: Option<&TalonConfig>) {
    if let Some(config) = config {
        let _ = SEARCH_CACHE_CAPACITY.set(config.search.cache_size);
    }
}

fn normalized_query(query: &str) -> String {
    let collapsed = query
        .split_whitespace()
        .fold(String::new(), |mut acc, word| {
            if !acc.is_empty() {
                acc.push(' ');
            }
            acc.push_str(word);
            acc
        });
    nfd::normalize(&collapsed).to_lowercase()
}

fn sorted_where_clauses(clauses: &[WhereClause]) -> Vec<String> {
    let mut clauses = clauses
        .iter()
        .map(|clause| {
            format!(
                "{}\0{}\0{}",
                nfd::normalize(&clause.key).to_lowercase(),
                operator_key(clause.op),
                clause
                    .value
                    .as_deref()
                    .map(normalized_query)
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();
    clauses.sort_unstable();
    clauses
}

const fn mode_key(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Hybrid => "hybrid",
        SearchMode::Semantic => "semantic",
        SearchMode::Fulltext => "fulltext",
        SearchMode::Title => "title",
    }
}

const fn operator_key(op: WhereOperator) -> &'static str {
    match op {
        WhereOperator::Equals => "eq",
        WhereOperator::NotEquals => "ne",
        WhereOperator::LessThan => "lt",
        WhereOperator::LessThanOrEqual => "lte",
        WhereOperator::GreaterThan => "gt",
        WhereOperator::GreaterThanOrEqual => "gte",
        WhereOperator::Contains => "contains",
        WhereOperator::Exists => "exists",
        WhereOperator::StartsWith => "startswith",
        WhereOperator::GlobMatch => "glob",
    }
}

fn database_identity(conn: &Connection) -> String {
    conn.query_row("PRAGMA database_list", [], |row| row.get::<_, String>(2))
        .unwrap_or_default()
}

fn config_fingerprint(config: &TalonConfig) -> String {
    format!("{:?}", config.scopes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexing::migrations::bump_db_version;
    use crate::search::SearchResponse;

    #[test]
    fn cache_returns_response_when_db_version_matches() {
        let mut cache = SearchCache::new(2);
        let key = CacheKey(7);
        let response = SearchResponse::empty_input();
        cache.put(key, response.clone(), 1);

        assert_eq!(cache.get(key, 1), Some(response));
    }

    #[test]
    fn cache_evicts_response_when_db_version_changes() {
        let mut cache = SearchCache::new(2);
        let key = CacheKey(7);
        cache.put(key, SearchResponse::empty_input(), 1);

        assert!(cache.get(key, 2).is_none());
        assert!(cache.is_empty());
    }

    #[test]
    fn cache_key_ignores_where_clause_order() -> Result<(), Box<dyn std::error::Error>> {
        let conn = Connection::open_in_memory()?;
        let first = input_with_where(vec![
            where_clause("status", WhereOperator::Equals, Some("active")),
            where_clause("kind", WhereOperator::Contains, Some("note")),
        ]);
        let second = input_with_where(vec![
            where_clause("kind", WhereOperator::Contains, Some("note")),
            where_clause("status", WhereOperator::Equals, Some("active")),
        ]);

        assert_eq!(key_for(&conn, &first, None), key_for(&conn, &second, None));
        Ok(())
    }

    #[test]
    fn cache_entry_misses_after_bumped_db_version() -> Result<(), Box<dyn std::error::Error>> {
        let conn = Connection::open_in_memory()?;
        conn.execute(
            "CREATE TABLE db_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )?;
        conn.execute(
            "INSERT INTO db_meta(key, value) VALUES ('db_version', '0')",
            [],
        )?;
        conn.execute(
            "INSERT INTO settings(key, value) VALUES ('db_version', '0')",
            [],
        )?;
        let mut cache = SearchCache::new(2);
        let key = CacheKey(11);
        cache.put(key, SearchResponse::empty_input(), read_db_version(&conn));

        bump_db_version(&conn)?;

        assert!(cache.get(key, read_db_version(&conn)).is_none());
        Ok(())
    }

    fn input_with_where(where_: Vec<WhereClause>) -> SearchInput {
        SearchInput {
            query: Some("Atomic Notes".to_string()),
            where_,
            ..SearchInput::default()
        }
    }

    fn where_clause(key: &str, op: WhereOperator, value: Option<&str>) -> WhereClause {
        WhereClause {
            key: key.to_string(),
            op,
            value: value.map(str::to_string),
        }
    }
}
