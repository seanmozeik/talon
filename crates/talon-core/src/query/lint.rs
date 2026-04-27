//! Lint checks for vault graph health.

use crate::contracts::VaultPath;
use crate::indexing::{LintCheck, LintFinding, LintInput, LintResponse};
use rusqlite::Connection;

pub fn query_lint(conn: &Connection, input: &LintInput) -> LintResponse {
    let findings = match input.check {
        LintCheck::All => find_all(conn, &input.scope_only),
        LintCheck::Orphans => find_orphans(conn, &input.scope_only),
        LintCheck::BrokenLinks => find_broken_links(conn, &input.scope_only),
        LintCheck::DanglingRefs => find_dangling_refs(conn, &input.scope_only),
        LintCheck::Unreferenced => find_unreferenced(conn, &input.scope_only),
    };
    LintResponse {
        check: input.check,
        findings,
    }
}

fn find_all(conn: &Connection, scope_only: &[String]) -> Vec<LintFinding> {
    let mut findings = find_orphans(conn, scope_only);
    findings.extend(find_broken_links(conn, scope_only));
    findings.extend(find_dangling_refs(conn, scope_only));
    findings.extend(find_unreferenced(conn, scope_only));
    findings
}

fn passes_scope_filter(path: &str, scope_only: &[String]) -> bool {
    scope_only.is_empty() || scope_only.iter().any(|s| path.starts_with(s.as_str()))
}

fn find_orphans(conn: &Connection, scope_only: &[String]) -> Vec<LintFinding> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT vault_path FROM notes \
         WHERE active = 1 \
         AND vault_path NOT IN (SELECT DISTINCT to_path FROM links) \
         ORDER BY vault_path",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok)
        .filter(|p| passes_scope_filter(p, scope_only))
        .filter_map(|path| {
            VaultPath::parse(&path).ok().map(|vp| LintFinding {
                check: LintCheck::Orphans,
                path: vp,
                message: "no incoming links".to_string(),
                line: None,
            })
        })
        .collect()
}

fn find_broken_links(conn: &Connection, scope_only: &[String]) -> Vec<LintFinding> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT DISTINCT from_path, to_path, raw_target FROM links \
         WHERE to_path NOT IN (SELECT vault_path FROM notes WHERE active = 1) \
         ORDER BY from_path, to_path",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        ))
    }) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok)
        .filter(|(from, _, _)| passes_scope_filter(from, scope_only))
        .filter_map(|(from, to, raw)| {
            let display = if raw.is_empty() { to.clone() } else { raw };
            VaultPath::parse(&from).ok().map(|vp| LintFinding {
                check: LintCheck::BrokenLinks,
                path: vp,
                message: format!("broken link: {display} → {to} (not found)"),
                line: None,
            })
        })
        .collect()
}

fn find_dangling_refs(conn: &Connection, scope_only: &[String]) -> Vec<LintFinding> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT n.vault_path, f.field, f.value \
         FROM notes n \
         JOIN note_frontmatter_fields f ON f.note_id = n.id \
         WHERE n.active = 1 \
         AND f.value LIKE '%.md' \
         AND f.value NOT IN (SELECT vault_path FROM notes WHERE active = 1) \
         ORDER BY n.vault_path, f.field",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    }) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok)
        .filter(|(path, _, _)| passes_scope_filter(path, scope_only))
        .filter_map(|(path, field, value)| {
            VaultPath::parse(&path).ok().map(|vp| LintFinding {
                check: LintCheck::DanglingRefs,
                path: vp,
                message: format!("dangling ref: {field}: {value} (not found)"),
                line: None,
            })
        })
        .collect()
}

fn find_unreferenced(conn: &Connection, scope_only: &[String]) -> Vec<LintFinding> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT vault_path FROM notes \
         WHERE active = 1 \
         AND vault_path NOT IN (SELECT DISTINCT to_path FROM links) \
         AND vault_path NOT IN (SELECT DISTINCT from_path FROM links) \
         ORDER BY vault_path",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok)
        .filter(|p| passes_scope_filter(p, scope_only))
        .filter_map(|path| {
            VaultPath::parse(&path).ok().map(|vp| LintFinding {
                check: LintCheck::Unreferenced,
                path: vp,
                message: "no incoming or outgoing links".to_string(),
                line: None,
            })
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::indexing::migrations::run_migrations;
    use rusqlite::Connection;
    use rusqlite::params;

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
        // Graph/Child has incoming link (from Parent); Graph/Grandchild has none
        insert_note(&conn, "Graph/Parent.md");
        insert_note(&conn, "Graph/Child.md");
        insert_note(&conn, "Graph/Grandchild.md");
        // Parent → Child (Child has incoming link, not orphan)
        insert_link(&conn, "Graph/Parent.md", "Graph/Child.md", "[[Child]]");
        // Grandchild has no incoming links → orphan

        let resp = query_lint(&conn, &lint_input(LintCheck::Orphans));
        let paths: Vec<&str> = resp.findings.iter().map(|f| f.path.as_str()).collect();
        // Parent and Grandchild are orphans; Child is not
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
        // Doomed links to a note that doesn't exist
        insert_link(
            &conn,
            "Lifecycle/Doomed.md",
            "Lifecycle/Nonexistent.md",
            "[[Nonexistent]]",
        );
        // Alive links to Doomed (which exists) — not broken
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

        // sources: Real.md → exists, not dangling
        insert_fm_field(&conn, node_id, "sources", "Atlas/Real.md");
        // sources: Ghost.md → doesn't exist → dangling
        insert_fm_field(&conn, node_id, "sources", "Atlas/Ghost.md");

        let resp = query_lint(&conn, &lint_input(LintCheck::DanglingRefs));
        assert_eq!(resp.findings.len(), 1);
        assert_eq!(resp.findings[0].path.as_str(), "Atlas/Node.md");
        assert!(resp.findings[0].message.contains("Ghost.md"));
    }

    #[test]
    fn test_unreferenced_requires_both_no_incoming_and_no_outgoing() {
        let conn = fresh_db();
        // Isolated: no links in or out → unreferenced
        insert_note(&conn, "Search/Isolated.md");
        // Has outgoing link → NOT unreferenced
        insert_note(&conn, "Search/Linker.md");
        insert_note(&conn, "Search/Target.md");
        insert_link(&conn, "Search/Linker.md", "Search/Target.md", "[[Target]]");
        // Target has incoming link → NOT unreferenced

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
        // Both are orphans (no incoming links), but scope filter limits to Atlas/

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
}
