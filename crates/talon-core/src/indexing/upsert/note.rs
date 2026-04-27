use rusqlite::{Connection, params};

use crate::TalonError;
use crate::indexer::prelude::hash_file_content;

use super::{NoteUpsertResult, UpsertNoteParams};

fn random_docid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("doc-{nanos:x}-{seq:x}")
}

/// Inserts a new note row or updates the existing row for `vault_path`.
///
/// Hashing, `JSON` serialization, and docid generation happen inside this
/// function so callers don't need to plumb them through.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] if any underlying statement fails or if
/// the post-insert `id` lookup fails.
pub fn upsert_note(
    conn: &Connection,
    params: &UpsertNoteParams<'_>,
) -> Result<NoteUpsertResult, TalonError> {
    let aliases_json =
        serde_json::to_string(params.aliases).map_err(|err| TalonError::Internal {
            message: format!(
                "serializing aliases for {} failed: {err}",
                params.vault_path
            ),
        })?;
    let tags_json = serde_json::to_string(params.tags).map_err(|err| TalonError::Internal {
        message: format!("serializing tags for {} failed: {err}", params.vault_path),
    })?;
    let frontmatter_json =
        serde_json::to_string(params.frontmatter).map_err(|err| TalonError::Internal {
            message: format!(
                "serializing frontmatter for {} failed: {err}",
                params.vault_path
            ),
        })?;
    let file_hash = hash_file_content(params.content);

    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM notes WHERE vault_path = ?",
            [params.vault_path],
            |row| row.get(0),
        )
        .ok();

    if let Some(note_id) = existing {
        conn.execute(
            "UPDATE notes SET
               title = ?, tags = ?, aliases = ?, content = ?, frontmatter = ?,
               mtime_ms = ?, size_bytes = ?, hash = ?, active = 1
             WHERE vault_path = ?",
            params![
                params.title,
                tags_json,
                aliases_json,
                params.content,
                frontmatter_json,
                params.mtime_ms,
                params.size_bytes,
                file_hash,
                params.vault_path,
            ],
        )
        .map_err(|source| TalonError::Sqlite {
            context: "update note",
            source,
        })?;
        return Ok(NoteUpsertResult {
            note_id,
            is_new: false,
        });
    }

    let docid = random_docid();
    conn.execute(
        "INSERT INTO notes
           (vault_path, title, tags, aliases, content, frontmatter,
            mtime_ms, size_bytes, hash, docid, active)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
        params![
            params.vault_path,
            params.title,
            tags_json,
            aliases_json,
            params.content,
            frontmatter_json,
            params.mtime_ms,
            params.size_bytes,
            file_hash,
            docid,
        ],
    )
    .map_err(|source| TalonError::Sqlite {
        context: "insert note",
        source,
    })?;
    let note_id: i64 = conn
        .query_row(
            "SELECT id FROM notes WHERE vault_path = ?",
            [params.vault_path],
            |row| row.get(0),
        )
        .map_err(|source| TalonError::Sqlite {
            context: "fetch newly inserted note id",
            source,
        })?;
    Ok(NoteUpsertResult {
        note_id,
        is_new: true,
    })
}
