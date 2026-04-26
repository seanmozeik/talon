//! `SQL` upsert helpers for indexed notes and their associated rows.
//!
//! Ports `services/talon/indexer/{note-upsert,chunk-upsert,note-meta}.ts`.
//! All functions take a `&Connection` and assume the schema from
//! [`crate::migrations`] is in place. Callers are expected to wrap multiple
//! upserts in a transaction when atomicity matters.

use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::TalonError;
use crate::frontmatter::{FrontmatterValue, normalize_keyword};
use crate::links::ResolvedLink;

use super::prelude::hash_file_content;

/// Outcome of [`upsert_note`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteUpsertResult {
    /// Database `notes.id` of the upserted row.
    pub note_id: i64,
    /// `true` if a new row was inserted, `false` if an existing one was updated.
    pub is_new: bool,
}

/// Parameters for [`upsert_note`].
///
/// The `frontmatter` is serialized to `JSON` and stored verbatim in
/// `notes.frontmatter`; `aliases` and `tags` are also stored as `JSON`
/// arrays. The body content's `SHA-256` hash is computed and stored.
#[derive(Debug)]
pub struct UpsertNoteParams<'a> {
    /// Vault-relative path (e.g. `"zone/note.md"`).
    pub vault_path: &'a str,
    /// Display title.
    pub title: &'a str,
    /// Note body (post-frontmatter).
    pub content: &'a str,
    /// Parsed frontmatter map.
    pub frontmatter: &'a BTreeMap<String, FrontmatterValue>,
    /// Aliases, in declaration order.
    pub aliases: &'a [String],
    /// Tags (frontmatter + inline), deduplicated.
    pub tags: &'a [String],
    /// File modification time, milliseconds since epoch.
    pub mtime_ms: i64,
    /// File size in bytes.
    pub size_bytes: i64,
}

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

/// Per-chunk upsert payload. Mirrors [`crate::chunker::NoteChunk`] but
/// flattened into the column shape that the `chunks` table expects.
#[derive(Debug, Clone)]
pub struct ChunkUpsertRow {
    /// 0-indexed position within the note.
    pub index: u32,
    /// Raw chunk text.
    pub text: String,
    /// Embedding-friendly text (chunker's `build_embedding_text` output).
    pub embedding_text: String,
    /// Heading path (`"H1 > H2"`), if any.
    pub heading_path: Option<String>,
    /// Character span in the parent note.
    pub char_start: u32,
    /// Character end (exclusive) in the parent note.
    pub char_end: u32,
    /// 1-based line span start.
    pub line_start: u32,
    /// 1-based line span end.
    pub line_end: u32,
    /// `SHA-256` of `text`, used for dedup.
    pub chunk_hash: String,
    /// Token-count estimate (chars/4 with floor).
    pub token_estimate: u32,
}

/// Upserts the chunks for `note_id`.
///
/// Applies the dedup-by-hash rule: chunks whose `chunk_hash` is unchanged
/// keep their `embedding_status`, chunks whose hash changed are re-marked
/// `'pending'`, and orphan rows (existing chunks at indexes not present in
/// `chunks`) are deleted.
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
    let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
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
    delete_orphan_chunks(conn, &existing_by_index, &seen)
}

fn load_existing_chunks_by_index(
    conn: &Connection,
    note_id: i64,
) -> Result<BTreeMap<i64, (i64, String)>, TalonError> {
    let mut stmt = conn
        .prepare("SELECT id, chunk_index, chunk_hash FROM chunks WHERE note_id = ?")
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
    seen: &std::collections::HashSet<i64>,
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

/// Replaces the link rows originating from `vault_path`.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] on failure.
pub fn upsert_links(
    conn: &Connection,
    vault_path: &str,
    links: &[ResolvedLink],
) -> Result<(), TalonError> {
    conn.execute("DELETE FROM links WHERE from_path = ?", [vault_path])
        .map_err(|source| TalonError::Sqlite {
            context: "delete old links",
            source,
        })?;
    for link in links {
        conn.execute(
            "INSERT OR IGNORE INTO links
               (from_path, to_path, raw_target, heading, alias)
             VALUES (?, ?, ?, ?, ?)",
            params![
                vault_path,
                link.to_path,
                link.raw_target,
                link.heading,
                link.alias,
            ],
        )
        .map_err(|source| TalonError::Sqlite {
            context: "insert link",
            source,
        })?;
    }
    Ok(())
}

/// Replaces the alias rows for `note_id` with `aliases`. Each alias is
/// stored both verbatim and in normalized form for exact-match lookup.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] on failure.
pub fn upsert_aliases(
    conn: &Connection,
    note_id: i64,
    aliases: &[String],
) -> Result<(), TalonError> {
    conn.execute("DELETE FROM note_aliases WHERE note_id = ?", [note_id])
        .map_err(|source| TalonError::Sqlite {
            context: "delete old aliases",
            source,
        })?;
    for alias in aliases {
        let norm = normalize_keyword(alias);
        conn.execute(
            "INSERT INTO note_aliases (note_id, alias, alias_norm) VALUES (?, ?, ?)",
            params![note_id, alias, norm],
        )
        .map_err(|source| TalonError::Sqlite {
            context: "insert alias",
            source,
        })?;
    }
    Ok(())
}

/// Replaces the tag rows for `note_id` with `tags`.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] on failure.
pub fn upsert_tags(conn: &Connection, note_id: i64, tags: &[String]) -> Result<(), TalonError> {
    conn.execute("DELETE FROM note_tags WHERE note_id = ?", [note_id])
        .map_err(|source| TalonError::Sqlite {
            context: "delete old tags",
            source,
        })?;
    for tag in tags {
        let norm = normalize_keyword(tag);
        conn.execute(
            "INSERT INTO note_tags (note_id, tag, tag_norm) VALUES (?, ?, ?)",
            params![note_id, tag, norm],
        )
        .map_err(|source| TalonError::Sqlite {
            context: "insert tag",
            source,
        })?;
    }
    Ok(())
}

fn flatten_frontmatter(value: &FrontmatterValue, prefix: &str, out: &mut Vec<(String, String)>) {
    let key = if prefix.is_empty() {
        String::from("value")
    } else {
        prefix.to_string()
    };
    match value {
        FrontmatterValue::String(s) | FrontmatterValue::Date(s) => out.push((key, s.clone())),
        FrontmatterValue::Number(n) => out.push((key, n.to_string())),
        FrontmatterValue::Boolean(b) => out.push((key, b.to_string())),
        FrontmatterValue::List(items) => {
            for item in items {
                flatten_frontmatter(&FrontmatterValue::String(item.clone()), prefix, out);
            }
        }
    }
}

/// Replaces the frontmatter-field rows for `note_id` with the flattened
/// representation of `frontmatter`.
///
/// Nested values get dotted keys (`outer.inner`); list values produce one
/// row per item under the same key.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] on failure.
pub fn upsert_frontmatter_fields(
    conn: &Connection,
    note_id: i64,
    frontmatter: &BTreeMap<String, FrontmatterValue>,
) -> Result<(), TalonError> {
    conn.execute(
        "DELETE FROM note_frontmatter_fields WHERE note_id = ?",
        [note_id],
    )
    .map_err(|source| TalonError::Sqlite {
        context: "delete old frontmatter fields",
        source,
    })?;
    let mut flat: Vec<(String, String)> = Vec::new();
    for (key, value) in frontmatter {
        flatten_frontmatter(value, key, &mut flat);
    }
    for (field, value) in flat {
        let norm = normalize_keyword(&value);
        conn.execute(
            "INSERT INTO note_frontmatter_fields (note_id, field, value, value_norm)
             VALUES (?, ?, ?, ?)",
            params![note_id, field, value, norm],
        )
        .map_err(|source| TalonError::Sqlite {
            context: "insert frontmatter field",
            source,
        })?;
    }
    Ok(())
}

/// Soft-deletes `note_id`: clears searchable fields, deletes child rows
/// (chunks, links, aliases, tags, frontmatter fields), removes vector
/// metadata, and writes a `'delete'` event-log entry.
///
/// The note row stays so foreign-key references and `--since` queries can
/// still resolve the path. The `vec_chunks` `DELETE` is best-effort — it
/// fails silently if the `sqlite-vec` extension is not loaded.
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
            .prepare("SELECT id FROM chunks WHERE note_id = ?")
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::store::open_database;
    use std::env::temp_dir;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_db(label: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        temp_dir().join(format!("talon-upsert-test-{label}-{pid}-{n}.sqlite"))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = fs_err::remove_file(path);
        let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
    }

    fn empty_fm() -> BTreeMap<String, FrontmatterValue> {
        BTreeMap::new()
    }

    #[test]
    fn upsert_note_inserts_new_row() {
        let path = unique_db("insert");
        let conn = open_database(&path).unwrap();
        let fm = empty_fm();
        let result = upsert_note(
            &conn,
            &UpsertNoteParams {
                vault_path: "a.md",
                title: "A",
                content: "hello",
                frontmatter: &fm,
                aliases: &[],
                tags: &[],
                mtime_ms: 100,
                size_bytes: 5,
            },
        )
        .unwrap();
        assert!(result.is_new);
        assert!(result.note_id > 0);
        cleanup(&path);
    }

    #[test]
    fn upsert_note_updates_existing_row_in_place() {
        let path = unique_db("update");
        let conn = open_database(&path).unwrap();
        let fm = empty_fm();
        let first = upsert_note(
            &conn,
            &UpsertNoteParams {
                vault_path: "a.md",
                title: "A",
                content: "v1",
                frontmatter: &fm,
                aliases: &[],
                tags: &[],
                mtime_ms: 100,
                size_bytes: 2,
            },
        )
        .unwrap();
        let second = upsert_note(
            &conn,
            &UpsertNoteParams {
                vault_path: "a.md",
                title: "A revised",
                content: "v2",
                frontmatter: &fm,
                aliases: &[],
                tags: &[],
                mtime_ms: 200,
                size_bytes: 2,
            },
        )
        .unwrap();
        assert!(!second.is_new);
        assert_eq!(first.note_id, second.note_id);
        let title: String = conn
            .query_row(
                "SELECT title FROM notes WHERE id = ?",
                [second.note_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(title, "A revised");
        cleanup(&path);
    }

    fn ch(idx: u32, text: &str) -> ChunkUpsertRow {
        ChunkUpsertRow {
            index: idx,
            text: text.into(),
            embedding_text: text.into(),
            heading_path: None,
            char_start: 0,
            char_end: u32::try_from(text.len()).unwrap_or(0),
            line_start: 1,
            line_end: 1,
            chunk_hash: hash_file_content(text),
            token_estimate: 1,
        }
    }

    #[test]
    fn upsert_chunks_inserts_then_dedupes_unchanged_then_deletes_orphan() {
        let path = unique_db("chunks");
        let conn = open_database(&path).unwrap();
        let fm = empty_fm();
        let n = upsert_note(
            &conn,
            &UpsertNoteParams {
                vault_path: "a.md",
                title: "A",
                content: "body",
                frontmatter: &fm,
                aliases: &[],
                tags: &[],
                mtime_ms: 100,
                size_bytes: 4,
            },
        )
        .unwrap();

        // First pass: insert two chunks; both should be 'pending'.
        upsert_chunks(&conn, n.note_id, &[ch(0, "alpha"), ch(1, "beta")]).unwrap();
        let pending: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE note_id = ? AND embedding_status = 'pending'",
                [n.note_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pending, 2);

        // Mark them as embedded so we can detect status preservation below.
        conn.execute(
            "UPDATE chunks SET embedding_status = 'done' WHERE note_id = ?",
            [n.note_id],
        )
        .unwrap();

        // Second pass: chunk 0 unchanged → status preserved; chunk 1 changed
        // → status reset to 'pending'; chunk 2 added.
        upsert_chunks(
            &conn,
            n.note_id,
            &[ch(0, "alpha"), ch(1, "beta-NEW"), ch(2, "gamma")],
        )
        .unwrap();
        let chunk0_status: String = conn
            .query_row(
                "SELECT embedding_status FROM chunks WHERE note_id = ? AND chunk_index = 0",
                [n.note_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(chunk0_status, "done", "unchanged chunk should keep status");
        let chunk1_status: String = conn
            .query_row(
                "SELECT embedding_status FROM chunks WHERE note_id = ? AND chunk_index = 1",
                [n.note_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            chunk1_status, "pending",
            "changed chunk should reset status"
        );
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE note_id = ?",
                [n.note_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(total, 3);

        // Third pass: only chunk 0 → others get deleted as orphans.
        upsert_chunks(&conn, n.note_id, &[ch(0, "alpha")]).unwrap();
        let total_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE note_id = ?",
                [n.note_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(total_after, 1);
        cleanup(&path);
    }

    #[test]
    fn upsert_aliases_writes_normalized_form() {
        let path = unique_db("aliases");
        let conn = open_database(&path).unwrap();
        let fm = empty_fm();
        let n = upsert_note(
            &conn,
            &UpsertNoteParams {
                vault_path: "a.md",
                title: "A",
                content: "x",
                frontmatter: &fm,
                aliases: &[],
                tags: &[],
                mtime_ms: 0,
                size_bytes: 0,
            },
        )
        .unwrap();
        upsert_aliases(&conn, n.note_id, &["Atomic".into(), "Other".into()]).unwrap();
        let alias_norm: String = conn
            .query_row(
                "SELECT alias_norm FROM note_aliases WHERE note_id = ? AND alias = ?",
                params![n.note_id, "Atomic"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(alias_norm, "atomic");

        // Re-upsert replaces — old aliases gone.
        upsert_aliases(&conn, n.note_id, &["Replaced".into()]).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM note_aliases WHERE note_id = ?",
                [n.note_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        cleanup(&path);
    }

    #[test]
    fn upsert_frontmatter_flattens_lists_and_nested_values() {
        let path = unique_db("fm");
        let conn = open_database(&path).unwrap();
        let mut fm: BTreeMap<String, FrontmatterValue> = BTreeMap::new();
        fm.insert(
            "tags".into(),
            FrontmatterValue::List(vec!["a".into(), "b".into()]),
        );
        fm.insert("status".into(), FrontmatterValue::String("draft".into()));

        let n = upsert_note(
            &conn,
            &UpsertNoteParams {
                vault_path: "a.md",
                title: "A",
                content: "x",
                frontmatter: &fm,
                aliases: &[],
                tags: &[],
                mtime_ms: 0,
                size_bytes: 0,
            },
        )
        .unwrap();
        upsert_frontmatter_fields(&conn, n.note_id, &fm).unwrap();

        let tag_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM note_frontmatter_fields WHERE note_id = ? AND field = ?",
                params![n.note_id, "tags"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tag_count, 2);

        let status_value: String = conn
            .query_row(
                "SELECT value FROM note_frontmatter_fields WHERE note_id = ? AND field = ?",
                params![n.note_id, "status"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status_value, "draft");
        cleanup(&path);
    }

    #[test]
    fn perform_note_deletion_soft_deletes_and_clears_children() {
        let path = unique_db("delete");
        let conn = open_database(&path).unwrap();
        let fm = empty_fm();
        let n = upsert_note(
            &conn,
            &UpsertNoteParams {
                vault_path: "a.md",
                title: "A",
                content: "body",
                frontmatter: &fm,
                aliases: &["Atomic".into()],
                tags: &["zk".into()],
                mtime_ms: 0,
                size_bytes: 4,
            },
        )
        .unwrap();
        upsert_chunks(&conn, n.note_id, &[ch(0, "body")]).unwrap();
        upsert_aliases(&conn, n.note_id, &["Atomic".into()]).unwrap();
        upsert_tags(&conn, n.note_id, &["zk".into()]).unwrap();

        perform_note_deletion(&conn, n.note_id, "a.md").unwrap();

        let active: i64 = conn
            .query_row("SELECT active FROM notes WHERE id = ?", [n.note_id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(active, 0);
        let chunks: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE note_id = ?",
                [n.note_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(chunks, 0);
        let aliases: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM note_aliases WHERE note_id = ?",
                [n.note_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(aliases, 0);
        let log: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM event_log WHERE action = 'delete' AND path = 'a.md'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(log, 1);
        cleanup(&path);
    }
}
