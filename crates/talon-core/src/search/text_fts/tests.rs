use super::*;

#[test]
fn get_trigrams_returns_lowercase_string_for_short_input() {
    let t = get_trigrams("AB");
    assert!(t.contains("ab"));
    assert_eq!(t.len(), 1);
}

#[test]
fn get_trigrams_slides_window_for_long_input() {
    let t = get_trigrams("Hello");
    assert!(t.contains("hel"));
    assert!(t.contains("ell"));
    assert!(t.contains("llo"));
    assert_eq!(t.len(), 3);
}

#[test]
fn trigram_overlap_full_match_returns_one() {
    assert_eq!(calculate_trigram_overlap("hello", "hello"), 1.0);
}

#[test]
fn trigram_overlap_no_match_returns_zero() {
    assert_eq!(calculate_trigram_overlap("xyz", "abc"), 0.0);
}

#[test]
fn trigram_overlap_partial_match() {
    let overlap = calculate_trigram_overlap("hello", "help");
    assert!((overlap - 1.0 / 3.0).abs() < 1e-9);
}

#[test]
fn sanitize_strips_special_chars_and_collapses_whitespace() {
    assert_eq!(sanitize_fts_query("  foo \"bar*  baz^ "), "foo bar baz");
}

#[test]
fn sanitize_empty_returns_empty_string() {
    assert_eq!(sanitize_fts_query("   "), "");
}

#[test]
fn to_fts_query_and_default() {
    assert_eq!(
        to_fts_query("foo bar", FtsOperator::And),
        "\"foo\"* AND \"bar\"*"
    );
}

#[test]
fn to_fts_query_or() {
    assert_eq!(
        to_fts_query("foo bar baz", FtsOperator::Or),
        "\"foo\"* OR \"bar\"* OR \"baz\"*"
    );
}

#[test]
fn to_fts_query_empty_returns_literal_empty() {
    assert_eq!(to_fts_query("", FtsOperator::And), LITERAL_EMPTY_FTS);
    assert_eq!(to_fts_query("   ", FtsOperator::And), LITERAL_EMPTY_FTS);
}

#[test]
fn to_fts_query_strips_special_chars_before_quoting() {
    assert_eq!(
        to_fts_query("foo*bar", FtsOperator::And),
        "\"foo\"* AND \"bar\"*"
    );
}

#[test]
fn to_fts_query_negates_single_bare_term() {
    assert_eq!(
        to_fts_query("rust -async", FtsOperator::And),
        "\"rust\"* NOT \"async\"*"
    );
}

#[test]
fn to_fts_query_negates_multiple_bare_terms() {
    assert_eq!(
        to_fts_query("rust -async -tokio", FtsOperator::And),
        "\"rust\"* NOT \"async\"* NOT \"tokio\"*"
    );
}

#[test]
fn to_fts_query_all_negative_returns_empty_string() {
    assert_eq!(to_fts_query("-async", FtsOperator::And), "");
    assert_eq!(to_fts_query("-async -tokio", FtsOperator::And), "");
}

#[test]
fn to_fts_query_keeps_negation_inside_quotes_literal() {
    assert_eq!(
        to_fts_query("\"hello -world\"", FtsOperator::And),
        "\"hello -world\""
    );
}

#[test]
fn build_trigram_or_query_quotes_each_trigram() {
    assert_eq!(
        build_trigram_or_query("hello"),
        "\"hel\" OR \"ell\" OR \"llo\""
    );
}

#[test]
fn build_trigram_or_query_short_input_returns_quoted_text() {
    assert_eq!(build_trigram_or_query("ab"), "\"ab\"");
}

#[test]
fn build_trigram_or_query_strips_inner_quotes() {
    assert!(!build_trigram_or_query("a\"bc").contains("\"a\"b\""));
}

#[test]
fn build_bm25_score_zero_returns_zero() {
    assert_eq!(build_bm25_score(0.0), 0.0);
}

#[test]
fn build_bm25_score_negative_input_normalizes_to_unit_interval() {
    let s = build_bm25_score(-2.0);
    assert!((s - 2.0 / 3.0).abs() < 1e-9);
}

#[test]
fn build_bm25_score_positive_input_also_works() {
    let s = build_bm25_score(3.0);
    assert!((s - 0.75).abs() < 1e-9);
}

#[test]
fn build_bm25_score_is_bounded_below_one() {
    assert!(build_bm25_score(1000.0) < 1.0);
    assert!(build_bm25_score(1000.0) > 0.99);
}

#[test]
fn to_fts_query_hyphenated_tokens() {
    assert_eq!(to_fts_query("gpt-4", FtsOperator::And), "\"gpt 4\"");
    assert_eq!(to_fts_query("a-b-c-d", FtsOperator::And), "\"a b c d\"");
    assert_eq!(to_fts_query("DEC-0054", FtsOperator::And), "\"DEC 0054\"");
    assert_eq!(
        to_fts_query("gpt-4 foo", FtsOperator::And),
        "\"gpt 4\" AND \"foo\"*"
    );
}

#[test]
fn to_fts_query_preserves_quotes_and_hyphenated() {
    assert_eq!(
        to_fts_query("foo \"bar baz\"", FtsOperator::And),
        "\"foo\"* AND \"bar baz\""
    );
    assert_eq!(
        to_fts_query("gpt-4 \"bar baz\" multi-agent", FtsOperator::Or),
        "\"gpt 4\" OR \"bar baz\" OR \"multi agent\""
    );
}
