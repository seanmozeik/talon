use super::*;
use crate::config::{
    ChunkerConfig, ExpansionConfig, InferenceConfig, InferenceModels, RerankConfig, Scope,
    ScopeGlob, ScopePriority, ScopesConfig, SearchConfig, TalonConfig,
};
use crate::indexing::migrations::run_migrations;
use rusqlite::{Connection, params};
use std::path::PathBuf;

fn test_config_with_scopes(scopes: Vec<(&str, &str)>) -> TalonConfig {
    let mut map = ScopesConfig::new();
    for (name, glob) in scopes {
        map.insert(
            name.to_string(),
            Scope {
                glob: ScopeGlob::Single(glob.to_string()),
                priority: ScopePriority::Normal,
                default: true,
                lint: true,
            },
        );
    }
    TalonConfig {
        vault_path: PathBuf::from("/vault"),
        db_path: PathBuf::from("/vault/.talon/index.db"),
        config_file_path: None,
        include_patterns: Vec::new(),
        ignore_patterns: Vec::new(),
        inference: InferenceConfig {
            base_url: "http://localhost:8080".to_string(),
            models: InferenceModels {
                query_embedding: "q".to_string(),
                query_embedding_context_tokens: 512,
                document_embedding: "d".to_string(),
                chunk_embedding: "c".to_string(),
                reranker: "r".to_string(),
                reranker_context_tokens: 512,
            },
            rerank: RerankConfig::default(),
        },
        expansion: ExpansionConfig {
            provider: "openai-compatible".to_string(),
            base_url: "http://localhost:8080".to_string(),
            model: "x".to_string(),
            context_tokens: 32768,
            max_output_tokens: None,
        },
        ask: crate::config::AskConfig::default(),
        mcp: crate::config::McpConfig::default(),
        scopes: map,
        search: SearchConfig::default(),
        lint: crate::config::LintConfig::default(),
        chunker: ChunkerConfig::default(),
    }
}

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
        scope_all: false,
    }
}

fn lint_input_scoped(check: LintCheck, scope_only: Vec<String>) -> LintInput {
    LintInput {
        check,
        scope: Vec::new(),
        scope_only,
        scope_all: false,
    }
}

fn test_config_with_ignore(ignore_patterns: Vec<String>) -> TalonConfig {
    TalonConfig {
        ignore_patterns,
        ..test_config_with_scopes(Vec::new())
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
    let resp = query_lint(&conn, &lint_input(LintCheck::All), None);
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

    let resp = query_lint(&conn, &lint_input(LintCheck::Orphans), None);
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

    let resp = query_lint(&conn, &lint_input(LintCheck::BrokenLinks), None);
    assert_eq!(resp.findings.len(), 1);
    assert_eq!(resp.findings[0].path.as_str(), "Lifecycle/Doomed.md");
    assert!(resp.findings[0].message.contains("Nonexistent"));
}

#[test]
fn test_broken_links_ignores_targets_matching_ignore_patterns() {
    let conn = fresh_db();
    insert_note(&conn, "Graph/Source.md");
    insert_link(&conn, "Graph/Source.md", "CLAUDE.md", "[[CLAUDE]]");
    insert_link(&conn, "Graph/Source.md", "PURPOSE.md", "[[PURPOSE]]");
    insert_link(&conn, "Graph/Source.md", "RealTarget.md", "[[RealTarget]]");

    let config = test_config_with_ignore(vec!["CLAUDE.md".into(), "PURPOSE.md".into()]);
    let resp = query_lint(&conn, &lint_input(LintCheck::BrokenLinks), Some(&config));

    // Links to ignored files are NOT broken; only the real missing target is.
    assert_eq!(resp.findings.len(), 1);
    assert!(resp.findings[0].message.contains("RealTarget"));
    assert!(!resp.findings[0].message.contains("CLAUDE"));
    assert!(!resp.findings[0].message.contains("PURPOSE"));
}

#[test]
fn test_dangling_refs_detects_missing_frontmatter_paths() {
    let conn = fresh_db();
    insert_note(&conn, "Atlas/Node.md");
    let node_id = last_insert_id(&conn);
    insert_note(&conn, "Atlas/Real.md");

    insert_fm_field(&conn, node_id, "sources", "Atlas/Real.md");
    insert_fm_field(&conn, node_id, "sources", "Atlas/Ghost.md");

    let resp = query_lint(&conn, &lint_input(LintCheck::DanglingRefs), None);
    assert_eq!(resp.findings.len(), 1);
    assert_eq!(resp.findings[0].path.as_str(), "Atlas/Node.md");
    assert!(resp.findings[0].message.contains("Ghost.md"));
}

#[test]
fn test_dangling_refs_ignores_targets_matching_ignore_patterns() {
    let conn = fresh_db();
    insert_note(&conn, "Atlas/Node.md");
    let node_id = last_insert_id(&conn);

    insert_fm_field(&conn, node_id, "sources", "CLAUDE.md");
    insert_fm_field(&conn, node_id, "sources", "Ghost.md");

    let config = test_config_with_ignore(vec!["CLAUDE.md".into()]);
    let resp = query_lint(&conn, &lint_input(LintCheck::DanglingRefs), Some(&config));

    // Frontmatter to ignored file is NOT dangling; only the real missing one.
    assert_eq!(resp.findings.len(), 1);
    assert!(resp.findings[0].message.contains("Ghost.md"));
    assert!(!resp.findings[0].message.contains("CLAUDE"));
}

#[test]
fn test_unreferenced_requires_both_no_incoming_and_no_outgoing() {
    let conn = fresh_db();
    insert_note(&conn, "Search/Isolated.md");
    insert_note(&conn, "Search/Linker.md");
    insert_note(&conn, "Search/Target.md");
    insert_link(&conn, "Search/Linker.md", "Search/Target.md", "[[Target]]");

    let resp = query_lint(&conn, &lint_input(LintCheck::Unreferenced), None);
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

    let config = test_config_with_scopes(vec![("atlas", "Atlas/**"), ("graph", "Graph/**")]);
    let resp = query_lint(
        &conn,
        &lint_input_scoped(LintCheck::Orphans, vec!["atlas".to_string()]),
        Some(&config),
    );
    let paths: Vec<&str> = resp.findings.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"Atlas/A.md"), "Atlas/A should appear");
    assert!(
        !paths.contains(&"Graph/B.md"),
        "Graph/B filtered out by scope"
    );
}
