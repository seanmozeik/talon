use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::graph::{GraphNode, GraphSnapshot, build_missing_link_suggestions};
use crate::indexing::migrations::run_migrations;

use super::suggest::line_mentions_term;

#[test]
fn suggestions_skip_existing_links_code_and_markdown() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = Connection::open_in_memory()?;
    run_migrations(&mut conn)?;
    insert_note(
        &conn,
        "Source.md",
        "Inline `Target Note` and [Target Note](https://example.com) are ignored.\n\
         Target Note should be linked here.\n\
         ```\nTarget Note ignored in fence\n```",
    )?;
    let mut snapshot = snapshot_with_targets(["Source.md", "Target.md"]);
    snapshot.edges.push(crate::graph::GraphEdge {
        from_path: "Other.md".into(),
        to_path: "Target.md".into(),
        link_text: "Target".into(),
        weight: 1,
    });

    let suggestions = build_missing_link_suggestions(&conn, &snapshot)?;

    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].path, "Source.md");
    assert_eq!(suggestions[0].target, "Target.md");
    assert_eq!(suggestions[0].line, Some(2));
    Ok(())
}

#[test]
fn suggestions_limit_one_per_target_per_source() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = Connection::open_in_memory()?;
    run_migrations(&mut conn)?;
    insert_note(&conn, "Source.md", "Target Note here.\nTarget Note again.")?;
    let snapshot = snapshot_with_targets(["Source.md", "Target.md"]);

    let suggestions = build_missing_link_suggestions(&conn, &snapshot)?;

    assert_eq!(suggestions.len(), 1);
    Ok(())
}

#[test]
fn suggestions_skip_basename_only_terms() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = Connection::open_in_memory()?;
    run_migrations(&mut conn)?;
    insert_note(
        &conn,
        "Source.md",
        "Review should not link by filename alone.",
    )?;
    let mut snapshot = snapshot_with_targets(["Source.md", "Review.md"]);
    set_title(&mut snapshot, "Review.md", "Editorial Pass")?;

    let suggestions = build_missing_link_suggestions(&conn, &snapshot)?;

    assert!(suggestions.is_empty());
    Ok(())
}

#[test]
fn suggestions_require_case_signal_for_lowercase_phrases() -> Result<(), Box<dyn std::error::Error>>
{
    let mut conn = Connection::open_in_memory()?;
    run_migrations(&mut conn)?;
    insert_note(
        &conn,
        "Source.md",
        "the silver ledger pattern is discussed here.",
    )?;
    let mut snapshot = snapshot_with_targets(["Source.md", "Silver Ledger.md"]);
    set_title(&mut snapshot, "Silver Ledger.md", "Silver Ledger")?;

    let suggestions = build_missing_link_suggestions(&conn, &snapshot)?;

    assert!(suggestions.is_empty());
    Ok(())
}

#[test]
fn short_single_word_terms_are_not_suggested() {
    let salient_terms = std::collections::BTreeSet::from(["mise".to_string()]);

    assert!(!line_mentions_term("Mise station", "mise", &salient_terms));
}

fn snapshot_with_targets(paths: impl IntoIterator<Item = &'static str>) -> GraphSnapshot {
    GraphSnapshot {
        nodes: paths
            .into_iter()
            .map(|path| (path.to_string(), node(path)))
            .collect::<BTreeMap<_, _>>(),
        ..GraphSnapshot::default()
    }
}

fn node(path: &str) -> GraphNode {
    GraphNode {
        vault_path: path.into(),
        title: if path == "Target.md" {
            "Target Note".into()
        } else {
            path.into()
        },
        aliases: Vec::new(),
        tags: Vec::new(),
        scope: String::new(),
        note_type: None,
        sources: Vec::new(),
        outgoing_degree: 0,
        backlink_degree: 0,
        total_degree: 0,
        structural: false,
        community_id: None,
        community_cohesion: 0.0,
        community_neighbor_count: 0,
        bridge_weight: 0.0,
    }
}

fn set_title(
    snapshot: &mut GraphSnapshot,
    path: &str,
    title: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(node) = snapshot.nodes.get_mut(path) else {
        return Err(format!("missing test node {path}").into());
    };
    node.title = title.into();
    Ok(())
}

fn insert_note(conn: &Connection, path: &str, content: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO notes (
           vault_path, title, tags, aliases, content, frontmatter,
           mtime_ms, size_bytes, hash, docid, active, scope
         ) VALUES (?1, ?1, '[]', '[]', ?2, '', 0, 0, ?1, ?1, 1, '')",
        params![path, content],
    )?;
    Ok(())
}
