use super::*;

#[test]
fn test_split_lines_basic() {
    let lines = split_lines("line1\nline2\nline3");
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].text, "line1");
    assert_eq!(lines[0].line_number, 1);
    assert_eq!(lines[0].break_length, 1);
    assert_eq!(lines[1].text, "line2");
    assert_eq!(lines[2].text, "line3");
    assert_eq!(lines[2].break_length, 0);
}

#[test]
fn test_split_lines_crlf() {
    let lines = split_lines("line1\r\nline2\nline3");
    assert_eq!(lines.len(), 3);
    // TS splits on \n only, so text includes trailing \r
    assert_eq!(lines[0].text, "line1\r");
    assert_eq!(lines[0].break_length, 1); // TS always uses LF_LENGTH=1
    assert_eq!(lines[1].text, "line2");
    assert_eq!(lines[1].break_length, 1);
    assert_eq!(lines[2].text, "line3");
}

#[test]
fn test_split_lines_empty() {
    let lines = split_lines("");
    assert!(lines.is_empty());
}

#[test]
fn test_split_lines_single_line_no_newline() {
    let lines = split_lines("just one line");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "just one line");
    assert_eq!(lines[0].break_length, 0);
}

#[test]
fn test_is_fence_line_triple_backtick() {
    assert!(is_fence_line("```"));
    assert!(is_fence_line("```ts"));
    assert!(is_fence_line("```rust"));
}

#[test]
fn test_is_fence_line_triple_tilde() {
    assert!(is_fence_line("~~~"));
    assert!(is_fence_line("~~~js"));
}

#[test]
fn test_is_fence_line_more_than_three() {
    assert!(is_fence_line("````"));
    assert!(is_fence_line("~~~~"));
}

#[test]
fn test_is_fence_line_not_a_fence() {
    assert!(!is_fence_line("``"));
    assert!(!is_fence_line("# heading"));
    assert!(!is_fence_line("not a fence"));
    // ```inline code IS a fence (matches pattern)
}

#[test]
fn test_is_heading_line_all_levels() {
    assert!(is_heading_line("# H1"));
    assert!(is_heading_line("## H2"));
    assert!(is_heading_line("### H3"));
    assert!(is_heading_line("#### H4"));
    assert!(is_heading_line("##### H5"));
    assert!(is_heading_line("###### H6"));
}

#[test]
fn test_is_heading_line_too_deep() {
    assert!(!is_heading_line("####### H7"));
}

#[test]
fn test_is_heading_line_no_space() {
    assert!(!is_heading_line("#NoSpace"));
    assert!(!is_heading_line("##NoSpace"));
}

#[test]
fn test_is_heading_line_not_heading() {
    assert!(!is_heading_line("Not a heading"));
    assert!(!is_heading_line(""));
}

#[test]
fn test_strip_heading_text_basic() {
    assert_eq!(strip_heading_text("# Hello World"), "Hello World");
    assert_eq!(strip_heading_text("### Nested ##"), "Nested");
    assert_eq!(strip_heading_text("###### Deep heading"), "Deep heading");
}

#[test]
fn test_estimate_tokens_empty() {
    assert_eq!(estimate_tokens(""), 1);
}

#[test]
fn test_estimate_tokens_short() {
    // ceil(5/4) = 2
    assert_eq!(estimate_tokens("hello"), 2);
}

#[test]
fn test_estimate_tokens_medium() {
    assert_eq!(estimate_tokens("hello world"), 3);
}

#[test]
fn test_estimate_tokens_long() {
    // 40 chars / 4 = 10 tokens
    assert_eq!(
        estimate_tokens("0123456789012345678901234567890123456789"),
        10
    );
}

#[test]
fn test_normalize_keyword_basic() {
    assert_eq!(normalize_keyword("Hello World"), "hello world");
    assert_eq!(normalize_keyword("  Test  "), "test");
    assert_eq!(normalize_keyword("CAFÉ"), "cafe\u{0301}");
}

#[test]
fn test_normalize_keyword_no_spaces() {
    assert_eq!(normalize_keyword("HelloWorld"), "helloworld");
}

#[test]
fn test_normalize_vault_path() {
    assert_eq!(normalize_vault_path("notes\\hello.md"), "notes/hello.md");
    assert_eq!(normalize_vault_path("notes/hello.md"), "notes/hello.md");
    assert_eq!(normalize_vault_path("a\\b\\c.md"), "a/b/c.md");
}

#[test]
fn normalize_vault_path_nfc_and_nfd_produce_same_form() {
    // é as a single precomposed codepoint (NFC)
    let nfc = "notes/caf\u{00e9}.md";
    // e + combining acute accent (NFD)
    let nfd = "notes/cafe\u{0301}.md";
    assert_ne!(nfc, nfd, "precondition: raw strings differ");
    assert_eq!(
        normalize_vault_path(nfc),
        normalize_vault_path(nfd),
        "NFC and NFD paths must normalize to the same form"
    );
}

#[test]
fn test_parse_wikilink_with_alias() {
    let link = parse_wikilink("Target|alias");
    assert_eq!(link.target, "Target");
    assert_eq!(link.raw_target, "Target");
    assert_eq!(link.alias, Some("alias".to_string()));
    assert_eq!(link.heading, None);
}

#[test]
fn test_parse_wikilink_simple() {
    let link = parse_wikilink("My Note");
    assert_eq!(link.target, "My Note");
    assert_eq!(link.raw_target, "My Note");
    assert_eq!(link.alias, None);
    assert_eq!(link.heading, None);
}

#[test]
fn test_parse_wikilink_with_heading() {
    let link = parse_wikilink("Target#heading");
    assert_eq!(link.target, "Target");
    // raw_target is the part before | split, trimmed. For no-alias case it's the full input.
    // TS: rawTarget = targetPart.trim() where targetPart = raw (no pipe)
    // But TS also has: const target = headingIndex === -1 ? targetPart.trim() : targetPart.slice(0, headingIndex).trim();
    // The raw_target in our impl should be the full part before | split
    assert_eq!(link.raw_target, "Target#heading");
    assert_eq!(link.alias, None);
    assert_eq!(link.heading, Some("heading".to_string()));
}

#[test]
fn test_parse_wikilink_with_alias_and_heading() {
    let link = parse_wikilink("Target#heading|alias");
    assert_eq!(link.target, "Target");
    // raw_target is the part before | split
    assert_eq!(link.raw_target, "Target#heading");
    assert_eq!(link.alias, Some("alias".to_string()));
    assert_eq!(link.heading, Some("heading".to_string()));
}

#[test]
fn test_parse_wikilink_with_spaces() {
    let link = parse_wikilink("My Target|My Alias");
    assert_eq!(link.target, "My Target");
    assert_eq!(link.alias, Some("My Alias".to_string()));
}

#[test]
fn test_strip_outer_quotes_double() {
    assert_eq!(strip_outer_quotes("\"hello\""), "hello");
}

#[test]
fn test_strip_outer_quotes_single() {
    assert_eq!(strip_outer_quotes("'hello'"), "hello");
}

#[test]
fn test_strip_outer_quotes_no_quotes() {
    assert_eq!(strip_outer_quotes("hello"), "hello");
}

#[test]
fn test_strip_outer_quotes_mismatched() {
    assert_eq!(strip_outer_quotes("'hello\""), "'hello\"");
}

#[test]
fn test_strip_outer_quotes_too_short() {
    assert_eq!(strip_outer_quotes("\""), "\"");
}

#[test]
fn test_strip_outer_quotes_whitespace() {
    assert_eq!(strip_outer_quotes("  \"hello\"  "), "hello");
}
