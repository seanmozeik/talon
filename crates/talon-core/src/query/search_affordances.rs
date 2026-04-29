use rusqlite::Connection;

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

pub(super) fn query_citations(conn: &Connection, note_id: i64) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT DISTINCT value FROM note_frontmatter_fields
         WHERE note_id = ?1 AND field = 'sources'
         ORDER BY value
         LIMIT ?2",
    ) else {
        return Vec::new();
    };
    stmt.query_map((note_id, SEARCH_CONTEXT_LIMIT), |row| row.get(0))
        .and_then(Iterator::collect)
        .unwrap_or_default()
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
