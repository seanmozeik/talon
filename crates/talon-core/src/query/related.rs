//! Related-notes handler for the Talon CLI.

use std::collections::{HashSet, VecDeque};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::constants::RELATED_MAX_DEPTH;
use crate::contracts::VaultPath;
use crate::search::Direction;

/// Related-note request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedInput {
    /// Path to find related notes for.
    pub path: String,
    /// Graph traversal depth.
    #[serde(default = "default_depth")]
    pub depth: u8,
    /// Traversal direction.
    #[serde(default)]
    pub direction: Direction,
    /// Scope names to include.
    #[serde(default)]
    pub scope: Vec<String>,
    /// Scope names to search exclusively.
    #[serde(default)]
    pub scope_only: Vec<String>,
}

/// Related-note response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedResponse {
    /// Source path.
    pub path: VaultPath,
    /// Direction traversed.
    pub direction: Direction,
    /// Related notes.
    pub results: Vec<RelatedResult>,
}

/// A single related-note result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedResult {
    /// Vault-relative path.
    pub vault_path: VaultPath,
    /// Display title.
    pub title: String,
    /// Link text from source.
    pub link_text: String,
    /// Direction: outgoing or backlink.
    pub relation: RelationKind,
}

/// Relation kind (outgoing vs backlink).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelationKind {
    Outgoing,
    Backlink,
}

const fn default_depth() -> u8 {
    crate::constants::RELATED_DEFAULT_DEPTH
}

/// Traverses the link graph from `input.path` and returns related notes.
///
/// - Depth is clamped to [`RELATED_MAX_DEPTH`] and minimum 1.
/// - Cycles are detected via a visited set — each path appears at most once.
/// - `scope_only` filters results to notes whose vault path starts with any
///   listed prefix. `scope` and `scope_only` both empty means no filtering.
pub fn find_related(conn: &Connection, input: &RelatedInput) -> RelatedResponse {
    let path = input.path.trim();

    let Ok(source_path) = VaultPath::parse(path) else {
        return RelatedResponse {
            path: VaultPath::parse("_").unwrap_or_else(|_| unreachable!()),
            direction: input.direction,
            results: Vec::new(),
        };
    };

    let depth = input.depth.clamp(1, RELATED_MAX_DEPTH);
    let direction = input.direction;

    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(path.to_string());

    // Queue holds (current_path, hops_from_source)
    let mut queue: VecDeque<(String, u8)> = VecDeque::new();
    queue.push_back((path.to_string(), 0));

    let mut results: Vec<RelatedResult> = Vec::new();

    while let Some((current_path, current_depth)) = queue.pop_front() {
        if current_depth >= depth {
            continue;
        }

        let neighbors = collect_neighbors(conn, &current_path, direction);

        for (neighbor_path, link_text, relation) in neighbors {
            if visited.contains(&neighbor_path) {
                continue;
            }
            visited.insert(neighbor_path.clone());

            if !passes_scope_filter(&neighbor_path, &input.scope_only) {
                continue;
            }

            let title = query_title(conn, &neighbor_path).unwrap_or_else(|| neighbor_path.clone());

            let Ok(vault_path) = VaultPath::parse(&neighbor_path) else {
                continue;
            };

            results.push(RelatedResult {
                vault_path,
                title,
                link_text,
                relation,
            });

            queue.push_back((neighbor_path, current_depth + 1));
        }
    }

    RelatedResponse {
        path: source_path,
        direction,
        results,
    }
}

/// Returns `(neighbor_path, link_text, relation)` tuples for a given path.
fn collect_neighbors(
    conn: &Connection,
    path: &str,
    direction: Direction,
) -> Vec<(String, String, RelationKind)> {
    let mut neighbors = Vec::new();
    if direction == Direction::Outgoing || direction == Direction::Both {
        neighbors.extend(query_outgoing(conn, path));
    }
    if direction == Direction::Backlinks || direction == Direction::Both {
        neighbors.extend(query_backlinks_neighbors(conn, path));
    }
    neighbors
}

fn query_outgoing(conn: &Connection, path: &str) -> Vec<(String, String, RelationKind)> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT DISTINCT to_path, COALESCE(alias, raw_target, to_path) \
         FROM links WHERE from_path = ? ORDER BY to_path",
    ) else {
        return Vec::new();
    };
    stmt.query_map(params![path], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })
    .map(|rows| {
        rows.filter_map(Result::ok)
            .map(|(p, t)| (p, t, RelationKind::Outgoing))
            .collect()
    })
    .unwrap_or_default()
}

fn query_backlinks_neighbors(conn: &Connection, path: &str) -> Vec<(String, String, RelationKind)> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT DISTINCT from_path, COALESCE(alias, raw_target, from_path) \
         FROM links WHERE to_path = ? ORDER BY from_path",
    ) else {
        return Vec::new();
    };
    stmt.query_map(params![path], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })
    .map(|rows| {
        rows.filter_map(Result::ok)
            .map(|(p, t)| (p, t, RelationKind::Backlink))
            .collect()
    })
    .unwrap_or_default()
}

fn query_title(conn: &Connection, path: &str) -> Option<String> {
    conn.query_row(
        "SELECT title FROM notes WHERE vault_path = ? AND active = 1",
        params![path],
        |row| row.get(0),
    )
    .ok()
    .flatten()
}

/// Returns true when `scope_only` is empty or the path starts with any listed prefix.
fn passes_scope_filter(path: &str, scope_only: &[String]) -> bool {
    if scope_only.is_empty() {
        return true;
    }
    scope_only.iter().any(|s| path.starts_with(s.as_str()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use rusqlite::Connection;

    use super::*;
    use crate::migrations::run_migrations;
    use crate::search::Direction;

    fn fresh_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn insert_note(conn: &Connection, vault_path: &str, title: &str) {
        conn.execute(
            "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active) \
             VALUES (?, ?, '[]', '[]', '', 0, 0, 'h', 'd', 1)",
            params![vault_path, title],
        )
        .unwrap();
    }

    fn insert_link(conn: &Connection, from: &str, to: &str, raw_target: &str) {
        conn.execute(
            "INSERT OR IGNORE INTO links (from_path, to_path, raw_target) VALUES (?, ?, ?)",
            params![from, to, raw_target],
        )
        .unwrap();
    }

    fn related_input(path: &str, depth: u8, direction: Direction) -> RelatedInput {
        RelatedInput {
            path: path.to_string(),
            depth,
            direction,
            scope: Vec::new(),
            scope_only: Vec::new(),
        }
    }

    fn make_graph(conn: &Connection) {
        // Graph/Parent → Graph/Child → Graph/Grandchild
        insert_note(conn, "Graph/Parent.md", "Parent");
        insert_note(conn, "Graph/Child.md", "Child");
        insert_note(conn, "Graph/Grandchild.md", "Grandchild");
        insert_link(conn, "Graph/Parent.md", "Graph/Child.md", "[[Child]]");
        insert_link(
            conn,
            "Graph/Child.md",
            "Graph/Grandchild.md",
            "[[Grandchild]]",
        );
    }

    #[test]
    fn outgoing_depth1_returns_direct_links() {
        let conn = fresh_db();
        make_graph(&conn);

        let resp = find_related(
            &conn,
            &related_input("Graph/Parent.md", 1, Direction::Outgoing),
        );

        assert_eq!(resp.results.len(), 1);
        assert_eq!(resp.results[0].vault_path.as_str(), "Graph/Child.md");
        assert_eq!(resp.results[0].relation, RelationKind::Outgoing);
        assert_eq!(resp.results[0].title, "Child");
    }

    #[test]
    fn outgoing_depth2_returns_transitive_links() {
        let conn = fresh_db();
        make_graph(&conn);

        let resp = find_related(
            &conn,
            &related_input("Graph/Parent.md", 2, Direction::Outgoing),
        );

        let paths: Vec<&str> = resp.results.iter().map(|r| r.vault_path.as_str()).collect();
        assert!(
            paths.contains(&"Graph/Child.md"),
            "depth-2 must include direct link"
        );
        assert!(
            paths.contains(&"Graph/Grandchild.md"),
            "depth-2 must include transitive link"
        );
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn backlinks_depth1_returns_inbound_links() {
        let conn = fresh_db();
        make_graph(&conn);

        let resp = find_related(
            &conn,
            &related_input("Graph/Child.md", 1, Direction::Backlinks),
        );

        assert_eq!(resp.results.len(), 1);
        assert_eq!(resp.results[0].vault_path.as_str(), "Graph/Parent.md");
        assert_eq!(resp.results[0].relation, RelationKind::Backlink);
    }

    #[test]
    fn both_direction_returns_outgoing_and_backlinks() {
        let conn = fresh_db();
        make_graph(&conn);

        // Graph/Child.md has:
        //   - 1 backlink from Graph/Parent.md
        //   - 1 outgoing link to Graph/Grandchild.md
        let resp = find_related(&conn, &related_input("Graph/Child.md", 1, Direction::Both));

        let paths: Vec<&str> = resp.results.iter().map(|r| r.vault_path.as_str()).collect();
        assert!(paths.contains(&"Graph/Parent.md"), "must include backlink");
        assert!(
            paths.contains(&"Graph/Grandchild.md"),
            "must include outgoing link"
        );
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn orphan_note_returns_empty_results() {
        let conn = fresh_db();
        insert_note(&conn, "Orphan.md", "Orphan");

        let resp = find_related(&conn, &related_input("Orphan.md", 3, Direction::Both));

        assert!(
            resp.results.is_empty(),
            "note with no links must return empty results"
        );
    }
}
