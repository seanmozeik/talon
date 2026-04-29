//! Real read handler for the Talon CLI.
//!
//! Reads a note from the index, strips frontmatter unless `--raw`, applies
//! line-range slicing, and hydrates links / backlinks / tags / aliases from
//! the relational tables.

mod target;

use std::path::Path;

use rusqlite::{Connection, params};

use crate::contracts::{PositiveCount, VaultPath};
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
            vault: None,
            results: Vec::new(),
        };
    };

    let read_target = target::resolve_read_target(conn, path);
    let result = build_read_result(conn, vault_root, &read_target, input);

    ReadResponse {
        vault: None,
        results: vec![result],
    }
}

fn build_read_result(
    conn: &Connection,
    vault_root: &Path,
    read_target: &target::ReadTarget,
    input: &ReadInput,
) -> ReadResult {
    let Ok(vault_path) = VaultPath::parse(&read_target.vault_path) else {
        return not_found_result(&read_target.vault_path, vault_root);
    };

    let note: Option<NoteRow> = conn
        .query_row(
            "SELECT id, title, content FROM notes WHERE vault_path = ? AND active = 1",
            params![&read_target.vault_path],
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
            title: None,
            content: None,
            section: None,
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

    let (body, section) = if let Some(heading) = read_target.heading.as_deref() {
        let Some(slice) = target::find_heading_section(&body, &read_target.vault_path, heading)
        else {
            return ReadResult {
                found: false,
                vault_path,
                title: note.title,
                content: None,
                section: None,
                links: query_outgoing_links(conn, &read_target.vault_path),
                backlinks: query_backlinks(conn, &read_target.vault_path),
                tags: query_tags(conn, note.id),
                aliases: query_aliases(conn, note.id),
            };
        };
        (slice.content, Some(slice.section))
    } else {
        (body, None)
    };

    let content = apply_line_slice(&body, input.from_line, input.max_lines);

    ReadResult {
        found: true,
        vault_path,
        title: note.title,
        content: Some(content),
        section,
        links: query_outgoing_links(conn, &read_target.vault_path),
        backlinks: query_backlinks(conn, &read_target.vault_path),
        tags: query_tags(conn, note.id),
        aliases: query_aliases(conn, note.id),
    }
}

fn not_found_result(vault_path_str: &str, _vault_root: &Path) -> ReadResult {
    let vault_path = VaultPath::parse(vault_path_str)
        .unwrap_or_else(|_| VaultPath::parse("_").unwrap_or_else(|_| unreachable!()));
    ReadResult {
        found: false,
        vault_path,
        title: None,
        content: None,
        section: None,
        links: Vec::new(),
        backlinks: Vec::new(),
        tags: Vec::new(),
        aliases: Vec::new(),
    }
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
        .and_then(Iterator::collect)
        .unwrap_or_default()
}

fn query_backlinks(conn: &Connection, vault_path: &str) -> Vec<String> {
    let Ok(mut stmt) =
        conn.prepare("SELECT DISTINCT from_path FROM links WHERE to_path = ? ORDER BY from_path")
    else {
        return Vec::new();
    };
    stmt.query_map(params![vault_path], |row| row.get(0))
        .and_then(Iterator::collect)
        .unwrap_or_default()
}

fn query_tags(conn: &Connection, note_id: i64) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare("SELECT tag FROM note_tags WHERE note_id = ? ORDER BY tag")
    else {
        return Vec::new();
    };
    stmt.query_map(params![note_id], |row| row.get(0))
        .and_then(Iterator::collect)
        .unwrap_or_default()
}

fn query_aliases(conn: &Connection, note_id: i64) -> Vec<String> {
    let Ok(mut stmt) =
        conn.prepare("SELECT alias FROM note_aliases WHERE note_id = ? ORDER BY alias")
    else {
        return Vec::new();
    };
    stmt.query_map(params![note_id], |row| row.get(0))
        .and_then(Iterator::collect)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
