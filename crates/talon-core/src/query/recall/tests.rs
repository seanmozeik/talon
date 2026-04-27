use rusqlite::{Connection, params};

use super::budget::trim_to_budget;
use super::*;
use crate::contracts::VaultPath;
use crate::indexing::migrations::run_migrations;
use crate::query::{EditedNote, FrontmatterFact, FuzzyAnchor, LinkedNote, NoteExcerpt};

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
        },
        NoteExcerpt {
            vault_path: VaultPath::parse("B.md").unwrap(),
            title: "B".to_string(),
            snippet: "b".repeat(50),
            score: 0.5,
            rank: 2,
        },
    ];
    let mut active_mut = active;
    let mut linked: Vec<LinkedNote> = Vec::new();
    let mut fm: Vec<FrontmatterFact> = Vec::new();
    let mut edits: Vec<EditedNote> = Vec::new();
    let mut anchors: Vec<FuzzyAnchor> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();

    trim_to_budget(
        1,
        &mut active_mut,
        &mut linked,
        &mut fm,
        &mut edits,
        &mut anchors,
        &mut dropped,
    );

    assert!(
        !dropped.is_empty(),
        "budget trimmer must populate excluded_by_budget"
    );
}
