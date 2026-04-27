//! `sqlite-vec` cosine similarity search against `vec_chunks`.
//!
//! Ports `services/talon/search/vector.ts`. The DB-backed [`search_vector`]
//! requires the `sqlite-vec` extension to be loaded into the connection (see
//! [`crate::store`] for the loader hook, wired in Phase 4 once the embedding
//! pipeline lands). The pure helper [`distance_to_score`] is independent of
//! the extension and is used by callers to interpret returned distances.

use rusqlite::{Connection, params, types::Value};
use std::collections::{HashMap, HashSet};
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
    let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm == 0.0 {
        return Vec::new();
    }
    let normalized = crate::embed::normalize_unit(embedding);
    let embedding_json = serde_json::to_string(&normalized).unwrap_or_else(|_| "[]".into());

    // Fetch 5× more candidates than needed so per-note dedup has enough pool
    // to fill the requested `candidate_limit` after collapsing multi-chunk notes.
    // Reference: obsidian-hybrid-search searcher.ts:674.
    let pool_size = candidate_limit.saturating_mul(5);
    let Ok(chunk_ids_distances) = fetch_vector_distances(conn, &embedding_json, pool_size) else {
        return Vec::new();
    };
    if chunk_ids_distances.is_empty() {
        return Vec::new();
    }
    let chunk_ids: Vec<i64> = chunk_ids_distances.iter().map(|(id, _)| *id).collect();
    let Ok(chunks) = fetch_chunk_metadata(conn, &chunk_ids) else {
        return Vec::new();
    };

    let mut by_id: HashMap<i64, ChunkMetadata> = HashMap::with_capacity(chunks.len());
    for c in chunks {
        by_id.insert(c.id, c);
    }

    // Dedup: keep only the best (lowest-distance) chunk per note.
    // chunk_ids_distances is ordered by distance ascending from sqlite-vec,
    // so the first occurrence of each note_id is always the closest chunk.
    // Reference: obsidian-hybrid-search searcher.ts:655-672.
    let mut seen_notes: HashSet<i64> = HashSet::new();
    chunk_ids_distances
        .into_iter()
        .filter_map(|(id, distance)| {
            let c = by_id.remove(&id)?;
            if !seen_notes.insert(c.note_id) {
                return None; // already have a closer chunk from this note
            }
            let score = distance_to_score(distance);
            let char_start = c.char_start.and_then(|v| u32::try_from(v).ok());
            let char_end = c.char_end.and_then(|v| u32::try_from(v).ok());
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
                semantic_heading: c.heading_path,
                semantic_char_start: char_start,
                semantic_char_end: char_end,
            })
        })
        .take(candidate_limit as usize)
        .collect()
}

fn fetch_vector_distances(
    conn: &Connection,
    embedding_json: &str,
    candidate_limit: u32,
) -> rusqlite::Result<Vec<(i64, f64)>> {
    // sqlite-vec requires `k` to be a literal — bind parameters are not
    // recognised by its xBestIndex implementation.  We cannot provide both `k`
    // and `LIMIT`; using only `k` is equivalent and enables the ANN index.
    // `candidate_limit` is always u32 so interpolation is safe.
    let sql = format!(
        "SELECT chunk_id, distance
         FROM vec_chunks
         WHERE embedding MATCH vec_f32(?)
           AND k = {candidate_limit}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![embedding_json], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
    })?;
    rows.collect()
}

struct ChunkMetadata {
    id: i64,
    note_id: i64,
    text: String,
    vault_path: String,
    title: Option<String>,
    tags: Option<String>,
    aliases: Option<String>,
    heading_path: Option<String>,
    char_start: Option<i64>,
    char_end: Option<i64>,
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
        "SELECT c.id, c.note_id, c.text, n.vault_path, n.title, n.tags, n.aliases,
                c.heading_path, c.char_start, c.char_end
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
            note_id: row.get(1)?,
            text: row.get(2)?,
            vault_path: row.get(3)?,
            title: row.get(4)?,
            tags: row.get(5)?,
            aliases: row.get(6)?,
            heading_path: row.get(7)?,
            char_start: row.get(8)?,
            char_end: row.get(9)?,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]
mod tests;
