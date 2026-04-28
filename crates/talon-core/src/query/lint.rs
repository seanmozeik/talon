//! Lint checks for vault graph health.

use crate::config::{ScopeFilter, TalonConfig};
use crate::contracts::VaultPath;
use crate::indexing::{LintCheck, LintFinding, LintInput, LintResponse};
use crate::sync::relink_unresolved;
use rusqlite::Connection;

pub fn query_lint(
    conn: &Connection,
    input: &LintInput,
    config: Option<&TalonConfig>,
) -> LintResponse {
    // Re-resolve any stale links before reporting. Sync also does this, but
    // running it here too means lint output is fresh regardless of whether
    // sync was the last operation. Cost: one SELECT plus an UPDATE per
    // newly-resolvable link — bounded by current broken-link count, so
    // O(0) on healthy vaults and small even on broken ones.
    let _ = relink_unresolved(conn);

    let filter = config.map(|cfg| {
        ScopeFilter::from_args(cfg, &input.scope, &input.scope_only, input.scope_all)
            .unwrap_or_else(|_| ScopeFilter::default_for(cfg))
    });
    let findings = match input.check {
        LintCheck::All => find_all(conn, filter.as_ref()),
        LintCheck::Orphans => find_orphans(conn, filter.as_ref()),
        LintCheck::BrokenLinks => find_broken_links(conn, filter.as_ref()),
        LintCheck::DanglingRefs => find_dangling_refs(conn, filter.as_ref()),
        LintCheck::Unreferenced => find_unreferenced(conn, filter.as_ref()),
    };
    let findings = match config {
        Some(cfg) => findings
            .into_iter()
            .filter(|f| !cfg.lint_excluded(std::path::Path::new(f.path.as_str())))
            .collect(),
        None => findings,
    };
    LintResponse {
        vault: None,
        check: input.check,
        findings,
    }
}

fn find_all(conn: &Connection, filter: Option<&ScopeFilter<'_>>) -> Vec<LintFinding> {
    let mut findings = find_orphans(conn, filter);
    findings.extend(find_broken_links(conn, filter));
    findings.extend(find_dangling_refs(conn, filter));
    findings.extend(find_unreferenced(conn, filter));
    findings
}

fn find_orphans(conn: &Connection, filter: Option<&ScopeFilter<'_>>) -> Vec<LintFinding> {
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
        .filter(|p| filter.is_none_or(|f| f.accepts(p)))
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

fn find_broken_links(conn: &Connection, filter: Option<&ScopeFilter<'_>>) -> Vec<LintFinding> {
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
        .filter(|(from, _, _)| filter.is_none_or(|f| f.accepts(from)))
        .filter_map(|(from, to, raw)| {
            // For bare `[[X]]` wikilinks (raw == to), the arrow form duplicates
            // information. Only show the resolution arrow when raw differs —
            // i.e. when the user wrote `[[Y|X]]` or some alias-style form.
            let message = if raw.is_empty() || raw == to {
                format!("broken link: [[{to}]] (not found)")
            } else {
                format!("broken link: [[{raw}]] → {to} (not found)")
            };
            VaultPath::parse(&from).ok().map(|vp| LintFinding {
                check: LintCheck::BrokenLinks,
                path: vp,
                message,
                line: None,
            })
        })
        .collect()
}

fn find_dangling_refs(conn: &Connection, filter: Option<&ScopeFilter<'_>>) -> Vec<LintFinding> {
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
        .filter(|(path, _, _)| filter.is_none_or(|f| f.accepts(path)))
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

fn find_unreferenced(conn: &Connection, filter: Option<&ScopeFilter<'_>>) -> Vec<LintFinding> {
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
        .filter(|p| filter.is_none_or(|f| f.accepts(p)))
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
