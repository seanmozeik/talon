use talon_core::{WhereClause, WhereOperator};

/// Parses a `--where` string into a [`WhereClause`].
///
/// Format: `KEY OP VALUE` (three space-separated tokens).
/// `exists` takes only one token: `KEY exists`.
///
/// # Errors
///
/// Returns an error string if the format is invalid or the operator is unknown.
pub fn parse_where_clause(value: &str) -> Result<WhereClause, String> {
    let mut parts = value.splitn(3, ' ');
    let Some(key) = parts.next().filter(|key| !key.is_empty()) else {
        return Err(invalid_clause(value));
    };
    let Some(operator) = parts.next().filter(|operator| !operator.is_empty()) else {
        return Err(invalid_clause(value));
    };
    let value_part = parts.next();
    let op = parse_operator(operator)?;

    if op == WhereOperator::Exists {
        return parse_exists_clause(key, op, value_part, value);
    }

    let Some(value_part) = value_part.filter(|part| !part.is_empty()) else {
        return Err(format!(
            "operator '{operator}' requires a value in '{value}'"
        ));
    };

    Ok(WhereClause {
        key: key.to_string(),
        op,
        value: Some(value_part.to_string()),
    })
}

fn parse_operator(operator: &str) -> Result<WhereOperator, String> {
    match operator {
        "=" => Ok(WhereOperator::Equals),
        "!=" => Ok(WhereOperator::NotEquals),
        "<" => Ok(WhereOperator::LessThan),
        "<=" => Ok(WhereOperator::LessThanOrEqual),
        ">" => Ok(WhereOperator::GreaterThan),
        ">=" => Ok(WhereOperator::GreaterThanOrEqual),
        "contains" => Ok(WhereOperator::Contains),
        "exists" => Ok(WhereOperator::Exists),
        other => Err(format!(
            "unknown operator '{other}'; try =, !=, <, <=, >, >=, contains, exists"
        )),
    }
}

fn parse_exists_clause(
    key: &str,
    op: WhereOperator,
    value_part: Option<&str>,
    clause: &str,
) -> Result<WhereClause, String> {
    if value_part.is_some() {
        return Err(format!(
            "operator 'exists' does not accept a value in '{clause}'"
        ));
    }
    Ok(WhereClause {
        key: key.to_string(),
        op,
        value: None,
    })
}

fn invalid_clause(value: &str) -> String {
    format!("invalid where clause '{value}'; expected 'KEY OP VALUE' or 'KEY exists'")
}
