//! Writes embedding vectors back to `vec_chunks` + `vector_metadata`.
//!
//! Ports `embed/chunks-persist.ts`. The vector is JSON-encoded inside SQL
//! because `vec0` accepts `json('[...]')` for `INSERT`; this avoids a
//! sqlite-vec specific binding shim.

use rusqlite::{Connection, params};

use crate::TalonError;
use crate::inference::EmbedChunkedDataItem;

/// Persists one chunk's embedding vector + metadata.
///
/// Writes to three tables:
///
/// 1. `vector_metadata` — model, dimension, embedded-at timestamp.
/// 2. `chunks.embedding_status = 'ok'`.
/// 3. `vec_chunks(rowid, embedding)` — the searchable vector itself.
///
/// The `vec_chunks` write is skipped silently if the table does not exist
/// (the `sqlite-vec` extension is not loaded). The chunk is still marked
/// `ok` because the upstream embedding text won't change before the next
/// embed pass.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] if `vector_metadata` or `chunks` updates
/// fail. `vec_chunks` failures are swallowed so the metadata bookkeeping
/// stays consistent across `vec_ext`-disabled environments.
pub fn persist_chunk_vector(
    conn: &Connection,
    chunk_id: i64,
    model: &str,
    dimensions: u32,
    embedded_at_ms: i64,
    embedding: &[f32],
) -> Result<(), TalonError> {
    conn.execute(
        "INSERT OR REPLACE INTO vector_metadata (chunk_id, model, dimensions, embedded_at_ms)
         VALUES (?, ?, ?, ?)",
        params![chunk_id, model, dimensions, embedded_at_ms],
    )
    .map_err(|source| TalonError::Sqlite {
        context: "upsert vector_metadata",
        source,
    })?;
    conn.execute(
        "UPDATE chunks SET embedding_status = 'ok' WHERE id = ?",
        params![chunk_id],
    )
    .map_err(|source| TalonError::Sqlite {
        context: "mark chunk embedded",
        source,
    })?;

    // Normalize to unit length so the cosine-from-L2 identity holds:
    //   similarity = max(0, 1 - distance² / 2)
    // sqlite-vec uses distance_metric=cosine which assumes ||v|| = 1.
    let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    let normalized: Vec<f32> = if norm > 0.0 {
        embedding.iter().map(|x| x / norm).collect()
    } else {
        embedding.to_vec()
    };
    let json = serde_json::to_string(&normalized).unwrap_or_else(|_| "[]".to_string());
    let _ = conn.execute(
        "INSERT OR REPLACE INTO vec_chunks(chunk_id, embedding) VALUES (?, json(?))",
        params![chunk_id, json],
    );
    Ok(())
}

/// First non-empty group from a `/embed-chunked` response, paired with its
/// detected dimensionality.
///
/// Returns `None` if the response is structurally empty or carries
/// zero-length embeddings.
#[must_use]
pub fn first_non_empty_batch(
    response: &crate::inference::EmbedChunkedResponse,
) -> Option<(u32, &EmbedChunkedDataItem)> {
    let row = response.data.first()?;
    let dims = row.embeddings.first()?.len();
    if dims == 0 {
        return None;
    }
    let dims_u32 = u32::try_from(dims).ok()?;
    Some((dims_u32, row))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::inference::EmbedChunkedResponse;
    use crate::store::open_database;
    use crate::vec_ext::{ensure_vec_chunks, register_sqlite_vec};
    use std::env::temp_dir;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_path(label: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        temp_dir().join(format!("talon-persist-test-{label}-{pid}-{n}.sqlite"))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = fs_err::remove_file(path);
        let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
    }

    fn seed_chunk(conn: &Connection) -> i64 {
        conn.execute(
            "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
             VALUES ('a.md', 'A', '[]', '[]', '', 0, 0, 'h', 'd', 1)",
            [],
        )
        .unwrap();
        let note_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO chunks (note_id, chunk_index, text, embedding_text, heading_path, char_start, char_end, chunk_hash, token_estimate, embedding_status)
             VALUES (?, 0, 'body', 'body', '', 0, 4, 'h', 1, 'pending')",
            params![note_id],
        ).unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn persist_writes_metadata_and_marks_chunk_ok() {
        register_sqlite_vec().unwrap();
        let path = unique_path("metadata");
        let conn = open_database(&path).unwrap();
        ensure_vec_chunks(&conn, 4).unwrap();
        let chunk_id = seed_chunk(&conn);
        persist_chunk_vector(&conn, chunk_id, "test-model", 4, 1, &[0.1, 0.2, 0.3, 0.4]).unwrap();

        let (model, dims): (String, i64) = conn
            .query_row(
                "SELECT model, dimensions FROM vector_metadata WHERE chunk_id = ?",
                params![chunk_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(model, "test-model");
        assert_eq!(dims, 4);
        let status: String = conn
            .query_row(
                "SELECT embedding_status FROM chunks WHERE id = ?",
                params![chunk_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "ok");
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn persist_swallows_missing_vec_chunks_table() {
        // Don't call ensure_vec_chunks — vec_chunks does not exist.
        let path = unique_path("no-vec");
        let conn = open_database(&path).unwrap();
        let chunk_id = seed_chunk(&conn);
        let result = persist_chunk_vector(&conn, chunk_id, "m", 4, 1, &[1.0, 2.0, 3.0, 4.0]);
        assert!(result.is_ok());
        let status: String = conn
            .query_row(
                "SELECT embedding_status FROM chunks WHERE id = ?",
                params![chunk_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "ok");
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn first_non_empty_batch_returns_dimensions() {
        let response = EmbedChunkedResponse {
            data: vec![EmbedChunkedDataItem {
                embeddings: vec![vec![0.1, 0.2, 0.3]],
                index: 0,
            }],
            model: "m".into(),
        };
        let (dims, _) = first_non_empty_batch(&response).unwrap();
        assert_eq!(dims, 3);
    }

    #[test]
    fn first_non_empty_batch_none_on_empty_response() {
        let response = EmbedChunkedResponse {
            data: vec![],
            model: "m".into(),
        };
        assert!(first_non_empty_batch(&response).is_none());
    }

    #[test]
    fn persist_normalizes_vector_to_unit_norm() {
        register_sqlite_vec().unwrap();
        let path = unique_path("unit-norm");
        let conn = open_database(&path).unwrap();
        ensure_vec_chunks(&conn, 3).unwrap();
        let chunk_id = seed_chunk(&conn);
        // Non-unit embedding: [3, 4, 0] has ||v|| = 5.
        persist_chunk_vector(&conn, chunk_id, "test-model", 3, 1, &[3.0, 4.0, 0.0]).unwrap();

        let raw: String = conn
            .query_row(
                "SELECT vec_to_json(embedding) FROM vec_chunks WHERE chunk_id = ?",
                params![chunk_id],
                |r| r.get(0),
            )
            .unwrap();
        let stored: Vec<f32> = serde_json::from_str(&raw).unwrap();
        let norm: f32 = stored.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0_f32).abs() < 1e-4,
            "stored vector must be unit-normed, got ||v|| = {norm}"
        );
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn first_non_empty_batch_none_on_zero_dim() {
        let response = EmbedChunkedResponse {
            data: vec![EmbedChunkedDataItem {
                embeddings: vec![vec![]],
                index: 0,
            }],
            model: "m".into(),
        };
        assert!(first_non_empty_batch(&response).is_none());
    }
}
