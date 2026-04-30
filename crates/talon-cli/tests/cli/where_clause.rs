use talon_cli::cli::parse_where_clause;
use talon_core::WhereOperator;

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
fn parse_where_clause_accepts_prefix_operator() {
    let Ok(clause) = parse_where_clause("path ^= Templates/") else {
        panic!("valid where clause should parse");
    };

    assert_eq!(clause.key, "path");
    assert_eq!(clause.op, WhereOperator::StartsWith);
    assert_eq!(clause.value.as_deref(), Some("Templates/"));
}

#[test]
fn parse_where_clause_accepts_glob_operator() {
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
