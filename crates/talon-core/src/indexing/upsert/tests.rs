use std::collections::BTreeMap;
use std::env::temp_dir;
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::params;

use super::*;
use crate::indexer::hash_file_content;
use crate::store::open_database;
use crate::text::frontmatter::FrontmatterValue;

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

    let tags: (i64, String) = conn
        .query_row(
            "SELECT COUNT(*), MIN(value_type) FROM note_frontmatter_fields WHERE note_id = ? AND field = ?",
            params![n.note_id, "tags"],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let status: (String, String) = conn
        .query_row(
            "SELECT value, value_type FROM note_frontmatter_fields WHERE note_id = ? AND field = ?",
            params![n.note_id, "status"],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(tags, (2, "list".to_string()));
    assert_eq!(status, ("draft".to_string(), "string".to_string()));
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
