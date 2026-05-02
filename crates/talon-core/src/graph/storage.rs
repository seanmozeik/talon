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
        .map_err(|source| TalonError::Sqlite {
            context: "insert graph node",
            source,
        })?;
    }

    for edge in &graph.edges {
        conn.execute(
            "INSERT INTO graph_edges (from_path, to_path, link_text, weight)
             VALUES (?1, ?2, ?3, ?4)",
            params![edge.from_path, edge.to_path, edge.link_text, edge.weight],
        )
        .map_err(|source| TalonError::Sqlite {
            context: "insert graph edge",
            source,
        })?;
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
    for suggestion in &graph.missing_links {
        conn.execute(
            "INSERT INTO graph_missing_links (path, target, term, line, provenance)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                suggestion.path,
                suggestion.target,
                suggestion.term,
                suggestion.line,
                suggestion.provenance,
            ],
        )
        .map_err(|source| TalonError::Sqlite {
            context: "insert graph missing link",
            source,
        })?;
    }
    Ok(())
}

fn clear_graph(conn: &Connection) -> Result<(), TalonError> {
    for table in [
        "graph_meta",
        "graph_nodes",
        "graph_edges",
        "graph_sources",
        "graph_communities",
        "graph_missing_links",
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
        "INSERT INTO graph_meta (key, value) VALUES (?1, ?2)",
        params![key, value],
    )
    .map(|_| ())
    .map_err(|source| TalonError::Sqlite {
        context: "write graph metadata",
        source,
    })
}
