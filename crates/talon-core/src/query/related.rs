//! Related-notes handler for the Talon CLI.

use std::collections::{HashSet, VecDeque};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::config::{ScopeFilter, TalonConfig};
use crate::constants::RELATED_MAX_DEPTH;
use crate::contracts::{ContainerPath, VaultPath};
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
    /// Include every configured scope, overriding `default = false`.
    #[serde(default)]
    pub scope_all: bool,
}

/// Related-note response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedResponse {
    /// Vault root (absolute container path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault: Option<ContainerPath>,
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
    /// Number of distinct link rows connecting source and target. A note
    /// linked once and a note linked from three different aliases score
    /// 1 vs 3 — a rough proxy for edge strength.
    pub count: u32,
    /// Resolved scope name, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// File modification time as RFC 3339 / ISO 8601.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime: Option<String>,
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
pub fn find_related(
    conn: &Connection,
    input: &RelatedInput,
    config: Option<&TalonConfig>,
) -> RelatedResponse {
    let path = input.path.trim();

    let Ok(source_path) = VaultPath::parse(path) else {
        return RelatedResponse {
            vault: None,
            path: VaultPath::parse("_").unwrap_or_else(|_| unreachable!()),
            direction: input.direction,
            results: Vec::new(),
        };
    };

    let depth = input.depth.clamp(1, RELATED_MAX_DEPTH);
    let direction = input.direction;

    let filter = config.map(|cfg| {
        ScopeFilter::from_args(cfg, &input.scope, &input.scope_only, input.scope_all)
            .unwrap_or_else(|_| ScopeFilter::default_for(cfg))
    });

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

        for (neighbor_path, link_text, relation, count) in neighbors {
            if visited.contains(&neighbor_path) {
                continue;
            }
            visited.insert(neighbor_path.clone());

            if let Some(ref f) = filter
                && !f.accepts(&neighbor_path)
            {
                continue;
            }

            let title = query_title(conn, &neighbor_path).unwrap_or_else(|| neighbor_path.clone());

            let Ok(vault_path) = VaultPath::parse(&neighbor_path) else {
                continue;
            };

            let scope = config
                .and_then(|cfg| cfg.resolve_scope_name(std::path::Path::new(&neighbor_path)))
                .map(str::to_string);
            let mtime = super::mtime::local_mtime_for_path(conn, &neighbor_path);

            results.push(RelatedResult {
                vault_path,
                title,
                link_text,
                relation,
                count,
                scope,
                mtime,
            });

            queue.push_back((neighbor_path, current_depth + 1));
        }
    }

    RelatedResponse {
        vault: None,
        path: source_path,
        direction,
        results,
    }
}

/// Returns `(neighbor_path, link_text, relation, count)` tuples for a given path.
fn collect_neighbors(
    conn: &Connection,
    path: &str,
    direction: Direction,
) -> Vec<(String, String, RelationKind, u32)> {
    let mut neighbors = Vec::new();
    if direction == Direction::Outgoing || direction == Direction::Both {
        neighbors.extend(query_outgoing(conn, path));
    }
    if direction == Direction::Backlinks || direction == Direction::Both {
        neighbors.extend(query_backlinks_neighbors(conn, path));
    }
    neighbors
}

fn query_outgoing(conn: &Connection, path: &str) -> Vec<(String, String, RelationKind, u32)> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT to_path, MIN(COALESCE(alias, raw_target, to_path)), COUNT(*) \
         FROM links WHERE from_path = ? GROUP BY to_path ORDER BY to_path",
    ) else {
        return Vec::new();
    };
    stmt.query_map(params![path], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, u32>(2)?,
        ))
    })
    .and_then(Iterator::collect)
    .map(|rows: Vec<(String, String, u32)>| {
        rows.into_iter()
            .map(|(p, t, c)| (p, t, RelationKind::Outgoing, c))
            .collect()
    })
    .unwrap_or_default()
}

fn query_backlinks_neighbors(
    conn: &Connection,
    path: &str,
) -> Vec<(String, String, RelationKind, u32)> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT from_path, MIN(COALESCE(alias, raw_target, from_path)), COUNT(*) \
         FROM links WHERE to_path = ? GROUP BY from_path ORDER BY from_path",
    ) else {
        return Vec::new();
    };
    stmt.query_map(params![path], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, u32>(2)?,
        ))
    })
    .and_then(Iterator::collect)
    .map(|rows: Vec<(String, String, u32)>| {
        rows.into_iter()
            .map(|(p, t, c)| (p, t, RelationKind::Backlink, c))
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
