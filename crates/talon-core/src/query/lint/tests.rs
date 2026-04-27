use super::*;
use crate::indexing::migrations::run_migrations;
use rusqlite::{Connection, params};

fn fresh_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    run_migrations(&mut conn).unwrap();
    conn
}

fn insert_note(conn: &Connection, vault_path: &str) {
    conn.execute(
        "INSERT INTO notes \
         (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active) \
         VALUES (?, '', '[]', '[]', '', 0, 0, 'h', 'd', 1)",
        params![vault_path],
    )
    .unwrap();
}

fn insert_link(conn: &Connection, from: &str, to: &str, raw: &str) {
    conn.execute(
        "INSERT OR IGNORE INTO links (from_path, to_path, raw_target) VALUES (?, ?, ?)",
        params![from, to, raw],
    )
    .unwrap();
}

fn insert_fm_field(conn: &Connection, note_id: i64, field: &str, value: &str) {
    conn.execute(
        "INSERT INTO note_frontmatter_fields \
         (note_id, field, value, value_norm) VALUES (?, ?, ?, ?)",
        params![note_id, field, value, value.to_lowercase()],
    )
    .unwrap();
}

fn last_insert_id(conn: &Connection) -> i64 {
    conn.last_insert_rowid()
}

fn lint_input(check: LintCheck) -> LintInput {
    LintInput {
        check,
        scope: Vec::new(),
        scope_only: Vec::new(),
    }
}

fn lint_input_scoped(check: LintCheck, scope_only: Vec<String>) -> LintInput {
    LintInput {
        check,
        scope: Vec::new(),
        scope_only,
    }
}

#[test]
fn test_all_runs_every_lint_check() {
    let conn = fresh_db();
    insert_note(&conn, "Graph/Orphan.md");
    insert_note(&conn, "Graph/Source.md");
    let source_id = last_insert_id(&conn);
    insert_note(&conn, "Graph/Target.md");
    insert_link(&conn, "Graph/Source.md", "Graph/Target.md", "[[Target]]");
    insert_link(&conn, "Graph/Source.md", "Graph/Missing.md", "[[Missing]]");
    insert_fm_field(&conn, source_id, "sources", "Graph/Ghost.md");
    let resp = query_lint(&conn, &lint_input(LintCheck::All));
    let messages: Vec<&str> = resp.findings.iter().map(|f| f.message.as_str()).collect();

    assert!(messages.iter().any(|msg| msg.contains("no incoming links")));
    assert!(messages.iter().any(|msg| msg.contains("broken link")));
    assert!(messages.iter().any(|msg| msg.contains("dangling ref")));
    assert!(
        messages
            .iter()
            .any(|msg| msg.contains("no incoming or outgoing links"))
    );
}

#[test]
fn test_orphans_detects_notes_with_no_incoming_links() {
    let conn = fresh_db();
    insert_note(&conn, "Graph/Parent.md");
    insert_note(&conn, "Graph/Child.md");
    insert_note(&conn, "Graph/Grandchild.md");
    insert_link(&conn, "Graph/Parent.md", "Graph/Child.md", "[[Child]]");

    let resp = query_lint(&conn, &lint_input(LintCheck::Orphans));
    let paths: Vec<&str> = resp.findings.iter().map(|f| f.path.as_str()).collect();
    assert!(
        paths.contains(&"Graph/Grandchild.md"),
        "Grandchild should be orphan"
    );
    assert!(
        paths.contains(&"Graph/Parent.md"),
        "Parent should be orphan (no incoming)"
    );
    assert!(
        !paths.contains(&"Graph/Child.md"),
        "Child should NOT be orphan"
    );
}

#[test]
fn test_broken_links_detects_missing_targets() {
    let conn = fresh_db();
    insert_note(&conn, "Lifecycle/Doomed.md");
    insert_note(&conn, "Lifecycle/Alive.md");
    insert_link(
        &conn,
        "Lifecycle/Doomed.md",
        "Lifecycle/Nonexistent.md",
        "[[Nonexistent]]",
    );
    insert_link(
        &conn,
        "Lifecycle/Alive.md",
        "Lifecycle/Doomed.md",
        "[[Doomed]]",
    );

    let resp = query_lint(&conn, &lint_input(LintCheck::BrokenLinks));
    assert_eq!(resp.findings.len(), 1);
    assert_eq!(resp.findings[0].path.as_str(), "Lifecycle/Doomed.md");
    assert!(resp.findings[0].message.contains("Nonexistent"));
}

#[test]
fn test_dangling_refs_detects_missing_frontmatter_paths() {
    let conn = fresh_db();
    insert_note(&conn, "Atlas/Node.md");
    let node_id = last_insert_id(&conn);
    insert_note(&conn, "Atlas/Real.md");

    insert_fm_field(&conn, node_id, "sources", "Atlas/Real.md");
    insert_fm_field(&conn, node_id, "sources", "Atlas/Ghost.md");

    let resp = query_lint(&conn, &lint_input(LintCheck::DanglingRefs));
    assert_eq!(resp.findings.len(), 1);
    assert_eq!(resp.findings[0].path.as_str(), "Atlas/Node.md");
    assert!(resp.findings[0].message.contains("Ghost.md"));
}

#[test]
fn test_unreferenced_requires_both_no_incoming_and_no_outgoing() {
    let conn = fresh_db();
    insert_note(&conn, "Search/Isolated.md");
    insert_note(&conn, "Search/Linker.md");
    insert_note(&conn, "Search/Target.md");
    insert_link(&conn, "Search/Linker.md", "Search/Target.md", "[[Target]]");

    let resp = query_lint(&conn, &lint_input(LintCheck::Unreferenced));
    let paths: Vec<&str> = resp.findings.iter().map(|f| f.path.as_str()).collect();
    assert!(
        paths.contains(&"Search/Isolated.md"),
        "Isolated should be unreferenced"
    );
    assert!(
        !paths.contains(&"Search/Linker.md"),
        "Linker has outgoing, NOT unreferenced"
    );
    assert!(
        !paths.contains(&"Search/Target.md"),
        "Target has incoming, NOT unreferenced"
    );
}

#[test]
fn test_scope_filter_limits_orphan_findings() {
    let conn = fresh_db();
    insert_note(&conn, "Atlas/A.md");
    insert_note(&conn, "Graph/B.md");

    let resp = query_lint(
        &conn,
        &lint_input_scoped(LintCheck::Orphans, vec!["Atlas/".to_string()]),
    );
    let paths: Vec<&str> = resp.findings.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"Atlas/A.md"), "Atlas/A should appear");
    assert!(
        !paths.contains(&"Graph/B.md"),
        "Graph/B filtered out by scope"
    );
}
