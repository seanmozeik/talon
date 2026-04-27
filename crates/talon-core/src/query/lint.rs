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
        vault: None,
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
    let Ok(paths): rusqlite::Result<Vec<_>> = rows.collect() else {
        return Vec::new();
    };
    paths
        .into_iter()
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
    let Ok(links): rusqlite::Result<Vec<_>> = rows.collect() else {
        return Vec::new();
    };
    links
        .into_iter()
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
    let Ok(references): rusqlite::Result<Vec<_>> = rows.collect() else {
        return Vec::new();
    };
    references
        .into_iter()
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
    let Ok(paths): rusqlite::Result<Vec<_>> = rows.collect() else {
        return Vec::new();
    };
    paths
        .into_iter()
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
mod tests;
