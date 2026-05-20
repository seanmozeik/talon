//! Sync finalization step: re-resolve previously-unresolved links against
//! the current note set.
//!
//! Closes a staleness window in incremental sync. When a target file gains
//! an alias (or a new note is created) after some other source file already
//! linked to that target, the source file's `links.to_path` row stays as
//! the unresolved raw target until the source itself is re-indexed. Without
//! this pass the user has to `touch` source files to "see" newly satisfied
//! links — a confusing UX for an Obsidian-style vault where alias additions
//! are a natural fix path.
//!
//! Cost: one SELECT to find unresolved rows, one in-memory note set load,
//! and at most N small UPDATEs where N is the unresolved count. Cheap on
//! healthy vaults; bounded on broken ones.

use rusqlite::{Connection, params};

use crate::error::TalonError;
use crate::indexer::load_notes_for_linking;
use crate::links::LinkResolver;

/// Walks unresolved links and tries to resolve each against the current
/// note set. Returns the number of rows updated.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] if the SELECT or UPDATE statements fail.
pub fn relink_unresolved(conn: &Connection) -> Result<u32, TalonError> {
    let unresolved = collect_unresolved(conn)?;
    if unresolved.is_empty() {
        return Ok(0);
    }

    let notes = load_notes_for_linking(conn).map_err(|e| TalonError::Sqlite {
        context: "relink: load notes",
        source: e,
    })?;

    let resolver = LinkResolver::new(&notes);
    let mut updated = 0_u32;
    for (from_path, raw_target, old_to_path) in unresolved {
        let Some(new_to_path) = resolver.resolve(&raw_target) else {
            continue;
        };
        if new_to_path == old_to_path {
            continue;
        }
        conn.execute(
            "UPDATE OR IGNORE links \
             SET to_path = ?1 \
             WHERE from_path = ?2 AND raw_target = ?3 AND to_path = ?4",
            params![new_to_path, from_path, raw_target, old_to_path],
        )
        .map_err(|e| TalonError::Sqlite {
            context: "relink: update links",
            source: e,
        })?;
        updated = updated.saturating_add(1);
    }
    Ok(updated)
}

fn collect_unresolved(conn: &Connection) -> Result<Vec<(String, String, String)>, TalonError> {
    let mut stmt = conn
        .prepare(
            "SELECT from_path, raw_target, to_path FROM links \
             WHERE to_path NOT IN (SELECT vault_path FROM notes WHERE active = 1)",
        )
        .map_err(|e| TalonError::Sqlite {
            context: "relink: prepare select",
            source: e,
        })?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| TalonError::Sqlite {
            context: "relink: query_map",
            source: e,
        })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| TalonError::Sqlite {
            context: "relink: collect",
            source: e,
        })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::indexing::migrations::run_migrations;
    use rusqlite::Connection;

    fn fresh_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn insert_note(conn: &Connection, path: &str, title: &str, aliases_json: &str) {
        conn.execute(
            "INSERT INTO notes \
             (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active) \
             VALUES (?, ?, '[]', ?, '', 0, 0, 'h', ?, 1)",
            params![path, title, aliases_json, path],
        )
        .unwrap();
    }

    fn insert_link(conn: &Connection, from: &str, to: &str, raw: &str) {
        conn.execute(
            "INSERT OR IGNORE INTO links (from_path, to_path, raw_target) VALUES (?, ?, ?)",
            params![from, to, raw],
        )
        .unwrap();
    }

    fn link_to_path(conn: &Connection, from: &str, raw: &str) -> String {
        conn.query_row(
            "SELECT to_path FROM links WHERE from_path = ?1 AND raw_target = ?2",
            params![from, raw],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
    }

    #[test]
    fn relink_resolves_alias_added_after_source_was_indexed() {
        // Set up: source A links to "Charred Spring Onion" (raw + to both),
        // target file lives at "Dish - Charred Spring Onion.md" with an alias
        // matching the bare wikilink. Before relink, the link's to_path is
        // the unresolved raw target.
        let conn = fresh_db();
        insert_note(&conn, "A.md", "A", "[]");
        insert_note(
            &conn,
            "Dish - Charred Spring Onion.md",
            "Dish - Charred Spring Onion",
            r#"["Charred Spring Onion"]"#,
        );
        insert_link(
            &conn,
            "A.md",
            "Charred Spring Onion",
            "Charred Spring Onion",
        );

        assert_eq!(
            link_to_path(&conn, "A.md", "Charred Spring Onion"),
            "Charred Spring Onion",
            "precondition: link unresolved"
        );

        let updated = relink_unresolved(&conn).expect("relink");
        assert_eq!(updated, 1, "should re-resolve one row");
        assert_eq!(
            link_to_path(&conn, "A.md", "Charred Spring Onion"),
            "Dish - Charred Spring Onion.md",
            "to_path now points at the alias-matching note"
        );
    }

    #[test]
    fn relink_no_op_when_target_still_missing() {
        let conn = fresh_db();
        insert_note(&conn, "A.md", "A", "[]");
        insert_link(&conn, "A.md", "Genuinely Missing", "Genuinely Missing");

        let updated = relink_unresolved(&conn).expect("relink");
        assert_eq!(updated, 0);
        assert_eq!(
            link_to_path(&conn, "A.md", "Genuinely Missing"),
            "Genuinely Missing",
        );
    }

    #[test]
    fn relink_skips_already_resolved() {
        let conn = fresh_db();
        insert_note(&conn, "A.md", "A", "[]");
        insert_note(&conn, "B.md", "B", "[]");
        insert_link(&conn, "A.md", "B.md", "B");

        let updated = relink_unresolved(&conn).expect("relink");
        assert_eq!(updated, 0, "resolved rows are skipped");
    }
}
