use rusqlite::{Connection, params};

use crate::graph::build::{GraphBuildInput, clean_source_reference, rebuild_graph};
use crate::graph::{GraphSnapshot, load_graph_snapshot, update_graph_incremental};
use crate::indexing::migrations::run_migrations;

#[test]
fn rebuild_graph_filters_unresolved_and_inactive_links() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = Connection::open_in_memory()?;
    run_migrations(&mut conn)?;
    insert_note(&conn, 1, "Graph/Source.md", 1)?;
    insert_note(&conn, 2, "Graph/Target.md", 1)?;
    insert_note(&conn, 3, "Graph/Inactive.md", 0)?;
    insert_link(&conn, "Graph/Source.md", "Graph/Target.md")?;
    insert_link(&conn, "Graph/Source.md", "Graph/Missing.md")?;
    insert_link(&conn, "Graph/Source.md", "Graph/Inactive.md")?;

    let stats = rebuild_graph(&mut conn, &GraphBuildInput)?;
    assert_eq!(stats.node_count, 2);
    assert_eq!(stats.edge_count, 1);

    let snapshot = load_graph_snapshot(&conn)?;
    assert!(snapshot.nodes.contains_key("Graph/Source.md"));
    assert!(!snapshot.nodes.contains_key("Graph/Inactive.md"));
    assert_eq!(snapshot.edges.len(), 1);
    assert_eq!(snapshot.edges[0].to_path, "Graph/Target.md");
    Ok(())
}

#[test]
fn rebuild_graph_persists_source_map_and_structural_detection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut conn = Connection::open_in_memory()?;
    run_migrations(&mut conn)?;
    insert_note(&conn, 1, "Graph/Index.md", 1)?;
    insert_frontmatter(&conn, 1, "sources", "[[Book One|book]]")?;
    insert_frontmatter(&conn, 1, "type", "index")?;

    let stats = rebuild_graph(&mut conn, &GraphBuildInput)?;
    assert_eq!(stats.source_count, 1);

    let snapshot = load_graph_snapshot(&conn)?;
    let Some(node) = snapshot.nodes.get("Graph/Index.md") else {
        panic!("missing graph node");
    };
    assert!(node.structural);
    assert_eq!(node.note_type.as_deref(), Some("index"));
    assert_eq!(node.sources, vec!["Graph/Book One.md"]);
    let Some(citations) = snapshot.source_citations.get("Graph/Book One.md") else {
        panic!("missing source citations");
    };
    assert_eq!(citations.len(), 1);
    Ok(())
}

#[test]
fn source_reference_cleanup_handles_obsidian_links_and_urls() {
    assert_eq!(
        clean_source_reference("Notes/Current.md", "[[Book#Chapter|label]]"),
        "Notes/Book.md"
    );
    assert_eq!(
        clean_source_reference("Notes/Current.md", "https://example.com/a"),
        "https://example.com/a"
    );
}

#[test]
fn incremental_update_matches_full_rebuild() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = Connection::open_in_memory()?;
    run_migrations(&mut conn)?;
    insert_note(&conn, 1, "Graph/Source.md", 1)?;
    insert_note(&conn, 2, "Graph/Old.md", 1)?;
    insert_note(&conn, 3, "Graph/New.md", 1)?;
    insert_link(&conn, "Graph/Source.md", "Graph/Old.md")?;
    insert_frontmatter(&conn, 1, "sources", "[[Original Book]]")?;
    rebuild_graph(&mut conn, &GraphBuildInput)?;

    conn.execute(
        "UPDATE notes SET title = 'Updated Source', aliases = '[\"S\"]' WHERE id = 1",
        [],
    )?;
    conn.execute("DELETE FROM links WHERE from_path = 'Graph/Source.md'", [])?;
    insert_link(&conn, "Graph/Source.md", "Graph/New.md")?;
    conn.execute("DELETE FROM note_frontmatter_fields WHERE note_id = 1", [])?;
    insert_frontmatter(&conn, 1, "sources", "[[Updated Book]]")?;
    insert_frontmatter(&conn, 1, "type", "concept")?;

    update_graph_incremental(&mut conn, &[String::from("Graph/Source.md")])?;
    let incremental = load_graph_snapshot(&conn)?;
    rebuild_graph(&mut conn, &GraphBuildInput)?;
    let full = load_graph_snapshot(&conn)?;

    assert_snapshots_match_ignoring_built_at(&incremental, &full);
    Ok(())
}

fn assert_snapshots_match_ignoring_built_at(left: &GraphSnapshot, right: &GraphSnapshot) {
    assert_eq!(left.db_version, right.db_version);
    assert_eq!(left.nodes, right.nodes);
    assert_eq!(left.edges, right.edges);
    assert_eq!(left.source_citations, right.source_citations);
}

fn insert_note(conn: &Connection, id: i64, path: &str, active: u8) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO notes (
           id, vault_path, title, tags, aliases, content, frontmatter,
           mtime_ms, size_bytes, hash, docid, active, scope
         ) VALUES (?1, ?2, ?3, '[]', '[]', '', '', 0, 0, ?4, ?5, ?6, '')",
        params![
            id,
            path,
            path,
            format!("hash-{id}"),
            format!("docid-{id}"),
            active
        ],
    )?;
    Ok(())
}

fn insert_link(conn: &Connection, from: &str, to: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO links (from_path, to_path, raw_target, heading, alias)
         VALUES (?1, ?2, ?2, NULL, NULL)",
        params![from, to],
    )?;
    Ok(())
}

fn insert_frontmatter(
    conn: &Connection,
    note_id: i64,
    field: &str,
    value: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO note_frontmatter_fields (note_id, field, value, value_type, value_norm)
         VALUES (?1, ?2, ?3, 'string', ?3)",
        params![note_id, field, value],
    )?;
    Ok(())
}
