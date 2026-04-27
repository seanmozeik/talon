use super::*;

#[test]
fn test_normalize_keyword() {
    assert_eq!(normalize_keyword("Hello World"), "helloworld");
    assert_eq!(normalize_keyword("  Test  "), "test");
    assert_eq!(normalize_keyword("CAFÉ"), "café");
}

#[test]
fn test_parse_frontmatter_simple() {
    let content = "---\ntitle: Hello\nauthor: World\n---\n\nBody text.";
    let result = parse_frontmatter(content);
    assert_eq!(result.body.trim(), "Body text.");
    assert!(result.frontmatter.contains_key("title"));
    assert!(result.frontmatter.contains_key("author"));
}

#[test]
fn test_parse_frontmatter_no_frontmatter() {
    let content = "No frontmatter here.";
    let result = parse_frontmatter(content);
    assert_eq!(result.body, content);
    assert!(result.frontmatter.is_empty());
}

#[test]
fn test_extract_wikilinks() {
    let content =
        "Check [[My Note]] and [[Other#section]].\n```\n[[not a link]]\n```\nAnd [[Target|alias]].";
    let links = extract_wikilinks(content);
    assert_eq!(links.len(), 3);
    assert_eq!(links[0].target, "My Note");
    assert_eq!(links[1].target, "Other");
    assert_eq!(links[1].heading, Some("section".to_string()));
    assert_eq!(links[2].target, "Target");
    assert_eq!(links[2].alias, Some("alias".to_string()));
}

#[test]
fn test_extract_inline_tags() {
    let content = "Some text #hello #world and #foo.\n```\n#not a tag\n```";
    let tags = links::extract_inline_tags(content);
    assert!(tags.contains(&"hello".to_string()));
    assert!(tags.contains(&"world".to_string()));
    assert!(tags.contains(&"foo".to_string()));
}

#[test]
fn test_reverse_index() {
    let mut idx = FrontmatterReverseIndex::default();
    idx.insert("path1.md", "tag", "rust");
    idx.insert("path2.md", "tag", "rust");
    idx.insert("path3.md", "tag", "python");

    let results = idx.lookup("tag", "rust");
    assert!(results.is_some());
    assert_eq!(results.unwrap().len(), 2);

    let results = idx.lookup("tag", "python");
    assert!(results.is_some());
    assert_eq!(results.unwrap().len(), 1);
}

#[test]
fn test_reverse_source_index() {
    let mut idx = ReverseSourceIndex::default();
    idx.insert("a.md".to_string(), "b.md".to_string());
    idx.insert("c.md".to_string(), "b.md".to_string());

    let sources = idx.get("b.md");
    assert!(sources.is_some());
    assert_eq!(sources.unwrap().len(), 2);
}
