use super::*;

#[test]
fn strips_heading_lines() {
    assert_eq!(build_match_text("# Heading\nbody text"), "body text");
    assert_eq!(build_match_text("## Sub\n### Sub2\ncontent"), "content");
}

#[test]
fn strips_fenced_code_blocks() {
    let text = "intro\n```rust\nlet x = 1;\n```\nafter";
    assert_eq!(build_match_text(text), "intro after");
}

#[test]
fn strips_html_tags() {
    assert_eq!(build_match_text("hello <b>world</b>"), "hello world");
}

#[test]
fn resolves_wikilinks_to_alias() {
    assert_eq!(build_match_text("see [[Target|Alias]]"), "see Alias");
    assert_eq!(build_match_text("see [[PlainTarget]]"), "see PlainTarget");
}

#[test]
fn strips_embed_wikilinks() {
    assert_eq!(
        build_match_text("before ![[embed.md]] after"),
        "before after"
    );
}

#[test]
fn strips_image_embeds() {
    assert_eq!(
        build_match_text("before ![alt](img.png) after"),
        "before after"
    );
}

#[test]
fn strips_bold_and_italic() {
    assert_eq!(build_match_text("**bold** and *italic*"), "bold and italic");
}

#[test]
fn strips_list_markers() {
    assert_eq!(build_match_text("- item one"), "item one");
    assert_eq!(build_match_text("1. first item"), "first item");
}

#[test]
fn strips_task_checkboxes() {
    assert_eq!(build_match_text("- [ ] todo item"), "todo item");
    assert_eq!(build_match_text("- [x] done item"), "done item");
}

#[test]
fn keeps_inline_link_text() {
    assert_eq!(
        build_match_text("[click here](http://example.com)"),
        "click here"
    );
}

#[test]
fn strips_footnote_refs() {
    assert_eq!(build_match_text("text[^1] more"), "text more");
}

#[test]
fn truncates_to_80_chars() {
    let long = "a".repeat(200);
    assert_eq!(build_match_text(&long).chars().count(), 80);
}

#[test]
fn callout_line_skipped() {
    let text = "> [!note]\n> body content";
    assert_eq!(build_match_text(text), "body content");
}

#[test]
fn inline_code_content_kept() {
    assert_eq!(build_match_text("`some_fn()`"), "some_fn()");
}
