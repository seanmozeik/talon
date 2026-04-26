//! `sqlite-vec` cosine similarity search against `vec_chunks`.
//!
//! Ports `services/talon/search/vector.ts`. The DB-backed [`search_vector`]
//! requires the `sqlite-vec` extension to be loaded into the connection (see
//! [`crate::store`] for the loader hook, wired in Phase 4 once the embedding
//! pipeline lands). The pure helper [`distance_to_score`] is independent of
//! the extension and is used by callers to interpret returned distances.

use rusqlite::{Connection, params, types::Value};
use std::rc::Rc;

use super::constants::COSINE_DISTANCE_MAX;
use super::types::{RawSearchResult, SearchScores};

/// Maps a cosine distance into a `[0, 1]` score using the standard
/// `1 - distance / max` transform clamped at zero.
#[must_use]
pub fn distance_to_score(distance: f64) -> f64 {
    (1.0 - distance / COSINE_DISTANCE_MAX).max(0.0)
}

fn parse_string_array(raw: Option<String>) -> Vec<String> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default()
}

/// Searches `vec_chunks` for the `candidate_limit` nearest chunks to
/// `embedding`, then joins back to `chunks`/`notes` for metadata.
///
/// The two-step pattern (separate distance query, then metadata fetch) mirrors
/// the TS reference and is required by `sqlite-vec`'s `MATCH` operator
/// (which doesn't compose well with arbitrary joins).
///
/// Returns an empty vector if `embedding` is empty, if the `sqlite-vec`
/// extension is not loaded, or on any SQL error.
#[must_use]
pub fn search_vector(
    conn: &Connection,
    embedding: &[f32],
    candidate_limit: u32,
) -> Vec<RawSearchResult> {
    if embedding.is_empty() {
        return Vec::new();
    }
    let embedding_json = serde_json::to_string(embedding).unwrap_or_else(|_| "[]".into());

    let Ok(chunk_ids_distances) = fetch_vector_distances(conn, &embedding_json, candidate_limit)
    else {
        return Vec::new();
    };
    if chunk_ids_distances.is_empty() {
        return Vec::new();
    }
    let chunk_ids: Vec<i64> = chunk_ids_distances.iter().map(|(id, _)| *id).collect();
    let Ok(chunks) = fetch_chunk_metadata(conn, &chunk_ids) else {
        return Vec::new();
    };

    let mut by_id: std::collections::HashMap<i64, ChunkMetadata> =
        std::collections::HashMap::with_capacity(chunks.len());
    for c in chunks {
        by_id.insert(c.id, c);
    }

    chunk_ids_distances
        .into_iter()
        .filter_map(|(id, distance)| {
            let c = by_id.remove(&id)?;
            let score = distance_to_score(distance);
            Some(RawSearchResult {
                path: c.vault_path,
                title: c.title.unwrap_or_default(),
                tags: parse_string_array(c.tags),
                aliases: parse_string_array(c.aliases),
                snippet: c.text,
                score,
                scores: SearchScores {
                    semantic: Some(score),
                    ..Default::default()
                },
            })
        })
        .collect()
}

fn fetch_vector_distances(
    conn: &Connection,
    embedding_json: &str,
    candidate_limit: u32,
) -> rusqlite::Result<Vec<(i64, f64)>> {
    let mut stmt = conn.prepare(
        "SELECT chunk_id, distance
         FROM vec_chunks
         WHERE embedding MATCH vec_f32(?)
           AND k = ?
         ORDER BY distance
         LIMIT ?",
    )?;
    let rows = stmt.query_map(
        params![embedding_json, candidate_limit, candidate_limit],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?)),
    )?;
    rows.collect()
}

struct ChunkMetadata {
    id: i64,
    text: String,
    vault_path: String,
    title: Option<String>,
    tags: Option<String>,
    aliases: Option<String>,
}

fn fetch_chunk_metadata(
    conn: &Connection,
    chunk_ids: &[i64],
) -> rusqlite::Result<Vec<ChunkMetadata>> {
    if chunk_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat_n("?", chunk_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT c.id, c.text, n.vault_path, n.title, n.tags, n.aliases
         FROM chunks c
         JOIN notes n ON n.id = c.note_id
         WHERE c.id IN ({placeholders}) AND n.active = 1"
    );
    let mut stmt = conn.prepare(&sql)?;
    // rusqlite's varargs API needs an iterator of values.
    let values: Vec<Value> = chunk_ids.iter().copied().map(Value::Integer).collect();
    let params_array = Rc::new(values);
    let rows = stmt.query_map(rusqlite::params_from_iter(params_array.iter()), |row| {
        Ok(ChunkMetadata {
            id: row.get(0)?,
            text: row.get(1)?,
            vault_path: row.get(2)?,
            title: row.get(3)?,
            tags: row.get(4)?,
            aliases: row.get(5)?,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn distance_to_score_zero_distance_is_one() {
        assert_eq!(distance_to_score(0.0), 1.0);
    }

    #[test]
    fn distance_to_score_max_distance_is_zero() {
        assert_eq!(distance_to_score(COSINE_DISTANCE_MAX), 0.0);
    }

    #[test]
    fn distance_to_score_above_max_is_clamped_to_zero() {
        assert_eq!(distance_to_score(COSINE_DISTANCE_MAX + 1.0), 0.0);
    }

    #[test]
    fn distance_to_score_midpoint_is_half() {
        assert_eq!(distance_to_score(COSINE_DISTANCE_MAX / 2.0), 0.5);
    }

    #[test]
    fn search_vector_empty_embedding_returns_empty() {
        // No DB needed — the empty-input guard short-circuits.
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        assert!(search_vector(&conn, &[], 10).is_empty());
    }

    #[test]
    fn search_vector_without_extension_returns_empty() {
        // Without sqlite-vec loaded, the prepare will fail. The function
        // should swallow that and return an empty result rather than panicking.
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        assert!(search_vector(&conn, &[0.1, 0.2, 0.3], 10).is_empty());
    }
}
