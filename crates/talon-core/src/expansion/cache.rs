//! SQL-backed cache for LLM expansion responses with millisecond-precision TTL.

use rusqlite::Connection;

use crate::TalonError;
use crate::change_tracking::now_ms;

/// Persistent key-value cache backed by the `llm_cache` `SQLite` table.
///
/// Entries expire after the TTL supplied to [`put`](LlmCache::put) elapses.
/// Expired entries are invisible to [`get`](LlmCache::get) but remain in the
/// database until [`purge_expired`](LlmCache::purge_expired) is called.
pub struct LlmCache<'conn> {
    conn: &'conn Connection,
}

impl std::fmt::Debug for LlmCache<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmCache").finish_non_exhaustive()
    }
}

impl<'conn> LlmCache<'conn> {
    /// Wraps a database connection.
    pub const fn new(conn: &'conn Connection) -> Self {
        Self { conn }
    }

    /// Returns the cached value for `key` if it exists and has not expired.
    ///
    /// # Errors
    ///
    /// Returns [`TalonError::Sqlite`] on a database error.
    pub fn get(&self, key: &str) -> Result<Option<String>, TalonError> {
        let now = now_ms().cast_signed();
        let result = self.conn.query_row(
            "SELECT value FROM llm_cache WHERE key = ?1 AND expires_at_ms > ?2",
            rusqlite::params![key, now],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(source) => Err(TalonError::Sqlite {
                context: "llm_cache get",
                source,
            }),
        }
    }

    /// Stores `value` under `key` with a time-to-live of `ttl_ms` milliseconds.
    ///
    /// Overwrites any existing entry for the same key.
    ///
    /// # Errors
    ///
    /// Returns [`TalonError::Sqlite`] on a database error.
    pub fn put(&self, key: &str, value: &str, ttl_ms: u64) -> Result<(), TalonError> {
        let expires_at_ms = (now_ms() + ttl_ms).cast_signed();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO llm_cache (key, value, expires_at_ms) \
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![key, value, expires_at_ms],
            )
            .map_err(|source| TalonError::Sqlite {
                context: "llm_cache put",
                source,
            })?;
        Ok(())
    }

    /// Deletes all entries whose TTL has elapsed.
    ///
    /// Returns the number of rows removed.
    ///
    /// # Errors
    ///
    /// Returns [`TalonError::Sqlite`] on a database error.
    pub fn purge_expired(&self) -> Result<usize, TalonError> {
        let now = now_ms().cast_signed();
        let count = self
            .conn
            .execute(
                "DELETE FROM llm_cache WHERE expires_at_ms <= ?1",
                rusqlite::params![now],
            )
            .map_err(|source| TalonError::Sqlite {
                context: "llm_cache purge_expired",
                source,
            })?;
        Ok(count)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    use crate::migrations::run_migrations;

    fn fresh_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    #[test]
    fn round_trip_stores_and_retrieves_value() {
        let conn = fresh_db();
        let cache = LlmCache::new(&conn);
        cache.put("k1", "hello world", 60_000).unwrap();
        let result = cache.get("k1").unwrap();
        assert_eq!(result.as_deref(), Some("hello world"));
    }

    #[test]
    fn missing_key_returns_none() {
        let conn = fresh_db();
        let cache = LlmCache::new(&conn);
        let result = cache.get("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn expired_entry_returns_none() {
        let conn = fresh_db();
        let cache = LlmCache::new(&conn);
        // TTL of 0 means expires_at_ms == now_ms, which fails the > check.
        cache.put("k2", "stale", 0).unwrap();
        let result = cache.get("k2").unwrap();
        assert!(result.is_none(), "expired entry must not be returned");
    }

    #[test]
    fn put_overwrites_existing_key() {
        let conn = fresh_db();
        let cache = LlmCache::new(&conn);
        cache.put("k3", "first", 60_000).unwrap();
        cache.put("k3", "second", 60_000).unwrap();
        assert_eq!(cache.get("k3").unwrap().as_deref(), Some("second"));
    }

    #[test]
    fn purge_expired_removes_stale_rows() {
        let conn = fresh_db();
        let cache = LlmCache::new(&conn);
        cache.put("stale1", "v1", 0).unwrap();
        cache.put("stale2", "v2", 0).unwrap();
        cache.put("live", "v3", 60_000).unwrap();

        let removed = cache.purge_expired().unwrap();
        assert_eq!(removed, 2, "two stale entries should be purged");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM llm_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "only the live entry should remain");
    }
}
