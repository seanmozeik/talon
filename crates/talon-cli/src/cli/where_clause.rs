use talon_core::{WhereClause, WhereOperator};

/// Parses a `--where` string into a [`WhereClause`].
///
/// Format: `KEY OP VALUE` (space-separated, but multi-char operators like
/// `^=` and `~=` can be glued to the key: `path^=prefix`).
/// `exists` takes only one token: `KEY exists`.
///
/// # Errors
///
/// Returns an error string if the format is invalid or the operator is unknown.
pub fn parse_where_clause(value: &str) -> Result<WhereClause, String> {
    let mut parts = value.splitn(3, ' ');

    // First token is always part of the key (possibly with a glued operator).
    let Some(key_with_op) = parts.next().filter(|k| !k.is_empty()) else {
        return Err(invalid_clause(value));
    };

    // Second token is either the operator or part of the value.
    let second_token = parts.next();

    // Try to extract a multi-char operator from within the first token.
    if let Some((key, op_str)) = split_operator_in_token(key_with_op) {
        let op = parse_operator(op_str)?;
        // The value is either the remaining iterator token OR (if no space-separated
        // token exists) the remainder of key_with_op after the operator.
        let value_part = match second_token {
            Some(v) if !v.is_empty() => Some(v),
            _ => {
                // No space separator — value is glued to the operator.
                // e.g. "path^=Templates/" → key="path", op="^=", value="Templates/"
                let prefix_len = key.len();
                Some(&key_with_op[prefix_len + op_str.len()..])
            }
        };
        return finish_clause(key, op, value_part, value);
    }

    // First token is a plain key; second token must be the operator.
    let Some(operator_str) = second_token.filter(|o| !o.is_empty()) else {
        return Err(invalid_clause(value));
    };

    let op = parse_operator(operator_str)?;
    finish_clause(key_with_op, op, parts.next(), value)
}

/// If `token` contains a known multi-char operator (`^=` or `~=`),
/// split it into `(key, operator)`. Returns `None` if no match.
fn split_operator_in_token(token: &str) -> Option<(&str, &str)> {
    // Check for ^= — find '^' and verify it's followed by '='.
    if let Some(pos) = token.find('^')
        && token.get(pos + 1..=pos + 1) == Some("=")
    {
        let key = &token[..pos];
        if !key.is_empty() {
            return Some((key, "^="));
        }
    }
    // Check for ~= — find '~=' anywhere.
    if let Some(pos) = token.find("~=") {
        let key = &token[..pos];
        if !key.is_empty() {
            return Some((key, "~="));
        }
    }
    None
}

fn finish_clause(
    key: &str,
    op: WhereOperator,
    value_part: Option<&str>,
    clause: &str,
) -> Result<WhereClause, String> {
    if op == WhereOperator::Exists {
        return parse_exists_clause(key, op, value_part, clause);
    }

    let Some(value_part) = value_part.filter(|v| !v.is_empty()) else {
        return Err(format!(
            "operator '{}' requires a value in '{}'",
            op_display(op),
            clause
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
        "^=" => Ok(WhereOperator::StartsWith),
        "~=" => Ok(WhereOperator::GlobMatch),
        other => Err(format!(
            "unknown operator '{other}'; try =, !=, <, <=, >, >=, contains, exists, ^=, ~="
        )),
    }
}

const fn op_display(op: WhereOperator) -> &'static str {
    match op {
        WhereOperator::Equals => "=",
        WhereOperator::NotEquals => "!=",
        WhereOperator::LessThan => "<",
        WhereOperator::LessThanOrEqual => "<=",
        WhereOperator::GreaterThan => ">",
        WhereOperator::GreaterThanOrEqual => ">=",
        WhereOperator::Contains => "contains",
        WhereOperator::Exists => "exists",
        WhereOperator::StartsWith => "^=",
        WhereOperator::GlobMatch => "~=",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_where_clause_accepts_value_operator() {
        let Ok(clause) = parse_where_clause("status = archived") else {
            panic!("valid where clause should parse");
        };

        assert_eq!(clause.key, "status");
        assert_eq!(clause.op, WhereOperator::Equals);
        assert_eq!(clause.value.as_deref(), Some("archived"));
    }

    #[test]
    fn parse_where_clause_accepts_exists_without_value() {
        let Ok(clause) = parse_where_clause("source exists") else {
            panic!("valid where clause should parse");
        };

        assert_eq!(clause.key, "source");
        assert_eq!(clause.op, WhereOperator::Exists);
        assert_eq!(clause.value, None);
    }

    #[test]
    fn parse_where_clause_accepts_prefix_operator_glued() {
        let Ok(clause) = parse_where_clause("path^=Patients/") else {
            panic!("valid where clause should parse");
        };

        assert_eq!(clause.key, "path");
        assert_eq!(clause.op, WhereOperator::StartsWith);
        assert_eq!(clause.value.as_deref(), Some("Patients/"));
    }

    #[test]
    fn parse_where_clause_accepts_prefix_operator_spaced() {
        let Ok(clause) = parse_where_clause("path ^= Patients/") else {
            panic!("valid where clause should parse");
        };

        assert_eq!(clause.key, "path");
        assert_eq!(clause.op, WhereOperator::StartsWith);
        assert_eq!(clause.value.as_deref(), Some("Patients/"));
    }

    #[test]
    fn parse_where_clause_accepts_glob_operator_glued() {
        let Ok(clause) = parse_where_clause("path~='Templates/**'") else {
            panic!("valid where clause should parse");
        };

        assert_eq!(clause.key, "path");
        assert_eq!(clause.op, WhereOperator::GlobMatch);
        assert_eq!(clause.value.as_deref(), Some("'Templates/**'"));
    }

    #[test]
    fn parse_where_clause_accepts_glob_operator_spaced() {
        let Ok(clause) = parse_where_clause("path ~= Templates/**") else {
            panic!("valid where clause should parse");
        };

        assert_eq!(clause.key, "path");
        assert_eq!(clause.op, WhereOperator::GlobMatch);
        assert_eq!(clause.value.as_deref(), Some("Templates/**"));
    }

    #[test]
    fn parse_where_clause_rejects_missing_value() {
        let Err(err) = parse_where_clause("status =") else {
            panic!("missing value should fail");
        };

        assert!(err.contains("requires a value"));
    }

    #[test]
    fn parse_where_clause_rejects_exists_value() {
        let Err(err) = parse_where_clause("source exists yes") else {
            panic!("exists value should fail");
        };

        assert!(err.contains("does not accept a value"));
    }
}
