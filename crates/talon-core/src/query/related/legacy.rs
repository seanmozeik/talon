use std::collections::{HashSet, VecDeque};

use rusqlite::{Connection, params};

use crate::config::{ScopeFilter, TalonConfig};
use crate::contracts::VaultPath;
use crate::graph::GraphSignalBreakdown;
use crate::search::Direction;

use super::{RelatedResult, RelationKind};

pub(super) fn legacy_related_results(
    conn: &Connection,
    path: &str,
    depth: u8,
    direction: Direction,
    filter: Option<&ScopeFilter<'_>>,
    config: Option<&TalonConfig>,
) -> Vec<RelatedResult> {
    let mut visited = HashSet::from([path.to_string()]);
    let mut queue = VecDeque::from([(path.to_string(), 0_u8)]);
    let mut results = Vec::new();

    while let Some((current_path, current_depth)) = queue.pop_front() {
        if current_depth >= depth {
            continue;
        }

        for (neighbor_path, link_text, relation, count) in
            collect_neighbors(conn, &current_path, direction)
        {
            if visited.contains(&neighbor_path) {
                continue;
            }
            visited.insert(neighbor_path.clone());
            if filter.is_some_and(|f| !f.accepts(&neighbor_path)) {
                continue;
            }
            if let Some(result) =
                legacy_result(conn, &neighbor_path, link_text, relation, count, config)
            {
                results.push(result);
            }
            queue.push_back((neighbor_path, current_depth + 1));
        }
    }
    results
}

fn legacy_result(
    conn: &Connection,
    neighbor_path: &str,
    link_text: String,
    relation: RelationKind,
    count: u32,
    config: Option<&TalonConfig>,
) -> Option<RelatedResult> {
    Some(RelatedResult {
        vault_path: VaultPath::parse(neighbor_path).ok()?,
        title: query_title(conn, neighbor_path).unwrap_or_else(|| neighbor_path.to_string()),
        link_text,
        relation,
        count,
        score: f64::from(count),
        signals: legacy_signals(relation, count),
        scope: config
            .and_then(|cfg| cfg.resolve_scope_name(std::path::Path::new(neighbor_path)))
            .map(str::to_string),
        mtime: super::super::mtime::local_mtime_for_path(conn, neighbor_path),
    })
}

fn legacy_signals(relation: RelationKind, count: u32) -> GraphSignalBreakdown {
    let count = f64::from(count);
    GraphSignalBreakdown {
        direct_out: if relation == RelationKind::Outgoing {
            count
        } else {
            0.0
        },
        direct_backlink: if relation == RelationKind::Backlink {
            count
        } else {
            0.0
        },
        ..GraphSignalBreakdown::default()
    }
}

fn collect_neighbors(
    conn: &Connection,
    path: &str,
    direction: Direction,
) -> Vec<(String, String, RelationKind, u32)> {
    let mut neighbors = Vec::new();
    if matches!(direction, Direction::Outgoing | Direction::Both) {
        neighbors.extend(query_outgoing(conn, path));
    }
    if matches!(direction, Direction::Backlinks | Direction::Both) {
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
            .map(|(path, text, count)| (path, text, RelationKind::Outgoing, count))
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
            .map(|(path, text, count)| (path, text, RelationKind::Backlink, count))
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
