use std::collections::{BTreeMap, HashSet};

use rusqlite::{Connection, params};

use crate::TalonError;
use crate::indexing::migrations::bump_db_version;

use super::ChunkUpsertRow;

/// Upserts the chunks for `note_id`.
///
/// Applies the dedup-by-hash rule: chunks whose `chunk_hash` is unchanged
/// keep their `embedding_status`, chunks whose hash changed are re-marked
/// `'pending'`, and orphan rows are deleted.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] on any underlying statement failure.
pub fn upsert_chunks(
    conn: &Connection,
    note_id: i64,
    chunks: &[ChunkUpsertRow],
) -> Result<(), TalonError> {
    let existing_by_index = load_existing_chunks_by_index(conn, note_id)?;
    let mut seen: HashSet<i64> = HashSet::new();
    for chunk in chunks {
        let idx = i64::from(chunk.index);
        seen.insert(idx);
        match existing_by_index.get(&idx) {
            Some((row_id, existing_hash)) if existing_hash == &chunk.chunk_hash => {
                update_unchanged_chunk(conn, *row_id, chunk)?;
            }
            Some((row_id, _)) => update_changed_chunk(conn, *row_id, chunk)?,
            None => insert_chunk(conn, note_id, chunk)?,
        }
    }
    delete_orphan_chunks(conn, &existing_by_index, &seen)?;
    bump_db_version(conn)?;
    Ok(())
}

fn load_existing_chunks_by_index(
    conn: &Connection,
    note_id: i64,
) -> Result<BTreeMap<i64, (i64, String)>, TalonError> {
    let mut stmt = conn
        .prepare_cached("SELECT id, chunk_index, chunk_hash FROM chunks WHERE note_id = ?")
        .map_err(|source| TalonError::Sqlite {
            context: "prepare existing chunks query",
            source,
        })?;
    let rows = stmt
        .query_map([note_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|source| TalonError::Sqlite {
            context: "query existing chunks",
            source,
        })?;
    let mut out: BTreeMap<i64, (i64, String)> = BTreeMap::new();
    for r in rows {
        let (id, idx, hash) = r.map_err(|source| TalonError::Sqlite {
            context: "iterate existing chunks",
            source,
        })?;
        out.insert(idx, (id, hash));
    }
    Ok(out)
}

fn update_unchanged_chunk(
    conn: &Connection,
    row_id: i64,
    chunk: &ChunkUpsertRow,
) -> Result<(), TalonError> {
    conn.execute(
        "UPDATE chunks SET text = ?, embedding_text = ?, heading_path = ?,
                           char_start = ?, char_end = ?, line_start = ?, line_end = ?,
                           token_estimate = ?
         WHERE id = ?",
        params![
            chunk.text,
            chunk.embedding_text,
            chunk.heading_path,
            chunk.char_start,
            chunk.char_end,
            chunk.line_start,
            chunk.line_end,
            chunk.token_estimate,
            row_id,
        ],
    )
    .map_err(|source| TalonError::Sqlite {
        context: "update unchanged chunk",
        source,
    })?;
    Ok(())
}

fn update_changed_chunk(
    conn: &Connection,
    row_id: i64,
    chunk: &ChunkUpsertRow,
) -> Result<(), TalonError> {
    conn.execute(
        "UPDATE chunks SET text = ?, embedding_text = ?, heading_path = ?,
                           char_start = ?, char_end = ?, line_start = ?, line_end = ?,
                           chunk_hash = ?, token_estimate = ?, embedding_status = 'pending'
         WHERE id = ?",
        params![
            chunk.text,
            chunk.embedding_text,
            chunk.heading_path,
            chunk.char_start,
            chunk.char_end,
            chunk.line_start,
            chunk.line_end,
            chunk.chunk_hash,
            chunk.token_estimate,
            row_id,
        ],
    )
    .map_err(|source| TalonError::Sqlite {
        context: "update changed chunk",
        source,
    })?;
    Ok(())
}

fn insert_chunk(conn: &Connection, note_id: i64, chunk: &ChunkUpsertRow) -> Result<(), TalonError> {
    conn.execute(
        "INSERT INTO chunks
           (note_id, chunk_index, text, embedding_text, heading_path,
            char_start, char_end, line_start, line_end, chunk_hash,
            token_estimate, embedding_status)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending')",
        params![
            note_id,
            chunk.index,
            chunk.text,
            chunk.embedding_text,
            chunk.heading_path,
            chunk.char_start,
            chunk.char_end,
            chunk.line_start,
            chunk.line_end,
            chunk.chunk_hash,
            chunk.token_estimate,
        ],
    )
    .map_err(|source| TalonError::Sqlite {
        context: "insert chunk",
        source,
    })?;
    Ok(())
}

fn delete_orphan_chunks(
    conn: &Connection,
    existing_by_index: &BTreeMap<i64, (i64, String)>,
    seen: &HashSet<i64>,
) -> Result<(), TalonError> {
    for (idx, (row_id, _)) in existing_by_index {
        if !seen.contains(idx) {
            conn.execute("DELETE FROM chunks WHERE id = ?", [row_id])
                .map_err(|source| TalonError::Sqlite {
                    context: "delete orphan chunk",
                    source,
                })?;
        }
    }
    Ok(())
}
