//! Build the persisted graph artifact from indexed notes and resolved links.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;
use serde_json::Value;

use crate::TalonError;
use crate::indexing::migrations::read_db_version;

use super::snapshot::{GraphEdge, GraphNode};

/// Graph rebuild options.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphBuildInput;

/// Statistics returned after rebuilding the graph artifact.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphBuildStats {
    /// Active graph node count.
    pub node_count: u32,
    /// Active-active directed edge count.
    pub edge_count: u32,
    /// Normalized source citation rows.
    pub source_count: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct BuiltGraph {
    pub(super) db_version: u64,
    pub(super) nodes: BTreeMap<String, GraphNode>,
    pub(super) edges: Vec<GraphEdge>,
    pub(super) source_citations: BTreeMap<String, BTreeSet<String>>,
    pub(super) communities: Vec<super::CommunityInfo>,
    pub(super) missing_links: Vec<super::LinkSuggestion>,
}

/// Rebuilds graph tables from active notes and active-active resolved links.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] when the index cannot be queried or written.
pub fn rebuild_graph(
    conn: &mut Connection,
    input: &GraphBuildInput,
) -> Result<GraphBuildStats, TalonError> {
    rebuild_graph_inner(conn, input)
}

fn rebuild_graph_inner(
    conn: &mut Connection,
    _input: &GraphBuildInput,
) -> Result<GraphBuildStats, TalonError> {
    crate::indexing::migrations::run_migrations(conn)?;
    let graph = build_graph(conn)?;
    let stats = GraphBuildStats {
        node_count: graph.nodes.len().try_into().unwrap_or(u32::MAX),
        edge_count: graph.edges.len().try_into().unwrap_or(u32::MAX),
        source_count: graph
            .source_citations
            .values()
            .map(BTreeSet::len)
            .sum::<usize>()
            .try_into()
            .unwrap_or(u32::MAX),
    };
    super::storage::replace_graph(conn, &graph)?;
    Ok(stats)
}

fn build_graph(conn: &Connection) -> Result<BuiltGraph, TalonError> {
    let mut graph = BuiltGraph {
        db_version: read_db_version(conn),
        ..BuiltGraph::default()
    };
    load_nodes(conn, &mut graph)?;
    load_edges(conn, &mut graph)?;
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
    Ok(graph)
}

fn load_nodes(conn: &Connection, graph: &mut BuiltGraph) -> Result<(), TalonError> {
    let frontmatter = load_graph_frontmatter(conn)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, vault_path, COALESCE(title, vault_path), aliases, tags, scope
             FROM notes
             WHERE active = 1
             ORDER BY vault_path",
        )
        .map_err(|source| TalonError::Sqlite {
            context: "load graph source notes",
            source,
        })?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|source| TalonError::Sqlite {
            context: "load graph source notes",
            source,
        })?;

    for row in rows {
        let (note_id, vault_path, title, aliases_json, tags_json, scope) =
            row.map_err(|source| TalonError::Sqlite {
                context: "load graph source notes",
                source,
            })?;
        let note_frontmatter = frontmatter.get(&note_id);
        let sources = note_frontmatter
            .map(|fields| {
                fields
                    .sources
                    .iter()
                    .filter_map(|source| {
                        let cleaned = clean_source_reference(&vault_path, source);
                        (!cleaned.is_empty()).then_some(cleaned)
                    })
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for source in &sources {
            graph
                .source_citations
                .entry(source.clone())
                .or_default()
                .insert(vault_path.clone());
        }
        graph.nodes.insert(
            vault_path.clone(),
            GraphNode {
                structural: is_structural_page(&vault_path),
                vault_path,
                title,
                aliases: parse_string_vec(&aliases_json),
                tags: parse_string_vec(&tags_json),
                scope,
                note_type: note_frontmatter.and_then(|fields| fields.note_type.clone()),
                sources,
                outgoing_degree: 0,
                backlink_degree: 0,
                total_degree: 0,
                community_id: None,
                community_cohesion: 0.0,
                community_neighbor_count: 0,
                bridge_weight: 0.0,
            },
        );
    }
    Ok(())
}

#[derive(Debug, Default)]
struct GraphFrontmatter {
    sources: Vec<String>,
    note_type: Option<String>,
}

fn load_graph_frontmatter(
    conn: &Connection,
) -> Result<BTreeMap<i64, GraphFrontmatter>, TalonError> {
    let mut stmt = conn
        .prepare(
            "SELECT note_id, field, value
             FROM note_frontmatter_fields
             WHERE field IN ('sources', 'type')
             ORDER BY note_id, field, value",
        )
        .map_err(|source| TalonError::Sqlite {
            context: "load graph frontmatter",
            source,
        })?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|source| TalonError::Sqlite {
            context: "load graph frontmatter",
            source,
        })?;
    let mut frontmatter = BTreeMap::<i64, GraphFrontmatter>::new();
    for row in rows {
        let (note_id, field, value) = row.map_err(|source| TalonError::Sqlite {
            context: "load graph frontmatter",
            source,
        })?;
        let fields = frontmatter.entry(note_id).or_default();
        match field.as_str() {
            "sources" => fields.sources.push(value),
            "type" if fields.note_type.is_none() => fields.note_type = Some(value),
            _ => {}
        }
    }
    Ok(frontmatter)
}

fn load_edges(conn: &Connection, graph: &mut BuiltGraph) -> Result<(), TalonError> {
    let mut stmt = conn
        .prepare(
            "SELECT l.from_path, l.to_path, MIN(COALESCE(l.alias, l.raw_target, l.to_path)), COUNT(*)
             FROM links l
             JOIN notes nf ON nf.vault_path = l.from_path AND nf.active = 1
             JOIN notes nt ON nt.vault_path = l.to_path AND nt.active = 1
             GROUP BY l.from_path, l.to_path
             ORDER BY l.from_path, l.to_path",
        )
        .map_err(|source| TalonError::Sqlite {
            context: "load graph edges",
            source,
        })?;
    graph.edges = stmt
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

fn populate_degrees(graph: &mut BuiltGraph) {
    let mut outgoing: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut backlinks: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for edge in &graph.edges {
        outgoing
            .entry(edge.from_path.clone())
            .or_default()
            .insert(edge.to_path.clone());
        backlinks
            .entry(edge.to_path.clone())
            .or_default()
            .insert(edge.from_path.clone());
    }
    for (path, node) in &mut graph.nodes {
        let out = outgoing.get(path).cloned().unwrap_or_default();
        let back = backlinks.get(path).cloned().unwrap_or_default();
        let total = out.union(&back).count();
        node.outgoing_degree = out.len().try_into().unwrap_or(u32::MAX);
        node.backlink_degree = back.len().try_into().unwrap_or(u32::MAX);
        node.total_degree = total.try_into().unwrap_or(u32::MAX);
    }
}

fn parse_string_vec(raw: &str) -> Vec<String> {
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Array(values)) => values
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn is_structural_page(path: &str) -> bool {
    let Some(file_name) = std::path::Path::new(path)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
    else {
        return false;
    };
    let lower = file_name.to_ascii_lowercase();
    matches!(lower.as_str(), "index.md" | "readme.md" | "overview.md")
        || lower.ends_with("_index.md")
        || lower.ends_with("-index.md")
}

pub(super) fn clean_source_reference(from_path: &str, value: &str) -> String {
    let trimmed = value.trim();
    let without_link = trimmed
        .strip_prefix("[[")
        .and_then(|inner| inner.strip_suffix("]]"))
        .unwrap_or(trimmed);
    let without_alias = without_link
        .split_once('|')
        .map_or(without_link, |(target, _alias)| target);
    let target = without_alias
        .split_once('#')
        .map_or(without_alias, |(target, _heading)| target)
        .trim();
    if target.is_empty() {
        return String::new();
    }
    if target.contains("://") {
        return target.to_string();
    }
    if has_markdown_extension(target) || target.contains('/') {
        return target.replace('\\', "/");
    }
    let Some(parent) = std::path::Path::new(from_path).parent() else {
        return format!("{target}.md");
    };
    let joined = parent.join(format!("{target}.md"));
    joined.to_string_lossy().replace('\\', "/")
}

fn has_markdown_extension(target: &str) -> bool {
    std::path::Path::new(target)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}
