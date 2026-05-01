//! Shared `--where` frontmatter filter primitives.
//!
//! Used by both the search and meta query handlers to evaluate
//! `WhereClause` predicates against notes stored in `note_frontmatter_fields`,
//! or against the note's own `vault_path` when the key is `"path"`.

use std::cmp::Ordering;

use rusqlite::{Connection, params};

use crate::search::{WhereClause, WhereOperator};

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
        WhereOperator::LessThan => {
            check_fm_field_ordered(conn, note_id, &clause.key, clause, Ordering::is_lt)
        }
        WhereOperator::LessThanOrEqual => {
            check_fm_field_ordered(conn, note_id, &clause.key, clause, |o| !o.is_gt())
        }
        WhereOperator::GreaterThan => {
            check_fm_field_ordered(conn, note_id, &clause.key, clause, Ordering::is_gt)
        }
        WhereOperator::GreaterThanOrEqual => {
            check_fm_field_ordered(conn, note_id, &clause.key, clause, |o| !o.is_lt())
        }
        WhereOperator::Contains => check_fm_field(conn, note_id, &clause.key, |value| {
            value.is_some_and(|v| v.contains(clause.value.as_deref().unwrap_or("")))
        }),
        WhereOperator::StartsWith => {
            if clause.key == "path" {
                check_path_prefix(conn, note_id, clause)
            } else {
                check_fm_field(conn, note_id, &clause.key, |value| {
                    value.is_some_and(|v| v.starts_with(clause.value.as_deref().unwrap_or("")))
                })
            }
        }
        WhereOperator::GlobMatch => {
            if clause.key == "path" {
                check_path_glob(conn, note_id, clause)
            } else {
                check_fm_field(conn, note_id, &clause.key, |value| {
                    value.is_some_and(|v| {
                        clause.value.as_deref().is_some_and(|pat| {
                            crate::glob_match_case_insensitive(pat, v).unwrap_or(false)
                        })
                    })
                })
            }
        }
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
        .and_then(Iterator::collect)
        .unwrap_or_default();

    !values.is_empty() && values.iter().any(|v| pred(Some(v.as_str())))
}

/// Returns `true` if ANY stored typed value for `field` on `note_id` satisfies `pred`.
///
/// Returns `false` when the field has no entries.
pub fn check_fm_field_ordered<F>(
    conn: &Connection,
    note_id: i64,
    field: &str,
    clause: &WhereClause,
    pred: F,
) -> bool
where
    F: Fn(Ordering) -> bool,
{
    let Some(target) = clause.value.as_deref() else {
        return false;
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT value, value_type FROM note_frontmatter_fields WHERE note_id = ? AND field = ?",
    ) else {
        return false;
    };
    let values: Vec<(String, String)> = stmt
        .query_map(params![note_id, field], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .and_then(Iterator::collect)
        .unwrap_or_default();

    values
        .iter()
        .filter_map(|(value, value_type)| compare_typed_value(value, value_type, target))
        .any(pred)
}

fn compare_typed_value(value: &str, value_type: &str, target: &str) -> Option<Ordering> {
    match value_type {
        "number" => {
            let lhs = value.parse::<f64>().ok()?;
            let rhs = target.parse::<f64>().ok()?;
            lhs.partial_cmp(&rhs)
        }
        "date" => {
            let lhs = parse_date_millis(value)?;
            let rhs = parse_date_millis(target)?;
            Some(lhs.cmp(&rhs))
        }
        _ => None,
    }
}

fn parse_date_millis(value: &str) -> Option<i128> {
    if let Ok(dt) =
        time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
    {
        return Some(dt.unix_timestamp_nanos() / 1_000_000);
    }

    let date = time::Date::parse(
        value,
        time::macros::format_description!("[year]-[month]-[day]"),
    )
    .ok()?;
    let dt = date
        .with_hms(0, 0, 0)
        .ok()
        .map(time::PrimitiveDateTime::assume_utc)?;
    Some(dt.unix_timestamp_nanos() / 1_000_000)
}

/// Returns the `vault_path` for a `note_id`.
fn get_vault_path(conn: &Connection, note_id: i64) -> Option<String> {
    conn.query_row(
        "SELECT vault_path FROM notes WHERE id = ? AND active = 1",
        params![note_id],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

/// Starts-with / prefix match on a note's `vault_path`.
fn check_path_prefix(conn: &Connection, note_id: i64, clause: &WhereClause) -> bool {
    let target = clause.value.as_deref().unwrap_or("");
    get_vault_path(conn, note_id).is_some_and(|path| path.starts_with(target))
}

/// Glob pattern match on a note's `vault_path`.
fn check_path_glob(conn: &Connection, note_id: i64, clause: &WhereClause) -> bool {
    let Some(ref pattern) = clause.value else {
        return false;
    };
    let Some(path) = get_vault_path(conn, note_id) else {
        return false;
    };
    crate::glob_match_case_insensitive(pattern, &path).unwrap_or(false)
}

#[cfg(test)]
mod glob_tests {
    #[test]
    fn test_glob_patterns() {
        // Case-insensitive: Patients/* matches patients/base.md
        assert!(
            crate::glob_match_case_insensitive("Patients/*", "patients/base.md").unwrap_or(false)
        );
        assert!(
            crate::glob_match_case_insensitive("Patients/*", "patients/nested/deep.md")
                .unwrap_or(false)
        );
        // Patients/** also matches (globset ** means zero or more dirs)
        assert!(
            crate::glob_match_case_insensitive("Patients/**", "patients/base.md").unwrap_or(false)
        );
        assert!(
            crate::glob_match_case_insensitive("Patients/**", "patients/nested/deep.md")
                .unwrap_or(false)
        );
        // Non-matching paths
        assert!(
            !crate::glob_match_case_insensitive("Patients/*", "artifacts/foo.md").unwrap_or(false)
        );
    }
}
