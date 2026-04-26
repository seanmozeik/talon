//! Meta (frontmatter query) handler for the Talon CLI.
//!
//! Queries active notes by frontmatter `--where` clauses, projects `--select`
//! fields, aggregates `--tag-counts`, and resolves `--sources` reverse
//! references — all against the `SQLite` index.

use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::frontmatter::FrontmatterValue;
use crate::tool::{MetaEntry, MetaInput, MetaResponse, VaultPath};

struct NoteRow {
    note_id: i64,
    vault_path: String,
    frontmatter_json: String,
    mtime_ms: i64,
}

/// Queries active notes by the filters in `input` and returns a [`MetaResponse`].
pub fn query_meta(conn: &Connection, input: &MetaInput) -> MetaResponse {
    let tag_counts = if input.tag_counts {
        Some(query_tag_counts(conn))
    } else {
        None
    };

    let mut notes = input.sources.as_ref().map_or_else(
        || query_all_active_notes(conn),
        |target| query_notes_by_sources(conn, target),
    );

    if let Some(ref since_str) = input.since
        && let Ok(ts) = crate::change_tracking::parse_since(since_str)
    {
        notes.retain(|n| n.mtime_ms >= ts.cast_signed());
    }

    if !input.where_.is_empty() {
        notes.retain(|n| super::where_filter::passes_where_clauses(conn, n.note_id, &input.where_));
    }

    if !input.scope_only.is_empty() {
        notes.retain(|n| passes_scope_filter(&n.vault_path, &input.scope_only));
    }

    let limit = input.limit.get() as usize;
    notes.truncate(limit);

    let entries = notes
        .into_iter()
        .filter_map(|n| build_meta_entry(&n.vault_path, &n.frontmatter_json, &input.select))
        .collect();

    MetaResponse {
        entries,
        tag_counts,
    }
}

fn query_tag_counts(conn: &Connection) -> BTreeMap<String, u32> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT nt.tag, COUNT(*) FROM note_tags nt \
         JOIN notes n ON nt.note_id = n.id \
         WHERE n.active = 1 \
         GROUP BY nt.tag \
         ORDER BY nt.tag",
    ) else {
        return BTreeMap::new();
    };
    stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
    })
    .map(|rows| rows.filter_map(Result::ok).collect())
    .unwrap_or_default()
}

fn query_all_active_notes(conn: &Connection) -> Vec<NoteRow> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT id, vault_path, frontmatter, mtime_ms \
         FROM notes WHERE active = 1 ORDER BY vault_path",
    ) else {
        return Vec::new();
    };
    stmt.query_map([], |row| {
        Ok(NoteRow {
            note_id: row.get(0)?,
            vault_path: row.get(1)?,
            frontmatter_json: row.get(2)?,
            mtime_ms: row.get(3)?,
        })
    })
    .map(|rows| rows.filter_map(Result::ok).collect())
    .unwrap_or_default()
}

fn query_notes_by_sources(conn: &Connection, target: &str) -> Vec<NoteRow> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT DISTINCT n.id, n.vault_path, n.frontmatter, n.mtime_ms \
         FROM notes n \
         JOIN note_frontmatter_fields nff ON n.id = nff.note_id \
         WHERE n.active = 1 AND nff.field = 'sources' AND nff.value = ? \
         ORDER BY n.vault_path",
    ) else {
        return Vec::new();
    };
    stmt.query_map(params![target], |row| {
        Ok(NoteRow {
            note_id: row.get(0)?,
            vault_path: row.get(1)?,
            frontmatter_json: row.get(2)?,
            mtime_ms: row.get(3)?,
        })
    })
    .map(|rows| rows.filter_map(Result::ok).collect())
    .unwrap_or_default()
}

fn passes_scope_filter(path: &str, scope_only: &[String]) -> bool {
    if scope_only.is_empty() {
        return true;
    }
    scope_only.iter().any(|s| path.starts_with(s.as_str()))
}

fn build_meta_entry(
    vault_path: &str,
    frontmatter_json: &str,
    select: &[String],
) -> Option<MetaEntry> {
    let path = VaultPath::parse(vault_path).ok()?;

    let fm_raw: BTreeMap<String, FrontmatterValue> = if frontmatter_json.is_empty() {
        BTreeMap::new()
    } else {
        serde_json::from_str(frontmatter_json).unwrap_or_default()
    };

    let frontmatter: BTreeMap<String, serde_json::Value> = fm_raw
        .into_iter()
        .filter(|(k, _)| select.is_empty() || select.contains(k))
        .map(|(k, v)| (k, fm_value_to_json(v)))
        .collect();

    Some(MetaEntry { path, frontmatter })
}

fn fm_value_to_json(v: FrontmatterValue) -> serde_json::Value {
    use serde_json::Value;
    match v {
        FrontmatterValue::String(s) | FrontmatterValue::Date(s) => Value::String(s),
        FrontmatterValue::Number(n) => {
            serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
        }
        FrontmatterValue::Boolean(b) => Value::Bool(b),
        FrontmatterValue::List(items) => {
            Value::Array(items.into_iter().map(Value::String).collect())
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use rusqlite::{Connection, params};

    use super::*;
    use crate::migrations::run_migrations;
    use crate::tool::{MetaInput, WhereClause, WhereOperator};

    fn fresh_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn insert_note_with_fm(
        conn: &Connection,
        vault_path: &str,
        frontmatter_json: &str,
        mtime_ms: i64,
    ) -> i64 {
        conn.execute(
            "INSERT INTO notes \
             (vault_path, title, tags, aliases, content, frontmatter, \
              mtime_ms, size_bytes, hash, docid, active) \
             VALUES (?, '', '[]', '[]', '', ?, ?, 0, 'h', 'd', 1)",
            params![vault_path, frontmatter_json, mtime_ms],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn insert_fm_field(conn: &Connection, note_id: i64, field: &str, value: &str) {
        conn.execute(
            "INSERT INTO note_frontmatter_fields \
             (note_id, field, value, value_norm) VALUES (?, ?, ?, ?)",
            params![note_id, field, value, value.to_lowercase()],
        )
        .unwrap();
    }

    fn insert_tag(conn: &Connection, note_id: i64, tag: &str) {
        conn.execute(
            "INSERT INTO note_tags (note_id, tag, tag_norm) VALUES (?, ?, ?)",
            params![note_id, tag, tag.to_lowercase()],
        )
        .unwrap();
    }

    // ── Test 1: tag counts ─────────────────────���──────────────────────────────

    #[test]
    fn tag_counts_aggregates_by_tag() {
        let conn = fresh_db();
        let n1 = insert_note_with_fm(&conn, "a.md", "{}", 0);
        let n2 = insert_note_with_fm(&conn, "b.md", "{}", 0);
        insert_tag(&conn, n1, "rust");
        insert_tag(&conn, n1, "programming");
        insert_tag(&conn, n2, "rust");
        insert_tag(&conn, n2, "algorithms");

        let resp = query_meta(
            &conn,
            &MetaInput {
                tag_counts: true,
                ..MetaInput::default()
            },
        );

        let tc = resp
            .tag_counts
            .expect("tag_counts must be Some when requested");
        assert_eq!(tc.get("rust"), Some(&2));
        assert_eq!(tc.get("programming"), Some(&1));
        assert_eq!(tc.get("algorithms"), Some(&1));
    }

    // ── Test 2: where equals ──────────────────────────────────────────────────

    #[test]
    fn where_equals_filters_notes() {
        let conn = fresh_db();
        let n1 = insert_note_with_fm(&conn, "project.md", "{}", 0);
        let n2 = insert_note_with_fm(&conn, "note.md", "{}", 0);
        insert_fm_field(&conn, n1, "type", "project");
        insert_fm_field(&conn, n2, "type", "note");

        let resp = query_meta(
            &conn,
            &MetaInput {
                where_: vec![WhereClause {
                    key: "type".into(),
                    op: WhereOperator::Equals,
                    value: Some("project".into()),
                }],
                ..MetaInput::default()
            },
        );

        assert_eq!(resp.entries.len(), 1);
        assert_eq!(resp.entries[0].path.as_str(), "project.md");
    }

    // ── Test 3: where exists ──────────────────────────────────────────────────

    #[test]
    fn where_exists_returns_notes_with_field() {
        let conn = fresh_db();
        let n1 = insert_note_with_fm(&conn, "done.md", "{}", 0);
        insert_note_with_fm(&conn, "empty.md", "{}", 0);
        insert_fm_field(&conn, n1, "status", "done");

        let resp = query_meta(
            &conn,
            &MetaInput {
                where_: vec![WhereClause {
                    key: "status".into(),
                    op: WhereOperator::Exists,
                    value: None,
                }],
                ..MetaInput::default()
            },
        );

        assert_eq!(resp.entries.len(), 1);
        assert_eq!(resp.entries[0].path.as_str(), "done.md");
    }

    // ── Test 4: where contains ────────────────────────────────────────────────

    #[test]
    fn where_contains_matches_substring() {
        let conn = fresh_db();
        let n1 = insert_note_with_fm(&conn, "rust-notes.md", "{}", 0);
        let n2 = insert_note_with_fm(&conn, "python-notes.md", "{}", 0);
        insert_fm_field(&conn, n1, "description", "rust programming language");
        insert_fm_field(&conn, n2, "description", "python scripting");

        let resp = query_meta(
            &conn,
            &MetaInput {
                where_: vec![WhereClause {
                    key: "description".into(),
                    op: WhereOperator::Contains,
                    value: Some("rust".into()),
                }],
                ..MetaInput::default()
            },
        );

        assert_eq!(resp.entries.len(), 1);
        assert_eq!(resp.entries[0].path.as_str(), "rust-notes.md");
    }

    // ── Test 5: sources lookup ────────────────────────────────────────────────

    #[test]
    fn sources_returns_notes_referencing_target() {
        let conn = fresh_db();
        let n1 = insert_note_with_fm(&conn, "referencing.md", "{}", 0);
        let n2 = insert_note_with_fm(&conn, "unrelated.md", "{}", 0);
        insert_fm_field(&conn, n1, "sources", "target.md");
        insert_fm_field(&conn, n2, "sources", "other.md");

        let resp = query_meta(
            &conn,
            &MetaInput {
                sources: Some("target.md".into()),
                ..MetaInput::default()
            },
        );

        assert_eq!(resp.entries.len(), 1);
        assert_eq!(resp.entries[0].path.as_str(), "referencing.md");
    }

    // ── Test 6: since filter ──────────────────────────────────────────────────

    #[test]
    fn since_filter_excludes_old_notes() {
        let conn = fresh_db();
        insert_note_with_fm(&conn, "old.md", "{}", 1000);
        insert_note_with_fm(&conn, "new.md", "{}", 3000);

        let resp = query_meta(
            &conn,
            &MetaInput {
                since: Some("2000".into()),
                ..MetaInput::default()
            },
        );

        assert_eq!(resp.entries.len(), 1);
        assert_eq!(resp.entries[0].path.as_str(), "new.md");
    }

    // ── Test 7: scope_only filter ─────────────────────────────────────────────

    #[test]
    fn scope_only_filters_by_prefix() {
        let conn = fresh_db();
        insert_note_with_fm(&conn, "Atlas/note.md", "{}", 0);
        insert_note_with_fm(&conn, "Search/note.md", "{}", 0);

        let resp = query_meta(
            &conn,
            &MetaInput {
                scope_only: vec!["Atlas".into()],
                ..MetaInput::default()
            },
        );

        assert_eq!(resp.entries.len(), 1);
        assert_eq!(resp.entries[0].path.as_str(), "Atlas/note.md");
    }

    // ── Test 8: select projects fields ────────────────────────────────────────

    #[test]
    fn select_projects_only_requested_fields() {
        let conn = fresh_db();
        let fm = r#"{"type":{"String":"project"},"status":{"String":"active"},"priority":{"Number":1.0}}"#;
        insert_note_with_fm(&conn, "proj.md", fm, 0);

        let resp = query_meta(
            &conn,
            &MetaInput {
                select: vec!["type".into()],
                ..MetaInput::default()
            },
        );

        assert_eq!(resp.entries.len(), 1);
        let fm = &resp.entries[0].frontmatter;
        assert!(fm.contains_key("type"), "selected field must be present");
        assert!(
            !fm.contains_key("status"),
            "non-selected field must be absent"
        );
        assert!(
            !fm.contains_key("priority"),
            "non-selected field must be absent"
        );
    }
}
