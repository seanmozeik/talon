//! Graph-health inspect findings from the persisted graph snapshot.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use crate::config::{ScopeFilter, TalonConfig};
use crate::contracts::VaultPath;
use crate::indexing::{InspectCheck, InspectFinding};

use super::{GraphSnapshot, GraphSuggestionClient, load_graph_snapshot};

const OVERCENTRAL_DEGREE: u32 = 12;
const SPARSE_COMMUNITY_COHESION: f64 = 0.25;

pub fn graph_health(
    conn: &Connection,
    config: Option<&TalonConfig>,
    filter: Option<&ScopeFilter<'_>>,
    skip_llm_suggestions: bool,
) -> Vec<InspectFinding> {
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
    findings.extend(missing_link_findings(
        conn,
        &snapshot,
        config,
        filter,
        skip_llm_suggestions,
    ));
    findings
}

fn isolated_findings(
    snapshot: &GraphSnapshot,
    filter: Option<&ScopeFilter<'_>>,
) -> Vec<InspectFinding> {
    snapshot
        .nodes
        .values()
        .filter(|node| !node.structural && node.total_degree == 0)
        .filter(|node| accepts(filter, &node.vault_path))
        .filter_map(|node| finding(&node.vault_path, "graph-isolated: no links in this note"))
        .collect()
}

fn sparse_community_findings(
    snapshot: &GraphSnapshot,
    filter: Option<&ScopeFilter<'_>>,
) -> Vec<InspectFinding> {
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
                "graph-sparse-community: low cohesion within community",
            )
        })
        .collect()
}

fn overcentral_findings(
    snapshot: &GraphSnapshot,
    filter: Option<&ScopeFilter<'_>>,
) -> Vec<InspectFinding> {
    snapshot
        .nodes
        .values()
        .filter(|node| node.structural && node.total_degree >= OVERCENTRAL_DEGREE)
        .filter(|node| accepts(filter, &node.vault_path))
        .filter_map(|node| {
            finding(
                &node.vault_path,
                "graph-overcentral: high degree on structural node",
            )
        })
        .collect()
}

fn bridge_findings(
    snapshot: &GraphSnapshot,
    filter: Option<&ScopeFilter<'_>>,
) -> Vec<InspectFinding> {
    snapshot
        .nodes
        .values()
        .filter(|node| node.community_neighbor_count >= 2 && node.total_degree <= 2)
        .filter(|node| accepts(filter, &node.vault_path))
        .filter_map(|node| {
            finding(
                &node.vault_path,
                "graph-bridge-thin: connects multiple communities",
            )
        })
        .collect()
}

fn surprising_connection_findings(
    snapshot: &GraphSnapshot,
    filter: Option<&ScopeFilter<'_>>,
) -> Vec<InspectFinding> {
    // Group cross-community targets by source note.
    let mut groups: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for edge in &snapshot.edges {
        let Some(from) = snapshot.nodes.get(&edge.from_path) else {
            continue;
        };
        let Some(to) = snapshot.nodes.get(&edge.to_path) else {
            continue;
        };
        // Skip intra-community, structural, or duplicate edges.
        if from.community_id == to.community_id || from.structural || to.structural {
            continue;
        }
        if !seen.insert((edge.from_path.as_str(), edge.to_path.as_str())) {
            continue;
        }
        if !accepts(filter, &edge.from_path) {
            continue;
        }
        groups
            .entry(edge.from_path.as_str())
            .or_default()
            .push(edge.to_path.as_str());
    }
    let mut findings = Vec::with_capacity(groups.len());
    for (path, targets) in groups {
        let targets_list = targets.join(", ");
        let Ok(path) = VaultPath::parse(path) else {
            continue;
        };
        findings.push(InspectFinding {
            check: InspectCheck::Graph,
            path,
            message: format!(
                "graph-surprising-connection: {} ({})",
                targets_list,
                targets.len()
            ),
            line: None,
        });
    }
    findings
}

fn accepts(filter: Option<&ScopeFilter<'_>>, path: &str) -> bool {
    filter.is_none_or(|scope| scope.accepts(path))
}

fn finding(path: &str, message: &str) -> Option<InspectFinding> {
    Some(InspectFinding {
        check: InspectCheck::Graph,
        path: VaultPath::parse(path).ok()?,
        message: message.to_string(),
        line: None,
    })
}

fn missing_link_findings(
    conn: &Connection,
    snapshot: &GraphSnapshot,
    config: Option<&TalonConfig>,
    filter: Option<&ScopeFilter<'_>>,
    skip_llm_suggestions: bool,
) -> Vec<InspectFinding> {
    let mut suggestions = super::build_link_suggestions(conn, snapshot, None).unwrap_or_default();

    // Append LLM-assisted suggestions if ask model is configured and not skipped.
    #[allow(clippy::collapsible_if)]
    if !skip_llm_suggestions {
        if let Some(client) = config
            .and_then(|cfg| GraphSuggestionClient::from_config(cfg).ok())
            .flatten()
        {
            suggestions.extend(
                super::build_llm_link_suggestions(conn, snapshot, &client).unwrap_or_default(),
            );
        }
    }

    let mut findings = Vec::new();
    for s in &suggestions {
        if !accepts(filter, &s.path) {
            continue;
        }
        let Ok(path) = VaultPath::parse(&s.path) else {
            continue;
        };
        findings.push(InspectFinding {
            check: InspectCheck::Graph,
            path,
            message: format!(
                "graph-missing-link: \"{term}\" -> {target} ({provenance})",
                term = s.term,
                target = s.target,
                provenance = if s.provenance == super::PROVENANCE_LLM {
                    "llm"
                } else {
                    "det"
                }
            ),
            line: Some(s.line),
        });
    }
    findings
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

        let findings = graph_health(&conn, None, None, false);

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

        let findings = graph_health(&conn, None, None, false);

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
