use rusqlite::{Connection, OptionalExtension};
use std::collections::HashSet;

const INDEX_PAGE_MULTIPLIER: f64 = 1.05;
const SEARCH_CONTEXT_LIMIT: u32 = 5;

pub(super) fn apply_index_page_preference(results: &mut [super::search::ScoredRawSearchResult]) {
    for result in results {
        if is_index_page(&result.raw.path) {
            result.raw.score *= INDEX_PAGE_MULTIPLIER;
        }
    }
}

pub(super) fn is_index_page(path: &str) -> bool {
    let Some(file_name) = std::path::Path::new(path)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
    else {
        return false;
    };
    let name = file_name.to_ascii_lowercase();
    matches!(name.as_str(), "index.md" | "readme.md") || name.ends_with("_index.md")
}

pub(super) fn query_citations(conn: &Connection, note_id: i64, vault_path: &str) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT value FROM note_frontmatter_fields
         WHERE note_id = ?1 AND field = 'sources'
         ORDER BY rowid",
    ) else {
        return Vec::new();
    };
    let Ok(values) = stmt.query_map([note_id], |row| row.get::<_, String>(0)) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut citations = Vec::new();
    for value in values.flatten() {
        let citation = resolve_source_citation(conn, vault_path, &value)
            .unwrap_or_else(|| clean_obsidian_reference(&value));
        if !citation.is_empty() && seen.insert(citation.clone()) {
            citations.push(citation);
        }
        if citations.len() >= SEARCH_CONTEXT_LIMIT as usize {
            break;
        }
    }
    citations
}

pub(super) fn query_backlinks(conn: &Connection, vault_path: &str) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT DISTINCT from_path FROM links
         WHERE to_path = ?1
         ORDER BY from_path
         LIMIT ?2",
    ) else {
        return Vec::new();
    };
    stmt.query_map((vault_path, SEARCH_CONTEXT_LIMIT), |row| row.get(0))
        .and_then(Iterator::collect)
        .unwrap_or_default()
}

pub(super) fn query_outgoing_links(conn: &Connection, vault_path: &str) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT DISTINCT to_path FROM links
         WHERE from_path = ?1
         ORDER BY to_path
         LIMIT ?2",
    ) else {
        return Vec::new();
    };
    stmt.query_map((vault_path, SEARCH_CONTEXT_LIMIT), |row| row.get(0))
        .and_then(Iterator::collect)
        .unwrap_or_default()
}

fn resolve_source_citation(conn: &Connection, from_path: &str, value: &str) -> Option<String> {
    let target = clean_obsidian_reference(value);
    resolve_linked_source(conn, from_path, value, &target)
        .or_else(|| resolve_active_note(conn, value, &target))
}

fn resolve_linked_source(
    conn: &Connection,
    from_path: &str,
    value: &str,
    target: &str,
) -> Option<String> {
    conn.query_row(
        "SELECT to_path FROM links
         WHERE from_path = ?1
           AND (raw_target = ?2 OR raw_target = ?3 OR to_path = ?2 OR to_path = ?3)
         ORDER BY to_path
         LIMIT 1",
        (from_path, value, target),
        |row| row.get(0),
    )
    .optional()
    .ok()
    .flatten()
}

fn resolve_active_note(conn: &Connection, value: &str, target: &str) -> Option<String> {
    let value_with_ext = with_markdown_extension(value);
    let target_with_ext = with_markdown_extension(target);
    conn.query_row(
        "SELECT vault_path FROM notes
         WHERE active = 1
           AND (
             vault_path = ?1
             OR vault_path = ?2
             OR vault_path = ?3
             OR vault_path = ?4
             OR title = ?1
             OR title = ?3
           )
         ORDER BY
           CASE
             WHEN vault_path = ?1 THEN 0
             WHEN vault_path = ?3 THEN 1
             WHEN vault_path = ?2 THEN 2
             WHEN vault_path = ?4 THEN 3
             ELSE 4
           END,
           vault_path
         LIMIT 1",
        (
            value,
            value_with_ext.as_str(),
            target,
            target_with_ext.as_str(),
        ),
        |row| row.get(0),
    )
    .optional()
    .ok()
    .flatten()
}

fn clean_obsidian_reference(value: &str) -> String {
    let trimmed = value.trim();
    let link_target = trimmed
        .strip_prefix("[[")
        .and_then(|inner| inner.strip_suffix("]]"))
        .unwrap_or(trimmed);
    let without_alias = link_target
        .split_once('|')
        .map_or(link_target, |(target, _alias)| target);
    let target = without_alias
        .split_once('#')
        .map_or(without_alias, |(target, _heading)| target)
        .trim();
    target.to_string()
}

fn with_markdown_extension(value: &str) -> String {
    if std::path::Path::new(value)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
    {
        value.to_string()
    } else {
        format!("{value}.md")
    }
}
