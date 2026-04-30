//! Pre-computed search filters injected into retrieval SQL.
//!
//! Replaces the post-filter pipeline in `query/search.rs` (`apply_since_filter`,
//! `apply_where_filter`, `apply_scope_filter`) with SQL-level constraints so the
//! candidate pool only ever contains notes that match the user's intent.

use std::fmt::Write as _;

use globset::{GlobBuilder, GlobSetBuilder};
use rusqlite::Connection;
use rusqlite::types::Value;

use crate::config::ScopeFilter;
use crate::search::input::{WhereClause, WhereOperator};
use crate::search::query_syntax::normalize_tag_filter;
use crate::search::types::RawSearchResult;

/// Filters to push down into each retrieval query.
///
/// Build once in `run_search_inner`, then pass to every retrieval function so
/// the candidate pool is already constrained before scoring.
#[derive(Debug, Default, Clone)]
pub struct PreFilter {
    /// Minimum `notes.mtime_ms` threshold (from `--since`).
    pub since_ms: Option<u64>,
    /// Accepted note IDs from scope config. `None` = all notes in scope.
    /// `Some([])` = impossible (zero notes pass): callers should short-circuit.
    pub accepted_note_ids: Option<Vec<i64>>,
    /// `--where` clauses (all of them; some may need SQL, some post-filter).
    pub where_clauses: Vec<WhereClause>,
    /// Tag filters from `SearchInput.tag` or query syntax.
    pub tags: Vec<String>,
    /// Heading filters from `SearchInput.heading` or query syntax.
    pub headings: Vec<String>,
}

impl PreFilter {
    #[must_use]
    pub const fn none() -> Self {
        Self {
            since_ms: None,
            accepted_note_ids: None,
            where_clauses: Vec::new(),
            tags: Vec::new(),
            headings: Vec::new(),
        }
    }

    /// Returns `true` when no notes can ever match (scope resolved to empty set).
    #[must_use]
    pub fn is_impossible(&self) -> bool {
        self.accepted_note_ids.as_ref().is_some_and(Vec::is_empty)
    }

    /// Builds a SQL fragment (`AND …`) and bound params to append to an
    /// existing `WHERE` clause. The notes table must be aliased as `n`.
    ///
    /// Returns `(fragment, params)` where `fragment` starts with ` AND` when
    /// non-empty, or is an empty string when no filters are active.
    #[must_use]
    pub fn sql_fragment(&self) -> (String, Vec<Value>) {
        let mut sql = String::new();
        let mut params: Vec<Value> = Vec::new();

        if let Some(ms) = self.since_ms {
            sql.push_str(" AND n.mtime_ms >= ?");
            // Timestamps in ms won't exceed i64::MAX before year 292 million.
            params.push(Value::Integer(i64::try_from(ms).unwrap_or(i64::MAX)));
        }

        if let Some(ids) = &self.accepted_note_ids {
            let placeholders = std::iter::repeat_n("?", ids.len())
                .collect::<Vec<_>>()
                .join(",");
            let _ = write!(sql, " AND n.id IN ({placeholders})");
            params.extend(ids.iter().map(|&id| Value::Integer(id)));
        }

        for clause in &self.where_clauses {
            let (fragment, clause_params) = where_clause_sql(clause);
            sql.push_str(&fragment);
            params.extend(clause_params);
        }

        for tag in &self.tags {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM note_tags nt \
                 WHERE nt.note_id = n.id AND nt.tag_norm = ?)",
            );
            params.push(Value::Text(normalize_tag_filter(tag)));
        }

        for heading in &self.headings {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM chunks c \
                 WHERE c.note_id = n.id AND LOWER(c.heading_path) LIKE ?)",
            );
            params.push(Value::Text(format!("%{}%", heading.to_lowercase())));
        }

        (sql, params)
    }
}

/// Pre-computes the set of note IDs accepted by `scope_filter`.
///
/// Returns `None` when all active notes are in scope (no SQL restriction
/// needed). Returns `Some(ids)` with the accepted subset otherwise.
pub fn scope_to_note_ids(conn: &Connection, scope_filter: &ScopeFilter<'_>) -> Option<Vec<i64>> {
    if scope_filter.accepts_all() {
        return None;
    }

    let Ok(mut stmt) = conn.prepare("SELECT id, vault_path FROM notes WHERE active = 1") else {
        return None;
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    }) else {
        return None;
    };

    let mut total: usize = 0;
    let mut accepted: Vec<i64> = Vec::new();
    for row in rows.filter_map(Result::ok) {
        let (id, path) = row;
        total += 1;
        if scope_filter.accepts(&path) {
            accepted.push(id);
        }
    }

    // When all notes pass, returning None avoids a potentially large IN clause
    // with no filtering benefit (covers the common all-default-scopes case).
    if accepted.len() == total {
        return None;
    }

    Some(accepted)
}

fn where_clause_sql(clause: &WhereClause) -> (String, Vec<Value>) {
    let key = Value::Text(clause.key.clone());
    let val = Value::Text(clause.value.clone().unwrap_or_default());
    let pfx = "EXISTS (SELECT 1 FROM note_frontmatter_fields WHERE note_id = n.id AND field = ?";

    match clause.op {
        WhereOperator::Exists => (format!(" AND {pfx})"), vec![key]),
        WhereOperator::Equals => (format!(" AND {pfx} AND value = ?)"), vec![key, val]),
        // EXISTS with `value != ?` naturally excludes notes where the field is absent
        // (no rows → EXISTS false), matching the Rust post-filter behavior exactly.
        WhereOperator::NotEquals => (format!(" AND {pfx} AND value != ?)"), vec![key, val]),
        WhereOperator::LessThan => ordered_where_clause_sql(pfx, key, clause, "<"),
        WhereOperator::LessThanOrEqual => ordered_where_clause_sql(pfx, key, clause, "<="),
        WhereOperator::GreaterThan => ordered_where_clause_sql(pfx, key, clause, ">"),
        WhereOperator::GreaterThanOrEqual => ordered_where_clause_sql(pfx, key, clause, ">="),
        // INSTR is case-sensitive, matching Rust's str::contains exactly.
        // LIKE would be case-insensitive for ASCII and diverge.
        WhereOperator::Contains => (
            format!(" AND {pfx} AND INSTR(value, ?) > 0)"),
            vec![key, val],
        ),
        WhereOperator::StartsWith => {
            let pattern = format!("{}%", escape_like(clause.value.as_ref()));
            if clause.key == "path" {
                // Prefix match on vault_path: LIKE 'prefix%'
                (
                    " AND n.vault_path LIKE ? ESCAPE '\'".to_string(),
                    vec![Value::Text(pattern)],
                )
            } else {
                // Frontmatter prefix: EXISTS with LIKE
                (
                    format!(" AND {pfx} AND value LIKE ? ESCAPE '\\')"),
                    vec![key, Value::Text(pattern)],
                )
            }
        }
        WhereOperator::GlobMatch => {
            // Glob patterns can't be expressed in SQL (SQLite GLOB doesn't support **).
            // Return empty fragment — caller must post-filter if needed.
            (String::new(), Vec::new())
        }
    }
}

/// Escapes `%`, `_`, and `\` for use in a SQL LIKE pattern.
fn escape_like(value: Option<&String>) -> String {
    let s = value.map_or("", |v| v.as_str());
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn ordered_where_clause_sql(
    pfx: &str,
    key: Value,
    clause: &WhereClause,
    op: &str,
) -> (String, Vec<Value>) {
    let Some(target) = clause.value.as_deref() else {
        return (format!(" AND {pfx} AND 0)"), vec![key]);
    };
    if let Ok(number) = target.parse::<f64>() {
        return (
            format!(" AND {pfx} AND value_type = 'number' AND CAST(value AS REAL) {op} ?)"),
            vec![key, Value::Real(number)],
        );
    }
    (
        format!(" AND {pfx} AND value_type = 'date' AND value {op} ?)"),
        vec![key, Value::Text(target.to_string())],
    )
}

/// Returns `true` if any clause needs post-filtering (i.e. cannot be expressed in SQL).
#[must_use]
pub fn has_glob_where_clauses(clauses: &[WhereClause]) -> bool {
    clauses.iter().any(|c| c.op == WhereOperator::GlobMatch)
}

/// Post-filters search results by glob `where` clauses.
///
/// Uses the same evaluation logic as [`crate::query::where_filter`] but operates
/// on raw paths and frontmatter JSON strings (no `note_id` lookup needed).
pub fn filter_results_by_glob(
    conn: &Connection,
    results: &[RawSearchResult],
    clauses: &[WhereClause],
) -> Vec<RawSearchResult> {
    let glob_clauses: Vec<&WhereClause> = clauses
        .iter()
        .filter(|c| c.op == WhereOperator::GlobMatch)
        .collect();

    if glob_clauses.is_empty() {
        return results.to_vec();
    }

    results
        .iter()
        .filter(|r| {
            let note_id = conn
                .query_row(
                    "SELECT id FROM notes WHERE vault_path = ? AND active = 1",
                    [&r.path],
                    |row| row.get::<_, i64>(0),
                )
                .ok();
            note_id.is_some_and(|id| {
                glob_clauses
                    .iter()
                    .all(|clause| clause_matches_glob(conn, id, clause))
            })
        })
        .cloned()
        .collect()
}

fn clause_matches_glob(conn: &Connection, note_id: i64, clause: &WhereClause) -> bool {
    let Some(ref pattern) = clause.value else {
        return false;
    };

    if clause.key == "path" {
        // Match against vault_path
        let path = conn
            .query_row(
                "SELECT vault_path FROM notes WHERE id = ? AND active = 1",
                [note_id],
                |row| row.get::<_, String>(0),
            )
            .ok();
        path.is_some_and(|p| glob_match(pattern, &p))
    } else {
        // Match against frontmatter field values
        let Ok(mut stmt) = conn
            .prepare("SELECT value FROM note_frontmatter_fields WHERE note_id = ? AND field = ?")
        else {
            return false;
        };
        let values: Vec<String> = stmt
            .query_map(rusqlite::params![note_id, &clause.key], |row| {
                row.get::<_, String>(0)
            })
            .and_then(Iterator::collect)
            .unwrap_or_default();

        !values.is_empty() && values.iter().any(|v| glob_match(pattern, v.as_str()))
    }
}

/// Checks if `text` matches the glob `pattern`.
fn glob_match(pattern: &str, text: &str) -> bool {
    let mut builder = GlobSetBuilder::new();
    let Ok(glob) = GlobBuilder::new(pattern).case_insensitive(false).build() else {
        return false;
    };
    builder.add(glob);
    let Ok(set) = builder.build() else {
        return false;
    };
    set.is_match(text)
}
