//! Real read handler for the Talon CLI.
//!
//! Reads a note from the index, strips frontmatter unless `--raw`, applies
//! line-range slicing, and hydrates links / backlinks / tags / aliases from
//! the relational tables.

use std::path::Path;

use rusqlite::{Connection, params};

use crate::contracts::{ContainerPath, PositiveCount, VaultPath};
use crate::query::{ReadInput, ReadResponse, ReadResult};
use crate::text::frontmatter::parse_frontmatter;

struct NoteRow {
    id: i64,
    title: Option<String>,
    content: String,
}

/// Reads a note by vault-relative path and returns a [`ReadResponse`].
///
/// - Frontmatter is stripped unless `input.raw` is true.
/// - `from_line` (1-based) and `max_lines` clip the returned body.
/// - Missing notes return a result with `found: false`.
/// - An empty or missing `input.path` returns an empty result list.
pub fn run_read(conn: &Connection, vault_root: &Path, input: &ReadInput) -> ReadResponse {
    let Some(path) = input.path.as_deref().filter(|p| !p.trim().is_empty()) else {
        return ReadResponse {
            results: Vec::new(),
        };
    };

    ReadResponse {
        results: vec![build_read_result(conn, vault_root, path, input)],
    }
}

fn build_read_result(
    conn: &Connection,
    vault_root: &Path,
    vault_path_str: &str,
    input: &ReadInput,
) -> ReadResult {
    let Ok(vault_path) = VaultPath::parse(vault_path_str) else {
        return not_found_result(vault_path_str, vault_root);
    };

    let note: Option<NoteRow> = conn
        .query_row(
            "SELECT id, title, content FROM notes WHERE vault_path = ? AND active = 1",
            params![vault_path_str],
            |row| {
                Ok(NoteRow {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    content: row.get(2)?,
                })
            },
        )
        .ok();

    let Some(note) = note else {
        return ReadResult {
            found: false,
            vault_path,
            path: build_container_path(vault_root, vault_path_str),
            title: None,
            content: None,
            links: Vec::new(),
            backlinks: Vec::new(),
            tags: Vec::new(),
            aliases: Vec::new(),
        };
    };

    let body = if input.raw {
        note.content.clone()
    } else {
        parse_frontmatter(&note.content).body
    };

    let content = apply_line_slice(&body, input.from_line, input.max_lines);

    ReadResult {
        found: true,
        vault_path,
        path: build_container_path(vault_root, vault_path_str),
        title: note.title,
        content: Some(content),
        links: query_outgoing_links(conn, vault_path_str),
        backlinks: query_backlinks(conn, vault_path_str),
        tags: query_tags(conn, note.id),
        aliases: query_aliases(conn, note.id),
    }
}

fn not_found_result(vault_path_str: &str, vault_root: &Path) -> ReadResult {
    let vault_path = VaultPath::parse(vault_path_str)
        .unwrap_or_else(|_| VaultPath::parse("_").unwrap_or_else(|_| unreachable!()));
    ReadResult {
        found: false,
        vault_path,
        path: build_container_path(vault_root, vault_path_str),
        title: None,
        content: None,
        links: Vec::new(),
        backlinks: Vec::new(),
        tags: Vec::new(),
        aliases: Vec::new(),
    }
}

fn build_container_path(vault_root: &Path, vault_path: &str) -> ContainerPath {
    let full = vault_root.join(vault_path);
    // A non-empty vault_path joined to vault_root always yields a non-empty path.
    ContainerPath::parse(full.to_string_lossy().as_ref())
        .unwrap_or_else(|_| ContainerPath::parse("/").unwrap_or_else(|_| unreachable!()))
}

fn apply_line_slice(
    content: &str,
    from_line: Option<PositiveCount>,
    max_lines: Option<PositiveCount>,
) -> String {
    if from_line.is_none() && max_lines.is_none() {
        return content.to_string();
    }
    let lines: Vec<&str> = content.lines().collect();
    let start = from_line.map_or(0, |n| usize::from(n.get()).saturating_sub(1));
    let slice = lines.get(start..).unwrap_or(&[]);
    let slice = max_lines.map_or(slice, |max| {
        let end = usize::from(max.get()).min(slice.len());
        &slice[..end]
    });
    slice.join("\n")
}

fn query_outgoing_links(conn: &Connection, vault_path: &str) -> Vec<String> {
    let Ok(mut stmt) =
        conn.prepare("SELECT DISTINCT to_path FROM links WHERE from_path = ? ORDER BY to_path")
    else {
        return Vec::new();
    };
    stmt.query_map(params![vault_path], |row| row.get(0))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

fn query_backlinks(conn: &Connection, vault_path: &str) -> Vec<String> {
    let Ok(mut stmt) =
        conn.prepare("SELECT DISTINCT from_path FROM links WHERE to_path = ? ORDER BY from_path")
    else {
        return Vec::new();
    };
    stmt.query_map(params![vault_path], |row| row.get(0))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

fn query_tags(conn: &Connection, note_id: i64) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare("SELECT tag FROM note_tags WHERE note_id = ? ORDER BY tag")
    else {
        return Vec::new();
    };
    stmt.query_map(params![note_id], |row| row.get(0))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

fn query_aliases(conn: &Connection, note_id: i64) -> Vec<String> {
    let Ok(mut stmt) =
        conn.prepare("SELECT alias FROM note_aliases WHERE note_id = ? ORDER BY alias")
    else {
        return Vec::new();
    };
    stmt.query_map(params![note_id], |row| row.get(0))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::path::PathBuf;

    use rusqlite::Connection;

    use super::*;
    use crate::indexing::migrations::run_migrations;

    fn fresh_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn insert_note(conn: &Connection, vault_path: &str, title: &str, content: &str) -> i64 {
        conn.execute(
            "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
             VALUES (?, ?, '[]', '[]', ?, 0, 0, 'h', 'd', 1)",
            params![vault_path, title, content],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn vault_root() -> PathBuf {
        PathBuf::from("/vault")
    }

    fn read_input(path: &str) -> ReadInput {
        ReadInput {
            path: Some(path.to_string()),
            raw: false,
            from_line: None,
            max_lines: None,
        }
    }

    #[test]
    fn strips_frontmatter_by_default() {
        let conn = fresh_db();
        insert_note(
            &conn,
            "notes/example.md",
            "Example",
            "---\ntitle: Example\ntags: [rust]\n---\n\nHello world\n",
        );

        let resp = run_read(&conn, &vault_root(), &read_input("notes/example.md"));
        let result = &resp.results[0];

        assert!(result.found);
        let content = result.content.as_deref().unwrap();
        assert!(
            !content.contains("---"),
            "frontmatter delimiters must be stripped"
        );
        assert!(
            !content.contains("title:"),
            "frontmatter fields must be stripped"
        );
        assert!(content.contains("Hello world"));
    }

    #[test]
    fn raw_mode_preserves_frontmatter() {
        let conn = fresh_db();
        insert_note(
            &conn,
            "notes/raw.md",
            "Raw",
            "---\ntitle: Raw\n---\n\nBody here\n",
        );

        let resp = run_read(
            &conn,
            &vault_root(),
            &ReadInput {
                path: Some("notes/raw.md".to_string()),
                raw: true,
                from_line: None,
                max_lines: None,
            },
        );
        let result = &resp.results[0];
        assert!(result.found);
        let content = result.content.as_deref().unwrap();
        assert!(
            content.contains("---"),
            "raw mode must preserve frontmatter delimiters"
        );
        assert!(content.contains("title: Raw"));
    }

    #[test]
    fn line_range_clips_body() {
        let conn = fresh_db();
        let body = "line1\nline2\nline3\nline4\nline5\n";
        insert_note(&conn, "notes/lines.md", "Lines", body);

        let resp = run_read(
            &conn,
            &vault_root(),
            &ReadInput {
                path: Some("notes/lines.md".to_string()),
                raw: true,
                from_line: Some(PositiveCount::new(2, "from_line").unwrap()),
                max_lines: Some(PositiveCount::new(2, "max_lines").unwrap()),
            },
        );
        let result = &resp.results[0];
        assert!(result.found);
        let content = result.content.as_deref().unwrap();
        assert_eq!(content, "line2\nline3");
    }

    #[test]
    fn missing_note_returns_not_found() {
        let conn = fresh_db();

        let resp = run_read(&conn, &vault_root(), &read_input("does/not/exist.md"));
        assert_eq!(resp.results.len(), 1);
        let result = &resp.results[0];
        assert!(!result.found);
        assert!(result.content.is_none());
    }

    #[test]
    fn links_and_backlinks_hydrated() {
        let conn = fresh_db();
        insert_note(&conn, "a.md", "A", "content");
        insert_note(&conn, "b.md", "B", "content");
        conn.execute(
            "INSERT INTO links (from_path, to_path, raw_target) VALUES (?, ?, ?)",
            params!["a.md", "b.md", "[[b]]"],
        )
        .unwrap();

        let resp = run_read(&conn, &vault_root(), &read_input("a.md"));
        let result = &resp.results[0];
        assert!(result.found);
        assert!(
            result.links.contains(&"b.md".to_string()),
            "outgoing link must be present"
        );

        let resp_b = run_read(&conn, &vault_root(), &read_input("b.md"));
        let result_b = &resp_b.results[0];
        assert!(
            result_b.backlinks.contains(&"a.md".to_string()),
            "backlink must be present"
        );
    }
}
