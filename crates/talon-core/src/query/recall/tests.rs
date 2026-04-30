use rusqlite::{Connection, params};

use super::budget::trim_to_budget;
use super::*;
use crate::config::{
    ChunkerConfig, ExpansionConfig, InferenceConfig, InferenceModels, LintConfig, RerankConfig,
    Scope, ScopeGlob, ScopePriority, ScopesConfig, SearchConfig, TalonConfig,
};
use crate::contracts::VaultPath;
use crate::indexing::migrations::run_migrations;
use crate::query::{LinkedNote, NoteExcerpt};
use std::path::PathBuf;

fn fresh_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    run_migrations(&mut conn).unwrap();
    conn
}

fn insert_note(conn: &Connection, vault_path: &str, title: &str) {
    conn.execute(
            "INSERT INTO notes \
             (vault_path, title, tags, aliases, content, frontmatter, mtime_ms, size_bytes, hash, docid, active) \
             VALUES (?, ?, '[]', '[]', '', '{}', 1_000_000, 0, 'h', ?, 1)",
            params![vault_path, title, vault_path],
        )
        .unwrap();
}

fn insert_link(conn: &Connection, from: &str, to: &str) {
    conn.execute(
        "INSERT OR IGNORE INTO links (from_path, to_path, raw_target) VALUES (?, ?, ?)",
        params![from, to, to],
    )
    .unwrap();
}

fn recall_input(message: &str) -> RecallInput {
    RecallInput {
        message: message.to_string(),
        budget_tokens: 10_000,
        ..RecallInput::default()
    }
}

fn scoped_config() -> TalonConfig {
    let mut scopes = ScopesConfig::new();
    scopes.insert(
        "wiki".to_string(),
        Scope {
            glob: ScopeGlob::Single("wiki/**".to_string()),
            priority: ScopePriority::Boosted,
            default: true,
            lint: true,
        },
    );
    scopes.insert(
        "private".to_string(),
        Scope {
            glob: ScopeGlob::Single("private/**".to_string()),
            priority: ScopePriority::Buried,
            default: false,
            lint: false,
        },
    );

    TalonConfig {
        vault_path: PathBuf::from("/tmp/vault"),
        db_path: PathBuf::from("/tmp/vault/idx.sqlite"),
        config_file_path: None,
        include_patterns: Vec::new(),
        ignore_patterns: Vec::new(),
        inference: InferenceConfig {
            base_url: "http://localhost".to_string(),
            models: InferenceModels {
                query_embedding: "query".to_string(),
                query_embedding_context_tokens: 512,
                document_embedding: "document".to_string(),
                chunk_embedding: "chunk".to_string(),
                reranker: "reranker".to_string(),
                reranker_context_tokens: 512,
            },
            rerank: RerankConfig::default(),
        },
        expansion: ExpansionConfig {
            provider: "openai-compatible".to_string(),
            base_url: "http://localhost".to_string(),
            model: "expansion".to_string(),
            context_tokens: 32768,
            max_output_tokens: None,
        },
        ask: crate::config::AskConfig::default(),
        mcp: crate::config::McpConfig::default(),
        scopes,
        search: SearchConfig::default(),
        lint: LintConfig::default(),
        chunker: ChunkerConfig::default(),
    }
}

#[test]
fn empty_message_returns_skipped() {
    let conn = fresh_db();
    let result = run_recall(&conn, None, None, &recall_input("   "), None);
    assert!(result.skipped);
    assert_eq!(result.evidence_score, 0.0);
}

#[test]
fn no_results_returns_skipped() {
    let conn = fresh_db();
    let result = run_recall(&conn, None, None, &recall_input("nothing here"), None);
    assert!(result.skipped);
}

#[test]
fn default_false_scopes_are_excluded_from_recall_unless_scoped_in() {
    let conn = fresh_db();
    let config = scoped_config();
    insert_note(&conn, "wiki/Lease.md", "Lease Public");
    insert_note(&conn, "private/Lease.md", "Lease Private");

    let default_result = run_recall(&conn, None, None, &recall_input("Lease"), Some(&config));
    let default_paths: Vec<String> = default_result
        .vault_recall
        .as_ref()
        .into_iter()
        .flat_map(|recall| recall.active_notes.iter())
        .map(|note| note.vault_path.as_str().to_string())
        .collect();
    assert!(default_paths.iter().any(|path| path == "wiki/Lease.md"));
    assert!(!default_paths.iter().any(|path| path == "private/Lease.md"));

    let input = RecallInput {
        scope: vec!["private".to_string()],
        ..recall_input("Lease")
    };
    let scoped_result = run_recall(&conn, None, None, &input, Some(&config));
    let scoped_paths: Vec<String> = scoped_result
        .vault_recall
        .as_ref()
        .into_iter()
        .flat_map(|recall| recall.active_notes.iter())
        .map(|note| note.vault_path.as_str().to_string())
        .collect();
    assert!(scoped_paths.iter().any(|path| path == "private/Lease.md"));
}

#[test]
fn exclude_does_not_panic() {
    let conn = fresh_db();
    insert_note(&conn, "Atlas/Note.md", "Note");

    let input = RecallInput {
        message: "Note".to_string(),
        exclude: vec!["Atlas/Note.md".to_string()],
        budget_tokens: 10_000,
        ..RecallInput::default()
    };
    let result = run_recall(&conn, None, None, &input, None);
    // excluded path must not appear in active_notes
    if let Some(vr) = &result.vault_recall {
        for note in &vr.active_notes {
            assert_ne!(note.vault_path.as_str(), "Atlas/Note.md");
        }
    }
}

#[test]
fn linked_context_does_not_panic() {
    let conn = fresh_db();
    insert_note(&conn, "Hub.md", "Hub");
    insert_note(&conn, "Child.md", "Child");
    insert_link(&conn, "Hub.md", "Child.md");

    let result = run_recall(&conn, None, None, &recall_input("Hub"), None);
    assert!(result.excluded_by_budget.is_empty());
}

#[test]
fn budget_enforcement_populates_excluded_by_budget() {
    let active = vec![
        NoteExcerpt {
            vault_path: VaultPath::parse("A.md").unwrap(),
            title: "A".to_string(),
            snippet: "a".repeat(50),
            score: 1.0,
            rank: 1,
            mtime: String::new(),
        },
        NoteExcerpt {
            vault_path: VaultPath::parse("B.md").unwrap(),
            title: "B".to_string(),
            snippet: "b".repeat(50),
            score: 0.5,
            rank: 2,
            mtime: String::new(),
        },
    ];
    let mut active_mut = active;
    let mut linked: Vec<LinkedNote> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();

    trim_to_budget(1, &mut active_mut, &mut linked, &mut dropped);

    assert!(
        !dropped.is_empty(),
        "budget trimmer must populate excluded_by_budget"
    );
}
