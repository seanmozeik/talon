//! In-memory view of the persisted graph artifact.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::TalonError;

/// Active note metadata and graph metrics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphNode {
    /// Vault-relative path.
    pub vault_path: String,
    /// Display title.
    pub title: String,
    /// Stored note aliases.
    pub aliases: Vec<String>,
    /// Stored note tags.
    pub tags: Vec<String>,
    /// Resolved scope name.
    pub scope: String,
    /// Explicit frontmatter `type`, when present.
    pub note_type: Option<String>,
    /// Normalized frontmatter sources.
    pub sources: Vec<String>,
    /// Distinct outgoing graph neighbors.
    pub outgoing_degree: u32,
    /// Distinct backlink graph neighbors.
    pub backlink_degree: u32,
    /// Distinct undirected graph neighbors.
    pub total_degree: u32,
    /// Whether this note is an index/readme/overview-style page.
    pub structural: bool,
    /// Persisted community id.
    pub community_id: Option<u32>,
    /// Persisted community cohesion.
    pub community_cohesion: f64,
    /// Number of neighboring communities.
    pub community_neighbor_count: u32,
    /// Weighted cross-community bridge degree.
    pub bridge_weight: f64,
}

/// Active-active directed graph edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    /// Source note path.
    pub from_path: String,
    /// Target note path.
    pub to_path: String,
    /// Representative link text.
    pub link_text: String,
    /// Number of link rows represented by this edge.
    pub weight: u32,
}

/// Full graph artifact loaded for query-time graph intelligence.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GraphSnapshot {
    /// Index content version used to build this graph.
    pub db_version: u64,
    /// Build timestamp in UTC.
    pub built_at: Option<String>,
    /// Active nodes keyed by vault path.
    pub nodes: BTreeMap<String, GraphNode>,
    /// Directed active-active edges.
    pub edges: Vec<GraphEdge>,
    /// Normalized source to citing paths map.
    pub source_citations: BTreeMap<String, BTreeSet<String>>,
}

/// Loads the latest persisted graph snapshot.
///
/// Missing graph tables or an empty artifact return an empty snapshot so fast
/// query paths can skip graph refinement gracefully.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] when graph tables exist but cannot be read.
pub fn load_graph_snapshot(conn: &Connection) -> Result<GraphSnapshot, TalonError> {
    if !table_exists(conn, "graph_nodes")? {
        return Ok(GraphSnapshot::default());
    }

    let mut snapshot = GraphSnapshot {
        db_version: read_graph_meta_u64(conn, "db_version")?,
        built_at: read_graph_meta(conn, "built_at")?,
        nodes: BTreeMap::new(),
        edges: Vec::new(),
        source_citations: BTreeMap::new(),
    };

    load_nodes(conn, &mut snapshot)?;
    load_edges(conn, &mut snapshot)?;
    load_sources(conn, &mut snapshot)?;
    Ok(snapshot)
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, TalonError> {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get::<_, u32>(0),
    )
    .map(|count| count > 0)
    .map_err(|source| TalonError::Sqlite {
        context: "check graph table",
        source,
    })
}

fn load_nodes(conn: &Connection, snapshot: &mut GraphSnapshot) -> Result<(), TalonError> {
    let mut stmt = conn
        .prepare(
            "SELECT vault_path, title, aliases, tags, scope, note_type, sources,
                    outgoing_degree, backlink_degree, total_degree, structural,
                    community_id, community_cohesion, community_neighbor_count, bridge_weight
             FROM graph_nodes
             ORDER BY vault_path",
        )
        .map_err(|source| TalonError::Sqlite {
            context: "load graph nodes",
            source,
        })?;
    let rows = stmt
        .query_map([], |row| {
            let aliases_json: String = row.get(2)?;
            let tags_json: String = row.get(3)?;
            let sources_json: String = row.get(6)?;
            Ok(GraphNode {
                vault_path: row.get(0)?,
                title: row.get(1)?,
                aliases: serde_json::from_str(&aliases_json).unwrap_or_default(),
                tags: serde_json::from_str(&tags_json).unwrap_or_default(),
                scope: row.get(4)?,
                note_type: row.get(5)?,
                sources: serde_json::from_str(&sources_json).unwrap_or_default(),
                outgoing_degree: row.get(7)?,
                backlink_degree: row.get(8)?,
                total_degree: row.get(9)?,
                structural: row.get::<_, i64>(10)? != 0,
                community_id: row.get(11)?,
                community_cohesion: row.get(12)?,
                community_neighbor_count: row.get(13)?,
                bridge_weight: row.get(14)?,
            })
        })
        .map_err(|source| TalonError::Sqlite {
            context: "load graph nodes",
            source,
        })?;
    for node in rows {
        let node = node.map_err(|source| TalonError::Sqlite {
            context: "load graph nodes",
            source,
        })?;
        snapshot.nodes.insert(node.vault_path.clone(), node);
    }
    Ok(())
}

fn load_edges(conn: &Connection, snapshot: &mut GraphSnapshot) -> Result<(), TalonError> {
    let mut stmt = conn
        .prepare(
            "SELECT from_path, to_path, link_text, weight
             FROM graph_edges
             ORDER BY from_path, to_path",
        )
        .map_err(|source| TalonError::Sqlite {
            context: "load graph edges",
            source,
        })?;
    snapshot.edges = stmt
        .query_map([], |row| {
            Ok(GraphEdge {
                from_path: row.get(0)?,
                to_path: row.get(1)?,
                link_text: row.get(2)?,
                weight: row.get(3)?,
            })
        })
        .map_err(|source| TalonError::Sqlite {
            context: "load graph edges",
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| TalonError::Sqlite {
            context: "load graph edges",
            source,
        })?;
    Ok(())
}

fn load_sources(conn: &Connection, snapshot: &mut GraphSnapshot) -> Result<(), TalonError> {
    let mut stmt = conn
        .prepare("SELECT source, path FROM graph_sources ORDER BY source, path")
        .map_err(|source| TalonError::Sqlite {
            context: "load graph sources",
            source,
        })?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|source| TalonError::Sqlite {
            context: "load graph sources",
            source,
        })?;
    for row in rows {
        let (source, path) = row.map_err(|source| TalonError::Sqlite {
            context: "load graph sources",
            source,
        })?;
        snapshot
            .source_citations
            .entry(source)
            .or_default()
            .insert(path);
    }
    Ok(())
}

fn read_graph_meta(conn: &Connection, key: &str) -> Result<Option<String>, TalonError> {
    match conn.query_row(
        "SELECT value FROM graph_meta WHERE key = ?1",
        [key],
        |row| row.get(0),
    ) {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(source) => Err(TalonError::Sqlite {
            context: "read graph metadata",
            source,
        }),
    }
}

fn read_graph_meta_u64(conn: &Connection, key: &str) -> Result<u64, TalonError> {
    Ok(read_graph_meta(conn, key)?
        .and_then(|value| value.parse().ok())
        .unwrap_or_default())
}
