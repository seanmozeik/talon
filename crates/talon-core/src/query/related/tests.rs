use rusqlite::Connection;

use super::*;
use crate::indexing::migrations::run_migrations;
use crate::search::Direction;

fn fresh_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    run_migrations(&mut conn).unwrap();
    conn
}

fn insert_note(conn: &Connection, vault_path: &str, title: &str) {
    conn.execute(
        "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active) \
         VALUES (?, ?, '[]', '[]', '', 0, 0, 'h', 'd', 1)",
        params![vault_path, title],
    )
    .unwrap();
}

fn insert_link(conn: &Connection, from: &str, to: &str, raw_target: &str) {
    conn.execute(
        "INSERT OR IGNORE INTO links (from_path, to_path, raw_target) VALUES (?, ?, ?)",
        params![from, to, raw_target],
    )
    .unwrap();
}

fn related_input(path: &str, depth: u8, direction: Direction) -> RelatedInput {
    RelatedInput {
        path: path.to_string(),
        depth,
        direction,
        scope: Vec::new(),
        scope_only: Vec::new(),
        scope_all: false,
    }
}

fn make_graph(conn: &Connection) {
    insert_note(conn, "Graph/Parent.md", "Parent");
    insert_note(conn, "Graph/Child.md", "Child");
    insert_note(conn, "Graph/Grandchild.md", "Grandchild");
    insert_link(conn, "Graph/Parent.md", "Graph/Child.md", "[[Child]]");
    insert_link(
        conn,
        "Graph/Child.md",
        "Graph/Grandchild.md",
        "[[Grandchild]]",
    );
}

#[test]
fn outgoing_depth1_returns_direct_links() {
    let conn = fresh_db();
    make_graph(&conn);

    let resp = find_related(
        &conn,
        &related_input("Graph/Parent.md", 1, Direction::Outgoing),
        None,
    );

    assert_eq!(resp.results.len(), 1);
    assert_eq!(resp.results[0].vault_path.as_str(), "Graph/Child.md");
    assert_eq!(resp.results[0].relation, RelationKind::Outgoing);
    assert_eq!(resp.results[0].title, "Child");
}

#[test]
fn outgoing_depth2_returns_transitive_links() {
    let conn = fresh_db();
    make_graph(&conn);

    let resp = find_related(
        &conn,
        &related_input("Graph/Parent.md", 2, Direction::Outgoing),
        None,
    );

    let paths: Vec<&str> = resp.results.iter().map(|r| r.vault_path.as_str()).collect();
    assert!(
        paths.contains(&"Graph/Child.md"),
        "depth-2 must include direct link"
    );
    assert!(
        paths.contains(&"Graph/Grandchild.md"),
        "depth-2 must include transitive link"
    );
    assert_eq!(paths.len(), 2);
}

#[test]
fn backlinks_depth1_returns_inbound_links() {
    let conn = fresh_db();
    make_graph(&conn);

    let resp = find_related(
        &conn,
        &related_input("Graph/Child.md", 1, Direction::Backlinks),
        None,
    );

    assert_eq!(resp.results.len(), 1);
    assert_eq!(resp.results[0].vault_path.as_str(), "Graph/Parent.md");
    assert_eq!(resp.results[0].relation, RelationKind::Backlink);
}

#[test]
fn both_direction_returns_outgoing_and_backlinks() {
    let conn = fresh_db();
    make_graph(&conn);

    let resp = find_related(
        &conn,
        &related_input("Graph/Child.md", 1, Direction::Both),
        None,
    );

    let paths: Vec<&str> = resp.results.iter().map(|r| r.vault_path.as_str()).collect();
    assert!(paths.contains(&"Graph/Parent.md"), "must include backlink");
    assert!(
        paths.contains(&"Graph/Grandchild.md"),
        "must include outgoing link"
    );
    assert_eq!(paths.len(), 2);
}

#[test]
fn orphan_note_returns_empty_results() {
    let conn = fresh_db();
    insert_note(&conn, "Orphan.md", "Orphan");

    let resp = find_related(&conn, &related_input("Orphan.md", 3, Direction::Both), None);

    assert!(
        resp.results.is_empty(),
        "note with no links must return empty results"
    );
}
