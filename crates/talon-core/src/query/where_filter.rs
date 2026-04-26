//! Shared `--where` frontmatter filter primitives.
//!
//! Used by both the search and meta query handlers to evaluate
//! `WhereClause` predicates against notes stored in `note_frontmatter_fields`.

use rusqlite::{Connection, params};

use crate::tool::{WhereClause, WhereOperator};

/// Returns `true` when all `clauses` match for `note_id` (AND-composed).
pub fn passes_where_clauses(conn: &Connection, note_id: i64, clauses: &[WhereClause]) -> bool {
    clauses.iter().all(|c| check_where_clause(conn, note_id, c))
}

/// Evaluates a single `WhereClause` against a note's frontmatter fields.
pub fn check_where_clause(conn: &Connection, note_id: i64, clause: &WhereClause) -> bool {
    match clause.op {
        WhereOperator::Exists => {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM note_frontmatter_fields WHERE note_id = ? AND field = ?",
                    params![note_id, &clause.key],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            count > 0
        }
        WhereOperator::Equals => check_fm_field(conn, note_id, &clause.key, |value| {
            value == clause.value.as_deref()
        }),
        WhereOperator::NotEquals => check_fm_field(conn, note_id, &clause.key, |value| {
            value != clause.value.as_deref()
        }),
        WhereOperator::LessThan => check_fm_field_numeric(conn, note_id, &clause.key, |v| {
            v < clause.value.as_deref().unwrap_or("")
        }),
        WhereOperator::LessThanOrEqual => check_fm_field_numeric(conn, note_id, &clause.key, |v| {
            v <= clause.value.as_deref().unwrap_or("")
        }),
        WhereOperator::GreaterThan => check_fm_field_numeric(conn, note_id, &clause.key, |v| {
            v > clause.value.as_deref().unwrap_or("")
        }),
        WhereOperator::GreaterThanOrEqual => {
            check_fm_field_numeric(conn, note_id, &clause.key, |v| {
                v >= clause.value.as_deref().unwrap_or("")
            })
        }
        WhereOperator::Contains => check_fm_field(conn, note_id, &clause.key, |value| {
            value.is_some_and(|v| v.contains(clause.value.as_deref().unwrap_or("")))
        }),
    }
}

/// Returns `true` if ANY stored value for `field` on `note_id` satisfies `pred`.
///
/// Returns `false` when the field has no entries (does not exist on the note).
pub fn check_fm_field<F>(conn: &Connection, note_id: i64, field: &str, pred: F) -> bool
where
    F: Fn(Option<&str>) -> bool,
{
    let Ok(mut stmt) =
        conn.prepare("SELECT value FROM note_frontmatter_fields WHERE note_id = ? AND field = ?")
    else {
        return false;
    };
    let values: Vec<String> = stmt
        .query_map(params![note_id, field], |row| row.get::<_, String>(0))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default();

    !values.is_empty() && values.iter().any(|v| pred(Some(v.as_str())))
}

/// Returns `true` if ANY stored string value for `field` on `note_id` satisfies `pred`.
///
/// Returns `false` when the field has no entries.
pub fn check_fm_field_numeric<F>(conn: &Connection, note_id: i64, field: &str, pred: F) -> bool
where
    F: Fn(&str) -> bool,
{
    let Ok(mut stmt) =
        conn.prepare("SELECT value FROM note_frontmatter_fields WHERE note_id = ? AND field = ?")
    else {
        return false;
    };
    let values: Vec<String> = stmt
        .query_map(params![note_id, field], |row| row.get::<_, String>(0))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default();

    !values.is_empty() && values.iter().any(|v| pred(v.as_str()))
}
