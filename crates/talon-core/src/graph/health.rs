//! Graph-health lint findings from the persisted graph snapshot.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use crate::config::ScopeFilter;
use crate::contracts::VaultPath;
use crate::indexing::{LintCheck, LintFinding};

use super::{GraphSnapshot, load_graph_snapshot};

const OVERCENTRAL_DEGREE: u32 = 12;
const SPARSE_COMMUNITY_COHESION: f64 = 0.25;

pub fn graph_health(conn: &Connection, filter: Option<&ScopeFilter<'_>>) -> Vec<LintFinding> {
    let Ok(snapshot) = load_graph_snapshot(conn) else {
        return Vec::new();
    };
    if snapshot.nodes.is_empty() {
        return Vec::new();
    }
    let mut findings = Vec::new();
    findings.extend(isolated_findings(&snapshot, filter));
    findings.extend(sparse_community_findings(&snapshot, filter));
    findings.extend(overcentral_findings(&snapshot, filter));
    findings.extend(bridge_findings(&snapshot, filter));
    findings.extend(surprising_connection_findings(&snapshot, filter));
    findings.extend(missing_link_findings(conn, filter));
    findings
}

fn isolated_findings(
    snapshot: &GraphSnapshot,
    filter: Option<&ScopeFilter<'_>>,
) -> Vec<LintFinding> {
    snapshot
        .nodes
        .values()
        .filter(|node| !node.structural && node.total_degree == 0)
        .filter(|node| accepts(filter, &node.vault_path))
        .filter_map(|node| {
            finding(
                &node.vault_path,
                "graph-isolated: add at least one useful wikilink",
            )
        })
        .collect()
}

fn sparse_community_findings(
    snapshot: &GraphSnapshot,
    filter: Option<&ScopeFilter<'_>>,
) -> Vec<LintFinding> {
    let mut communities: BTreeMap<u32, Vec<&super::GraphNode>> = BTreeMap::new();
    for node in snapshot.nodes.values() {
        if let Some(id) = node.community_id {
            communities.entry(id).or_default().push(node);
        }
    }
    communities
        .into_values()
        .filter(|nodes| nodes.len() >= 3)
        .filter(|nodes| nodes[0].community_cohesion < SPARSE_COMMUNITY_COHESION)
        .filter_map(|nodes| {
            nodes
                .iter()
                .find(|node| accepts(filter, &node.vault_path))
                .copied()
        })
        .filter_map(|node| {
            finding(
                &node.vault_path,
                "graph-sparse-community: add links between notes in this cluster",
            )
        })
        .collect()
}

fn overcentral_findings(
    snapshot: &GraphSnapshot,
    filter: Option<&ScopeFilter<'_>>,
) -> Vec<LintFinding> {
    snapshot
        .nodes
        .values()
        .filter(|node| node.structural && node.total_degree >= OVERCENTRAL_DEGREE)
        .filter(|node| accepts(filter, &node.vault_path))
        .filter_map(|node| {
            finding(
                &node.vault_path,
                "graph-overcentral: split or downscope this structural hub",
            )
        })
        .collect()
}

fn bridge_findings(snapshot: &GraphSnapshot, filter: Option<&ScopeFilter<'_>>) -> Vec<LintFinding> {
    snapshot
        .nodes
        .values()
        .filter(|node| node.community_neighbor_count >= 2 && node.total_degree <= 2)
        .filter(|node| accepts(filter, &node.vault_path))
        .filter_map(|node| {
            finding(
                &node.vault_path,
                "graph-bridge-thin: strengthen this cross-community bridge",
            )
        })
        .collect()
}

fn surprising_connection_findings(
    snapshot: &GraphSnapshot,
    filter: Option<&ScopeFilter<'_>>,
) -> Vec<LintFinding> {
    let mut seen = BTreeSet::new();
    snapshot
        .edges
        .iter()
        .filter_map(|edge| {
            let from = snapshot.nodes.get(&edge.from_path)?;
            let to = snapshot.nodes.get(&edge.to_path)?;
            if from.community_id == to.community_id || from.structural || to.structural {
                return None;
            }
            if !seen.insert((edge.from_path.clone(), edge.to_path.clone())) {
                return None;
            }
            if !accepts(filter, &edge.from_path) {
                return None;
            }
            finding(
                &edge.from_path,
                &format!(
                    "graph-surprising-connection: review link to {}",
                    edge.to_path
                ),
            )
        })
        .collect()
}

fn accepts(filter: Option<&ScopeFilter<'_>>, path: &str) -> bool {
    filter.is_none_or(|scope| scope.accepts(path))
}

fn finding(path: &str, message: &str) -> Option<LintFinding> {
    Some(LintFinding {
        check: LintCheck::Graph,
        path: VaultPath::parse(path).ok()?,
        message: message.to_string(),
        line: None,
    })
}

fn missing_link_findings(conn: &Connection, filter: Option<&ScopeFilter<'_>>) -> Vec<LintFinding> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT path, target, term, line FROM graph_missing_links
         ORDER BY path, target, term",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<u32>>(3)?,
        ))
    }) else {
        return Vec::new();
    };
    rows.flatten()
        .filter(|(path, _target, _term, _line)| accepts(filter, path))
        .filter_map(|(path, target, term, line)| {
            Some(LintFinding {
                check: LintCheck::Graph,
                path: VaultPath::parse(&path).ok()?,
                message: format!("graph-missing-link: possible wikilink: \"{term}\" -> {target}"),
                line,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use rusqlite::{Connection, params};

    use crate::indexing::migrations::run_migrations;

    use super::graph_health;

    #[test]
    fn graph_health_reports_isolated_and_overcentral() -> Result<(), Box<dyn std::error::Error>> {
        let mut conn = Connection::open_in_memory()?;
        run_migrations(&mut conn)?;
        insert_node(&conn, "Isolated.md", 0, false, None, 0.0, 0)?;
        insert_node(&conn, "Index.md", 12, true, None, 0.0, 0)?;

        let findings = graph_health(&conn, None);

        assert!(
            findings
                .iter()
                .any(|finding| finding.message.starts_with("graph-isolated"))
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.message.starts_with("graph-overcentral"))
        );
        Ok(())
    }

    #[test]
    fn graph_health_reports_sparse_bridge_and_cross_community_edge()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut conn = Connection::open_in_memory()?;
        run_migrations(&mut conn)?;
        insert_node(&conn, "A.md", 1, false, Some(0), 0.1, 0)?;
        insert_node(&conn, "B.md", 1, false, Some(0), 0.1, 0)?;
        insert_node(&conn, "C.md", 1, false, Some(0), 0.1, 0)?;
        insert_node(&conn, "Bridge.md", 2, false, Some(1), 0.4, 2)?;
        insert_edge(&conn, "A.md", "Bridge.md")?;

        let findings = graph_health(&conn, None);

        assert!(
            findings
                .iter()
                .any(|finding| finding.message.starts_with("graph-sparse-community"))
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.message.starts_with("graph-bridge-thin"))
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.message.starts_with("graph-surprising-connection"))
        );
        Ok(())
    }

    fn insert_node(
        conn: &Connection,
        path: &str,
        degree: u32,
        structural: bool,
        community_id: Option<u32>,
        cohesion: f64,
        community_neighbor_count: u32,
    ) -> Result<(), rusqlite::Error> {
        conn.execute(
            "INSERT INTO graph_nodes (
               vault_path, title, aliases, tags, scope, note_type, sources,
               outgoing_degree, backlink_degree, total_degree, structural,
               community_id, community_cohesion, community_neighbor_count, bridge_weight
             ) VALUES (?1, ?1, '[]', '[]', '', NULL, '[]', 0, 0, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                path,
                degree,
                u8::from(structural),
                community_id,
                cohesion,
                community_neighbor_count
            ],
        )?;
        Ok(())
    }

    fn insert_edge(
        conn: &Connection,
        from_path: &str,
        to_path: &str,
    ) -> Result<(), rusqlite::Error> {
        conn.execute(
            "INSERT INTO graph_edges (from_path, to_path, link_text, weight)
             VALUES (?1, ?2, ?2, 1)",
            params![from_path, to_path],
        )?;
        Ok(())
    }
}
