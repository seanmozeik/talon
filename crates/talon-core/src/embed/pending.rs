//! Reads pending chunks from the index, grouped by their parent note.
//!
//! Ports `sync/pending-chunks.ts`. `force=true` ignores `embedding_status`
//! and selects every chunk on every active note (used to rebuild after a
//! dimension change).

use rusqlite::{Connection, params_from_iter};

use crate::TalonError;

/// Row-group passed to the embed pipeline.
///
/// Chunks for one note travel together so the runner can pick the
/// `/embed` vs `/embed-chunked` endpoint based on chunk count.
#[derive(Debug, Clone)]
pub struct NoteWithChunks {
    /// `notes.id`.
    pub note_id: i64,
    /// Vault-relative path (used for diagnostics + restrict filtering).
    pub vault_path: String,
    /// Note title (display only).
    pub title: String,
    /// Chunks in `chunk_index` order.
    pub chunks: Vec<ChunkInfo>,
}

/// One pending chunk.
#[derive(Debug, Clone)]
pub struct ChunkInfo {
    /// `chunks.id`.
    pub chunk_id: i64,
    /// Pre-rendered embedding text (heading path + body).
    pub embedding_text: String,
    /// Content hash (used to detect "already embedded with this exact text").
    pub chunk_hash: String,
}

/// Hard cap per pass so a one-shot embed run cannot DOS the sidecar with a
/// freshly-imported vault.
pub const MAX_PENDING_CHUNKS_PER_PASS: u32 = 5_000;

/// Returns up to [`MAX_PENDING_CHUNKS_PER_PASS`] pending chunks, grouped by
/// note.
///
/// `restrict_paths` (when non-empty) limits the scan to the given vault-
/// relative paths. `force=true` ignores `embedding_status` and returns
/// every chunk on every active note in the matched set.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] for any underlying query failure.
pub fn get_pending_chunks(
    conn: &Connection,
    force: bool,
    restrict_paths: &[String],
) -> Result<Vec<NoteWithChunks>, TalonError> {
    let path_clause = if restrict_paths.is_empty() {
        String::new()
    } else {
        let placeholders = std::iter::repeat_n("?", restrict_paths.len())
            .collect::<Vec<_>>()
            .join(",");
        format!(" AND n.vault_path IN ({placeholders})")
    };
    let status_clause = if force {
        String::new()
    } else {
        " AND c.embedding_status IN ('pending', 'failed')".to_string()
    };
    let sql = format!(
        "SELECT c.id, c.embedding_text, c.chunk_hash, n.id, n.title, n.vault_path
         FROM chunks c
         JOIN notes n ON c.note_id = n.id
         WHERE n.active = 1{path_clause}{status_clause}
         ORDER BY n.id, c.chunk_index
         LIMIT {MAX_PENDING_CHUNKS_PER_PASS}"
    );
    let mut stmt = conn.prepare(&sql).map_err(|source| TalonError::Sqlite {
        context: "prepare pending_chunks query",
        source,
    })?;
    let mapped = stmt
        .query_map(
            params_from_iter(restrict_paths.iter()),
            |row| -> rusqlite::Result<(ChunkInfo, i64, String, String)> {
                let chunk = ChunkInfo {
                    chunk_id: row.get(0)?,
                    embedding_text: row.get(1)?,
                    chunk_hash: row.get(2)?,
                };
                let note_id: i64 = row.get(3)?;
                let title: Option<String> = row.get(4)?;
                let vault_path: String = row.get(5)?;
                Ok((chunk, note_id, title.unwrap_or_default(), vault_path))
            },
        )
        .map_err(|source| TalonError::Sqlite {
            context: "execute pending_chunks query",
            source,
        })?;
    let mut grouped: Vec<NoteWithChunks> = Vec::new();
    for row in mapped {
        let (chunk, note_id, title, vault_path) = row.map_err(|source| TalonError::Sqlite {
            context: "iterate pending_chunks rows",
            source,
        })?;
        if let Some(last) = grouped.last_mut()
            && last.note_id == note_id
        {
            last.chunks.push(chunk);
            continue;
        }
        grouped.push(NoteWithChunks {
            note_id,
            vault_path,
            title,
            chunks: vec![chunk],
        });
    }
    Ok(grouped)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::store::open_database;
    use rusqlite::params;
    use std::env::temp_dir;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_path() -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        temp_dir().join(format!("talon-pending-test-{pid}-{n}.sqlite"))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = fs_err::remove_file(path);
        let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
    }

    fn insert_note_with_chunks(
        conn: &Connection,
        path: &str,
        title: &str,
        chunks: &[(&str, &str, &str)], // (text, hash, status)
        active: bool,
    ) -> i64 {
        conn.execute(
            "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
             VALUES (?, ?, '[]', '[]', '', 0, 0, 'h', 'd', ?)",
            params![path, title, i64::from(active)],
        ).unwrap();
        let note_id = conn.last_insert_rowid();
        for (i, (text, hash, status)) in chunks.iter().enumerate() {
            conn.execute(
                "INSERT INTO chunks (note_id, chunk_index, text, embedding_text, heading_path, char_start, char_end, chunk_hash, token_estimate, embedding_status)
                 VALUES (?, ?, ?, ?, '', 0, 0, ?, 1, ?)",
                params![note_id, i64::try_from(i).unwrap(), text, text, hash, status],
            ).unwrap();
        }
        note_id
    }

    #[test]
    fn returns_only_pending_and_failed_when_not_forced() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        insert_note_with_chunks(&conn, "a.md", "A", &[("x", "h1", "pending")], true);
        insert_note_with_chunks(&conn, "b.md", "B", &[("y", "h2", "ok")], true);
        insert_note_with_chunks(&conn, "c.md", "C", &[("z", "h3", "failed")], true);
        let groups = get_pending_chunks(&conn, false, &[]).unwrap();
        let paths: Vec<&str> = groups.iter().map(|g| g.vault_path.as_str()).collect();
        assert!(paths.contains(&"a.md"));
        assert!(paths.contains(&"c.md"));
        assert!(!paths.contains(&"b.md"));
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn force_returns_every_chunk_on_active_notes() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        insert_note_with_chunks(&conn, "a.md", "A", &[("x", "h1", "ok")], true);
        insert_note_with_chunks(&conn, "b.md", "B", &[("y", "h2", "ok")], false);
        let groups = get_pending_chunks(&conn, true, &[]).unwrap();
        let paths: Vec<&str> = groups.iter().map(|g| g.vault_path.as_str()).collect();
        assert_eq!(paths, vec!["a.md"]);
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn restrict_paths_filters() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        insert_note_with_chunks(&conn, "a.md", "A", &[("x", "h1", "pending")], true);
        insert_note_with_chunks(&conn, "b.md", "B", &[("y", "h2", "pending")], true);
        let groups = get_pending_chunks(&conn, false, &["a.md".to_string()]).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].vault_path, "a.md");
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn groups_chunks_by_note_in_chunk_index_order() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        insert_note_with_chunks(
            &conn,
            "multi.md",
            "Multi",
            &[
                ("c0", "h0", "pending"),
                ("c1", "h1", "pending"),
                ("c2", "h2", "pending"),
            ],
            true,
        );
        let groups = get_pending_chunks(&conn, false, &[]).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].chunks.len(), 3);
        let texts: Vec<&str> = groups[0]
            .chunks
            .iter()
            .map(|c| c.embedding_text.as_str())
            .collect();
        assert_eq!(texts, vec!["c0", "c1", "c2"]);
        drop(conn);
        cleanup(&path);
    }
}
