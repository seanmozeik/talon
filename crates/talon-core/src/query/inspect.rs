//! Inspect checks for vault graph health.

use crate::config::{ScopeFilter, TalonConfig};
use crate::contracts::VaultPath;
use crate::graph::graph_health;
use crate::indexer::prelude::{build_ignore_globset, file_matches_ignore};
use crate::indexing::{InspectCheck, InspectFinding, InspectInput, InspectResponse};
use crate::sync::relink_unresolved;
use globset::GlobSet;
use rusqlite::Connection;

pub fn query_inspect(
    conn: &Connection,
    input: &InspectInput,
    config: Option<&TalonConfig>,
) -> InspectResponse {
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
    let ignore_set = config
        .map_or_else(
            || build_ignore_globset(&[]),
            |cfg| build_ignore_globset(&cfg.ignore_patterns),
        )
        .ok();
    let findings = match input.check {
        InspectCheck::All => find_all(conn, filter.as_ref(), ignore_set.as_ref()),
        InspectCheck::Orphans => find_orphans(conn, filter.as_ref()),
        InspectCheck::BrokenLinks => find_broken_links(conn, filter.as_ref(), ignore_set.as_ref()),
        InspectCheck::DanglingRefs => {
            find_dangling_refs(conn, filter.as_ref(), ignore_set.as_ref())
        }
        InspectCheck::Unreferenced => find_unreferenced(conn, filter.as_ref()),
        InspectCheck::Graph => graph_health(conn, filter.as_ref()),
    };
    let findings = match config {
        Some(cfg) => findings
            .into_iter()
            .filter(|f| !cfg.inspect_excluded(std::path::Path::new(f.path.as_str())))
            .collect(),
        None => findings,
    };
    InspectResponse {
        vault: None,
        check: input.check,
        findings,
    }
}

fn find_all(
    conn: &Connection,
    filter: Option<&ScopeFilter<'_>>,
    ignore_set: Option<&GlobSet>,
) -> Vec<InspectFinding> {
    let mut findings = find_orphans(conn, filter);
    findings.extend(find_broken_links(conn, filter, ignore_set));
    findings.extend(find_dangling_refs(conn, filter, ignore_set));
    findings.extend(find_unreferenced(conn, filter));
    findings.extend(graph_health(conn, filter));
    findings
}

fn find_orphans(conn: &Connection, filter: Option<&ScopeFilter<'_>>) -> Vec<InspectFinding> {
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
            VaultPath::parse(&path).ok().map(|vp| InspectFinding {
                check: InspectCheck::Orphans,
                path: vp,
                message: "no incoming links".to_string(),
                line: None,
            })
        })
        .collect()
}

fn find_broken_links(
    conn: &Connection,
    filter: Option<&ScopeFilter<'_>>,
    ignore_set: Option<&GlobSet>,
) -> Vec<InspectFinding> {
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
        // Links to ignored files are not broken — the target is intentionally
        // excluded from indexing (e.g. CLAUDE.md, PURPOSE.md). Treat them as
        // valid targets.
        .filter(|(_, to, _)| !ignored_by_set(to, ignore_set))
        .filter_map(|(from, to, raw)| {
            // For bare `[[X]]` wikilinks (raw == to), the arrow form duplicates
            // information. Only show the resolution arrow when raw differs —
            // i.e. when the user wrote `[[Y|X]]` or some alias-style form.
            let message = if raw.is_empty() || raw == to {
                format!("broken link: [[{to}]] (not found)")
            } else {
                format!("broken link: [[{raw}]] → {to} (not found)")
            };
            VaultPath::parse(&from).ok().map(|vp| InspectFinding {
                check: InspectCheck::BrokenLinks,
                path: vp,
                message,
                line: None,
            })
        })
        .collect()
}

fn find_dangling_refs(
    conn: &Connection,
    filter: Option<&ScopeFilter<'_>>,
    ignore_set: Option<&GlobSet>,
) -> Vec<InspectFinding> {
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
        // Frontmatter paths to ignored files are not dangling — the target is
        // intentionally excluded from indexing.
        .filter(|(_, _, value)| !ignored_by_set(value, ignore_set))
        .filter_map(|(path, field, value)| {
            VaultPath::parse(&path).ok().map(|vp| InspectFinding {
                check: InspectCheck::DanglingRefs,
                path: vp,
                message: format!("dangling ref: {field}: {value} (not found)"),
                line: None,
            })
        })
        .collect()
}

fn ignored_by_set(path: &str, ignore_set: Option<&GlobSet>) -> bool {
    ignore_set.is_some_and(|set| file_matches_ignore(path, set))
}

fn find_unreferenced(conn: &Connection, filter: Option<&ScopeFilter<'_>>) -> Vec<InspectFinding> {
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
            VaultPath::parse(&path).ok().map(|vp| InspectFinding {
                check: InspectCheck::Unreferenced,
                path: vp,
                message: "no incoming or outgoing links".to_string(),
                line: None,
            })
        })
        .collect()
}
