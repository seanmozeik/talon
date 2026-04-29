//! Pre-computed search filters injected into retrieval SQL.
//!
//! Replaces the post-filter pipeline in `query/search.rs` (`apply_since_filter`,
//! `apply_where_filter`, `apply_scope_filter`) with SQL-level constraints so the
//! candidate pool only ever contains notes that match the user's intent.

use std::fmt::Write as _;

use rusqlite::Connection;
use rusqlite::types::Value;

use crate::config::ScopeFilter;
use crate::search::input::{WhereClause, WhereOperator};

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
    /// `--where` frontmatter clauses.
    pub where_clauses: Vec<WhereClause>,
}

impl PreFilter {
    #[must_use]
    pub const fn none() -> Self {
        Self {
            since_ms: None,
            accepted_note_ids: None,
            where_clauses: Vec::new(),
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
    }
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
