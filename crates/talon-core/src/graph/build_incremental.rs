//! Incremental graph artifact updates.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};

use crate::TalonError;
use crate::indexing::migrations::read_db_version;

use super::build::{
    BuiltGraph, GraphBuildInput, GraphBuildStats, GraphFrontmatter, build_graph,
    clean_source_reference, graph_stats, is_structural_page, parse_string_vec, populate_degrees,
    rebuild_graph,
};
use super::snapshot::{GraphEdge, GraphNode};

/// Updates graph tables for changed paths while preserving full-rebuild output.
///
/// Communities are recomputed globally so assignments and bridge metrics match
/// the full rebuild path exactly.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] when the index or graph tables cannot be queried or written.
pub fn update_graph_incremental(
    conn: &mut Connection,
    changed_paths: &[String],
) -> Result<GraphBuildStats, TalonError> {
    if changed_paths.is_empty() {
        return rebuild_graph(conn, &GraphBuildInput);
    }
    crate::indexing::migrations::run_migrations(conn)?;
    let mut graph = load_incremental_base(conn)?;
    let changed_paths = changed_paths.iter().cloned().collect::<BTreeSet<_>>();
    update_nodes(conn, &mut graph, &changed_paths)?;
    update_edges(conn, &mut graph, &changed_paths)?;
    populate_degrees(&mut graph);
    let mut snapshot = super::GraphSnapshot {
        db_version: graph.db_version,
        built_at: None,
        nodes: graph.nodes.clone(),
        edges: graph.edges.clone(),
        source_citations: graph.source_citations.clone(),
    };
    graph.communities = super::detect_communities(&mut snapshot);
    graph.nodes = snapshot.nodes;
    let stats = graph_stats(&graph);
    let tx = conn.transaction().map_err(|source| TalonError::Sqlite {
        context: "persist graph transaction",
        source,
    })?;
    super::storage::update_graph(&tx, &graph, &changed_paths)?;
    tx.commit().map_err(|source| TalonError::Sqlite {
        context: "persist graph transaction",
        source,
    })?;
    Ok(stats)
}

fn load_incremental_base(conn: &Connection) -> Result<BuiltGraph, TalonError> {
    let snapshot = super::load_graph_snapshot(conn)?;
    if snapshot.nodes.is_empty() && snapshot.db_version == 0 {
        return build_graph(conn);
    }
    Ok(BuiltGraph {
        db_version: read_db_version(conn),
        nodes: snapshot.nodes,
        edges: snapshot.edges,
        source_citations: snapshot.source_citations,
        communities: Vec::new(),
    })
}

fn update_nodes(
    conn: &Connection,
    graph: &mut BuiltGraph,
    changed_paths: &BTreeSet<String>,
) -> Result<(), TalonError> {
    for path in changed_paths {
        remove_source_citations_for_path(graph, path);
        match load_node(conn, path)? {
            Some(node) => {
                for source in &node.sources {
                    graph
                        .source_citations
                        .entry(source.clone())
                        .or_default()
                        .insert(path.clone());
                }
                graph.nodes.insert(path.clone(), node);
            }
            None => {
                graph.nodes.remove(path);
            }
        }
    }
    Ok(())
}

fn remove_source_citations_for_path(graph: &mut BuiltGraph, path: &str) {
    graph.source_citations.retain(|_, paths| {
        paths.remove(path);
        !paths.is_empty()
    });
}

fn load_node(conn: &Connection, path: &str) -> Result<Option<GraphNode>, TalonError> {
    let row = conn
        .query_row(
            "SELECT id, vault_path, COALESCE(title, vault_path), aliases, tags, scope
             FROM notes
             WHERE active = 1 AND vault_path = ?1",
            [path],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional()
        .map_err(|source| TalonError::Sqlite {
            context: "load graph node",
            source,
        })?;
    let Some((note_id, vault_path, title, aliases_json, tags_json, scope)) = row else {
        return Ok(None);
    };
    let frontmatter = load_graph_frontmatter_for_note(conn, note_id)?;
    let sources = frontmatter
        .sources
        .iter()
        .filter_map(|source| {
            let cleaned = clean_source_reference(&vault_path, source);
            (!cleaned.is_empty()).then_some(cleaned)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    Ok(Some(GraphNode {
        structural: is_structural_page(&vault_path),
        vault_path,
        title,
        aliases: parse_string_vec(&aliases_json),
        tags: parse_string_vec(&tags_json),
        scope,
        note_type: frontmatter.note_type,
        sources,
        outgoing_degree: 0,
        backlink_degree: 0,
        total_degree: 0,
        community_id: None,
        community_cohesion: 0.0,
        community_neighbor_count: 0,
        bridge_weight: 0.0,
    }))
}

fn load_graph_frontmatter_for_note(
    conn: &Connection,
    note_id: i64,
) -> Result<GraphFrontmatter, TalonError> {
    let mut stmt = conn
        .prepare(
            "SELECT field, value
             FROM note_frontmatter_fields
             WHERE note_id = ?1 AND field IN ('sources', 'type')
             ORDER BY field, value",
        )
        .map_err(|source| TalonError::Sqlite {
            context: "load graph frontmatter",
            source,
        })?;
    let rows = stmt
        .query_map([note_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|source| TalonError::Sqlite {
            context: "load graph frontmatter",
            source,
        })?;
    let mut frontmatter = GraphFrontmatter::default();
    for row in rows {
        let (field, value) = row.map_err(|source| TalonError::Sqlite {
            context: "load graph frontmatter",
            source,
        })?;
        match field.as_str() {
            "sources" => frontmatter.sources.push(value),
            "type" if frontmatter.note_type.is_none() => frontmatter.note_type = Some(value),
            _ => {}
        }
    }
    Ok(frontmatter)
}

fn update_edges(
    conn: &Connection,
    graph: &mut BuiltGraph,
    changed_paths: &BTreeSet<String>,
) -> Result<(), TalonError> {
    graph.edges.retain(|edge| {
        !changed_paths.contains(&edge.from_path) && !changed_paths.contains(&edge.to_path)
    });
    let mut stmt = conn
        .prepare(
            "SELECT l.from_path, l.to_path, MIN(COALESCE(l.alias, l.raw_target, l.to_path)), COUNT(*)
             FROM links l
             JOIN notes nf ON nf.vault_path = l.from_path AND nf.active = 1
             JOIN notes nt ON nt.vault_path = l.to_path AND nt.active = 1
             WHERE l.from_path = ?1 OR l.to_path = ?1
             GROUP BY l.from_path, l.to_path
             ORDER BY l.from_path, l.to_path",
        )
        .map_err(|source| TalonError::Sqlite {
            context: "load graph edges",
            source,
        })?;
    for path in changed_paths {
        let rows = stmt
            .query_map(params![path], |row| {
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
            })?;
        for row in rows {
            graph.edges.push(row.map_err(|source| TalonError::Sqlite {
                context: "load graph edges",
                source,
            })?);
        }
    }
    graph.edges.sort_by(|left, right| {
        left.from_path
            .cmp(&right.from_path)
            .then_with(|| left.to_path.cmp(&right.to_path))
    });
    graph
        .edges
        .dedup_by(|left, right| left.from_path == right.from_path && left.to_path == right.to_path);
    Ok(())
}
