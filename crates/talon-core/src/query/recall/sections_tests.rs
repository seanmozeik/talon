use std::collections::HashSet;

use rusqlite::{Connection, params};

use super::sections::build_linked_context;
use crate::indexing::migrations::run_migrations;
use crate::query::RecallInput;
use crate::search::types::{RawSearchResult, SearchScores};

#[test]
fn linked_context_includes_shared_source_graph_candidates() -> Result<(), Box<dyn std::error::Error>>
{
    let mut conn = Connection::open_in_memory()?;
    run_migrations(&mut conn)?;
    insert_graph_node(&conn, "Source.md", "Source", None)?;
    insert_graph_node(&conn, "Shared.md", "Shared", None)?;
    insert_graph_node(&conn, "Other.md", "Other", None)?;
    insert_graph_node(&conn, "Anchor.md", "Anchor", None)?;
    set_graph_node_sources(&conn, "Source.md", r#"["Evidence.md"]"#)?;
    set_graph_node_sources(&conn, "Shared.md", r#"["Evidence.md"]"#)?;
    insert_graph_source(&conn, "Evidence.md", "Source.md")?;
    insert_graph_source(&conn, "Evidence.md", "Shared.md")?;
    insert_graph_edge(&conn, "Other.md", "Anchor.md", 1)?;

    let (linked, raw_count) = build_linked_context(
        &conn,
        &[raw_result("Source.md", 0.8)],
        &RecallInput::default(),
        &HashSet::new(),
        None,
    );

    assert_eq!(raw_count, 1);
    assert_eq!(linked[0].vault_path.as_str(), "Shared.md");
    Ok(())
}

#[test]
fn linked_context_caps_each_source_per_community() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = Connection::open_in_memory()?;
    run_migrations(&mut conn)?;
    insert_graph_node(&conn, "Source.md", "Source", None)?;
    for path in ["C1.md", "C2.md", "C3.md", "C4.md"] {
        insert_graph_node(&conn, path, path, Some(7))?;
        insert_graph_edge(&conn, "Source.md", path, 2)?;
    }
    insert_graph_node(&conn, "Other.md", "Other", Some(8))?;
    insert_graph_edge(&conn, "Source.md", "Other.md", 1)?;

    let (linked, raw_count) = build_linked_context(
        &conn,
        &[raw_result("Source.md", 0.9)],
        &RecallInput::default(),
        &HashSet::new(),
        None,
    );
    let paths = linked
        .iter()
        .map(|note| note.vault_path.as_str())
        .collect::<Vec<_>>();

    assert_eq!(raw_count, 4);
    assert_eq!(paths.iter().filter(|path| path.starts_with('C')).count(), 3);
    assert!(paths.contains(&"Other.md"));
    Ok(())
}

#[test]
fn linked_context_ignores_marginal_active_sources() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = Connection::open_in_memory()?;
    run_migrations(&mut conn)?;
    insert_graph_node(&conn, "WeakSource.md", "WeakSource", None)?;
    insert_graph_node(&conn, "Hub.md", "Hub", None)?;
    insert_graph_edge(&conn, "WeakSource.md", "Hub.md", 4)?;

    let (linked, raw_count) = build_linked_context(
        &conn,
        &[raw_result("WeakSource.md", 0.4)],
        &RecallInput::default(),
        &HashSet::new(),
        None,
    );

    assert_eq!(raw_count, 0);
    assert!(linked.is_empty());
    Ok(())
}

fn raw_result(path: &str, score: f64) -> RawSearchResult {
    RawSearchResult {
        path: path.into(),
        title: path.into(),
        tags: Vec::new(),
        aliases: Vec::new(),
        snippet: String::new(),
        score,
        scores: SearchScores::default(),
        semantic_heading: None,
        semantic_char_start: None,
        semantic_char_end: None,
    }
}

fn insert_graph_node(
    conn: &Connection,
    path: &str,
    title: &str,
    community_id: Option<u32>,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO graph_nodes (
           vault_path, title, aliases, tags, scope, note_type, sources,
           outgoing_degree, backlink_degree, total_degree, structural,
           community_id, community_cohesion, community_neighbor_count, bridge_weight
         ) VALUES (?1, ?2, '[]', '[]', '', NULL, '[]', 0, 0, 0, 0, ?3, 0.0, 0, 0.0)",
        params![path, title, community_id],
    )?;
    Ok(())
}

fn insert_graph_edge(
    conn: &Connection,
    from_path: &str,
    to_path: &str,
    weight: u32,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO graph_edges (from_path, to_path, link_text, weight)
         VALUES (?1, ?2, ?2, ?3)",
        params![from_path, to_path, weight],
    )?;
    Ok(())
}

fn insert_graph_source(conn: &Connection, source: &str, path: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO graph_sources (source, path) VALUES (?1, ?2)",
        params![source, path],
    )?;
    Ok(())
}

fn set_graph_node_sources(
    conn: &Connection,
    path: &str,
    sources: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE graph_nodes SET sources = ?2 WHERE vault_path = ?1",
        params![path, sources],
    )?;
    Ok(())
}
