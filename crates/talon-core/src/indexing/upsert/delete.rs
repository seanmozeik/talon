use rusqlite::{Connection, params};

use crate::TalonError;

/// Soft-deletes `note_id`: clears searchable fields and child rows.
///
/// The note row stays so foreign-key references and `--since` queries can
/// still resolve the path. The `vec_chunks` `DELETE` is best-effort and fails
/// silently if the `sqlite-vec` extension is not loaded.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] on any non-vector statement failure.
pub fn perform_note_deletion(
    conn: &Connection,
    note_id: i64,
    vault_path: &str,
) -> Result<(), TalonError> {
    let chunk_ids: Vec<i64> = {
        let mut stmt = conn
            .prepare_cached("SELECT id FROM chunks WHERE note_id = ?")
            .map_err(|source| TalonError::Sqlite {
                context: "prepare chunk-id lookup",
                source,
            })?;
        let rows = stmt
            .query_map([note_id], |row| row.get::<_, i64>(0))
            .map_err(|source| TalonError::Sqlite {
                context: "query chunk ids",
                source,
            })?;
        rows.filter_map(Result::ok).collect()
    };

    for id in &chunk_ids {
        // sqlite-vec table — ignore errors when extension not loaded.
        let _ = conn.execute("DELETE FROM vec_chunks WHERE chunk_id = ?", [id]);
        conn.execute("DELETE FROM vector_metadata WHERE chunk_id = ?", [id])
            .map_err(|source| TalonError::Sqlite {
                context: "delete vector metadata",
                source,
            })?;
    }

    conn.execute(
        "UPDATE notes SET
           active = 0, title = '', tags = '[]', aliases = '[]',
           content = '', frontmatter = '{}'
         WHERE id = ?",
        [note_id],
    )
    .map_err(|source| TalonError::Sqlite {
        context: "soft-delete note",
        source,
    })?;
    conn.execute("DELETE FROM chunks WHERE note_id = ?", [note_id])
        .map_err(|source| TalonError::Sqlite {
            context: "delete chunks",
            source,
        })?;
    conn.execute("DELETE FROM links WHERE from_path = ?", [vault_path])
        .map_err(|source| TalonError::Sqlite {
            context: "delete links",
            source,
        })?;
    conn.execute("DELETE FROM note_aliases WHERE note_id = ?", [note_id])
        .map_err(|source| TalonError::Sqlite {
            context: "delete aliases",
            source,
        })?;
    conn.execute("DELETE FROM note_tags WHERE note_id = ?", [note_id])
        .map_err(|source| TalonError::Sqlite {
            context: "delete tags",
            source,
        })?;
    conn.execute(
        "DELETE FROM note_frontmatter_fields WHERE note_id = ?",
        [note_id],
    )
    .map_err(|source| TalonError::Sqlite {
        context: "delete frontmatter fields",
        source,
    })?;

    let timestamp = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| String::from("?"));
    conn.execute(
        "INSERT INTO event_log (action, path, timestamp) VALUES (?, ?, ?)",
        params!["delete", vault_path, timestamp],
    )
    .map_err(|source| TalonError::Sqlite {
        context: "log delete event",
        source,
    })?;
    Ok(())
}
