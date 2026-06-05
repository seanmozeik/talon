//! `SQLite` persistence for graph snapshots.

use rusqlite::{Connection, params};
use time::OffsetDateTime;

use crate::TalonError;

use super::build::BuiltGraph;

pub(super) fn replace_graph(conn: &Connection, graph: &BuiltGraph) -> Result<(), TalonError> {
    clear_graph(conn)?;
    write_meta(conn, "db_version", &graph.db_version.to_string())?;
    write_meta(
        conn,
        "built_at",
        &OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| String::from("1970-01-01T00:00:00Z")),
    )?;
    write_meta(conn, "node_count", &graph.nodes.len().to_string())?;
    write_meta(conn, "edge_count", &graph.edges.len().to_string())?;

    for node in graph.nodes.values() {
        insert_node(conn, node)?;
    }

    for edge in &graph.edges {
        insert_edge(conn, edge)?;
    }

    for (source, paths) in &graph.source_citations {
        for path in paths {
            conn.execute(
                "INSERT INTO graph_sources (source, path) VALUES (?1, ?2)",
                params![source, path],
            )
            .map_err(|source| TalonError::Sqlite {
                context: "insert graph source",
                source,
            })?;
        }
    }
    replace_communities(conn, graph)?;
    Ok(())
}

pub(super) fn update_graph(
    conn: &Connection,
    graph: &BuiltGraph,
    changed_paths: &std::collections::BTreeSet<String>,
) -> Result<(), TalonError> {
    write_meta(conn, "db_version", &graph.db_version.to_string())?;
    write_meta(
        conn,
        "built_at",
        &OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| String::from("1970-01-01T00:00:00Z")),
    )?;
    write_meta(conn, "node_count", &graph.nodes.len().to_string())?;
    write_meta(conn, "edge_count", &graph.edges.len().to_string())?;

    for path in changed_paths {
        conn.execute("DELETE FROM graph_nodes WHERE vault_path = ?1", [path])
            .map_err(|source| TalonError::Sqlite {
                context: "delete graph node",
                source,
            })?;
        conn.execute("DELETE FROM graph_sources WHERE path = ?1", [path])
            .map_err(|source| TalonError::Sqlite {
                context: "delete graph source",
                source,
            })?;
        conn.execute(
            "DELETE FROM graph_edges WHERE from_path = ?1 OR to_path = ?1",
            [path],
        )
        .map_err(|source| TalonError::Sqlite {
            context: "delete graph edge",
            source,
        })?;
    }
    for path in changed_paths {
        if let Some(node) = graph.nodes.get(path) {
            insert_node(conn, node)?;
        }
    }
    for edge in &graph.edges {
        if changed_paths.contains(&edge.from_path) || changed_paths.contains(&edge.to_path) {
            insert_edge(conn, edge)?;
        }
    }
    for (source, paths) in &graph.source_citations {
        for path in paths {
            if changed_paths.contains(path) {
                conn.execute(
                    "INSERT INTO graph_sources (source, path) VALUES (?1, ?2)",
                    params![source, path],
                )
                .map_err(|source| TalonError::Sqlite {
                    context: "insert graph source",
                    source,
                })?;
            }
        }
    }
    for node in graph.nodes.values() {
        update_node_metrics(conn, node)?;
    }
    replace_communities(conn, graph)
}

fn clear_graph(conn: &Connection) -> Result<(), TalonError> {
    for table in [
        "graph_meta",
        "graph_nodes",
        "graph_edges",
        "graph_sources",
        "graph_communities",
    ] {
        conn.execute(&format!("DELETE FROM {table}"), [])
            .map_err(|source| TalonError::Sqlite {
                context: "clear graph table",
                source,
            })?;
    }
    Ok(())
}

fn write_meta(conn: &Connection, key: &str, value: &str) -> Result<(), TalonError> {
    conn.execute(
        "INSERT INTO graph_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map(|_| ())
    .map_err(|source| TalonError::Sqlite {
        context: "write graph metadata",
        source,
    })
}

fn insert_node(conn: &Connection, node: &super::GraphNode) -> Result<(), TalonError> {
    conn.execute(
        "INSERT INTO graph_nodes (
           vault_path, title, aliases, tags, scope, note_type, sources,
           outgoing_degree, backlink_degree, total_degree, structural,
           community_id, community_cohesion, community_neighbor_count, bridge_weight
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            node.vault_path,
            node.title,
            serde_json::to_string(&node.aliases).unwrap_or_else(|_| String::from("[]")),
            serde_json::to_string(&node.tags).unwrap_or_else(|_| String::from("[]")),
            node.scope,
            node.note_type,
            serde_json::to_string(&node.sources).unwrap_or_else(|_| String::from("[]")),
            node.outgoing_degree,
            node.backlink_degree,
            node.total_degree,
            u8::from(node.structural),
            node.community_id,
            node.community_cohesion,
            node.community_neighbor_count,
            node.bridge_weight,
        ],
    )
    .map(|_| ())
    .map_err(|source| TalonError::Sqlite {
        context: "insert graph node",
        source,
    })
}

fn insert_edge(conn: &Connection, edge: &super::GraphEdge) -> Result<(), TalonError> {
    conn.execute(
        "INSERT INTO graph_edges (from_path, to_path, link_text, weight)
         VALUES (?1, ?2, ?3, ?4)",
        params![edge.from_path, edge.to_path, edge.link_text, edge.weight],
    )
    .map(|_| ())
    .map_err(|source| TalonError::Sqlite {
        context: "insert graph edge",
        source,
    })
}

fn update_node_metrics(conn: &Connection, node: &super::GraphNode) -> Result<(), TalonError> {
    conn.execute(
        "UPDATE graph_nodes
         SET outgoing_degree = ?2,
             backlink_degree = ?3,
             total_degree = ?4,
             community_id = ?5,
             community_cohesion = ?6,
             community_neighbor_count = ?7,
             bridge_weight = ?8
         WHERE vault_path = ?1",
        params![
            node.vault_path,
            node.outgoing_degree,
            node.backlink_degree,
            node.total_degree,
            node.community_id,
            node.community_cohesion,
            node.community_neighbor_count,
            node.bridge_weight,
        ],
    )
    .map(|_| ())
    .map_err(|source| TalonError::Sqlite {
        context: "update graph node metrics",
        source,
    })
}

fn replace_communities(conn: &Connection, graph: &BuiltGraph) -> Result<(), TalonError> {
    conn.execute("DELETE FROM graph_communities", [])
        .map_err(|source| TalonError::Sqlite {
            context: "clear graph communities",
            source,
        })?;
    for community in &graph.communities {
        conn.execute(
            "INSERT INTO graph_communities (community_id, node_count, cohesion, top_nodes)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                community.community_id,
                community.node_count,
                community.cohesion,
                serde_json::to_string(&community.top_nodes).unwrap_or_else(|_| String::from("[]")),
            ],
        )
        .map_err(|source| TalonError::Sqlite {
            context: "insert graph community",
            source,
        })?;
    }
    Ok(())
}
