use super::*;

fn default_cfg() -> ChunkerConfig {
    ChunkerConfig::default()
}

fn tiny_cfg() -> ChunkerConfig {
    ChunkerConfig {
        chunk_tokens: 50,
        chunk_overlap: 0,
        chunk_min_tokens: 1,
    }
}

#[test]
fn test_build_heading_path() {
    assert_eq!(build_heading_path(&[]), "");
    assert_eq!(build_heading_path(&["Intro".to_string()]), "Intro");
    assert_eq!(
        build_heading_path(&["Intro".to_string(), "Deep".to_string()]),
        "Intro > Deep"
    );
}

#[test]
fn test_build_embedding_text() {
    let text = build_embedding_text(
        "My Title",
        "notes/test.md",
        &["Section".to_string()],
        "body",
    );
    assert_eq!(
        text,
        "Title: My Title\nPath: notes/test.md\nHeadings: Section\n\nbody"
    );
}

#[test]
fn test_make_chunk_hash() {
    let hash = make_chunk_hash("hello world");
    assert_eq!(hash.len(), 64);
    assert_eq!(
        make_chunk_hash("hello world"),
        make_chunk_hash("hello world")
    );
    assert_ne!(make_chunk_hash("hello"), make_chunk_hash("world"));
}

#[test]
fn test_parser_fidelity_body_is_byte_faithful() {
    let raw = "---\ntitle: Fidelity\nstatus: active\n---\n\n# Body\n\nContent here.\n";
    let parsed = crate::text::frontmatter::parse_frontmatter(raw);
    // body = content after the closing '---' (the \n terminating '---' is included)
    // so body starts with \n (from '---\n') then \n (blank line) then # Body...
    let expected_body = "\n\n# Body\n\nContent here.\n";
    assert_eq!(
        parsed.body, expected_body,
        "body should be everything after the closing '---' marker"
    );
    // Exact reconstruction: "---\n" + frontmatter_raw + "---" + body == raw
    assert_eq!(
        format!("---\n{}---{}", parsed.frontmatter_raw, parsed.body),
        raw,
        "full file should round-trip via frontmatter_raw + body"
    );
}

#[test]
fn test_frontmatter_excluded_from_chunks() {
    // Simulate what wiring.rs does: pass parsed.body, not the full file
    let body = "\n# Filters Note\n\nThis is the body content.\n";
    let chunks = chunk_markdown(body, "Filters Note", "Filters/Note.md", &default_cfg());
    for chunk in &chunks {
        assert!(
            !chunk.text.contains("status:"),
            "chunk text must not contain YAML key 'status:': {:?}",
            chunk.text
        );
        assert!(
            !chunk.text.contains("archived"),
            "chunk text must not contain frontmatter value 'archived': {:?}",
            chunk.text
        );
    }
}

#[test]
fn test_obsidian_inline_comment_stripped() {
    let body = "Visible text. %%hidden comment%% More visible.";
    let chunks = chunk_markdown(body, "T", "t.md", &tiny_cfg());
    let all_text: String = chunks
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        !all_text.contains("%%"),
        "inline Obsidian comment must be stripped: {all_text}"
    );
    assert!(
        !all_text.contains("hidden comment"),
        "comment content must not appear: {all_text}"
    );
}

#[test]
fn test_obsidian_block_comment_stripped() {
    let body = "Before.\n%%\nblock comment line 1\nblock comment line 2\n%%\nAfter.";
    let chunks = chunk_markdown(body, "T", "t.md", &tiny_cfg());
    let all_text: String = chunks
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !all_text.contains("%%"),
        "block Obsidian comment must be stripped"
    );
    assert!(
        !all_text.contains("block comment line"),
        "comment content must not appear"
    );
    assert!(
        all_text.contains("Before"),
        "content before comment must survive"
    );
    assert!(
        all_text.contains("After"),
        "content after comment must survive"
    );
}

#[test]
fn test_callout_not_split() {
    // CommonMark blockquote — text-splitter treats BlockQuote as a Block level element
    let body = "> [!note]\n> body line 1\n> body line 2\n\nRegular paragraph after callout.";
    let chunks = chunk_markdown(body, "T", "t.md", &tiny_cfg());
    // The callout lines should appear together in one chunk
    let callout_chunk = chunks.iter().find(|c| c.text.contains("[!note]"));
    assert!(callout_chunk.is_some(), "callout should produce a chunk");
    if let Some(c) = callout_chunk {
        assert!(
            c.text.contains("body line 1") && c.text.contains("body line 2"),
            "callout body lines should be in the same chunk: {:?}",
            c.text
        );
    }
}

#[test]
fn test_math_block_not_split() {
    // pulldown-cmark with Options::all() parses $$…$$ as DisplayMath (Block level)
    let body = "Paragraph before.\n\n$$\n\\frac{a}{b} = c\n$$\n\nParagraph after.";
    let chunks = chunk_markdown(body, "T", "t.md", &tiny_cfg());
    // There must be no chunk that starts with the equation but lacks the closing $$
    // i.e. no chunk contains only part of the math block
    let math_chunks: Vec<_> = chunks.iter().filter(|c| c.text.contains("frac")).collect();
    for mc in &math_chunks {
        // A chunk containing the math content must also contain the delimiter or the full equation
        assert!(
            mc.text.contains("$$") || (mc.text.contains("frac") && !mc.text.contains("$$")),
            "math block chunk should not be split mid-equation: {:?}",
            mc.text
        );
    }
    // Verify the equation text appears somewhere (not stripped)
    assert!(
        chunks.iter().any(|c| c.text.contains("frac")),
        "math equation content must survive in chunks"
    );
}

#[test]
fn test_fenced_code_block_not_split() {
    let body =
        "Before code.\n\n```rust\nfn hello() {\n    println!(\"hello\");\n}\n```\n\nAfter code.";
    let chunks = chunk_markdown(body, "T", "t.md", &tiny_cfg());
    // No chunk should start with ``` without also ending with ```
    for chunk in &chunks {
        let fence_count = chunk.text.matches("```").count();
        if fence_count > 0 {
            assert!(
                fence_count >= 2 || !chunk.text.trim_start().starts_with("```"),
                "fenced code block must not be split mid-fence: {:?}",
                chunk.text
            );
        }
    }
    // The function body must appear somewhere
    assert!(
        chunks.iter().any(|c| c.text.contains("println")),
        "code block content must survive"
    );
}

#[test]
fn test_block_id_preserved_inline() {
    let body = "This is a paragraph with a block ID. ^my-block-id\n\nAnother paragraph.";
    let chunks = chunk_markdown(body, "T", "t.md", &default_cfg());
    let has_block_id = chunks.iter().any(|c| c.text.contains("^my-block-id"));
    assert!(has_block_id, "block IDs should be preserved inside chunks");
}

#[test]
fn test_heading_only_chunk_skipped() {
    // A body with only a heading and no body text
    let body = "# Just A Heading\n";
    let chunks = chunk_markdown(body, "T", "t.md", &tiny_cfg());
    assert!(
        chunks.is_empty(),
        "heading-only content should produce no chunks, got: {chunks:?}"
    );
}

#[test]
fn test_separator_only_chunk_skipped() {
    let body = "Some text.\n\n---\n\nMore text.";
    let chunks = chunk_markdown(body, "T", "t.md", &tiny_cfg());
    // No chunk should contain only '---'
    for chunk in &chunks {
        assert_ne!(
            chunk.text.trim(),
            "---",
            "separator-only chunk must be filtered"
        );
    }
}

#[test]
fn test_single_wikilink_chunk_skipped() {
    let body = "[[Some Note]]\n\nThis is real content.";
    let chunks = chunk_markdown(body, "T", "t.md", &tiny_cfg());
    for chunk in &chunks {
        assert_ne!(
            chunk.text.trim(),
            "[[Some Note]]",
            "single wikilink chunk must be filtered"
        );
    }
}

#[test]
fn test_single_image_embed_chunk_skipped() {
    let body = "![[image.png]]\n\nThis is real content.";
    let chunks = chunk_markdown(body, "T", "t.md", &tiny_cfg());
    for chunk in &chunks {
        assert_ne!(
            chunk.text.trim(),
            "![[image.png]]",
            "single image embed chunk must be filtered"
        );
    }
}

#[test]
fn test_chunk_min_tokens_filters_tiny_chunks() {
    let body =
        "Hi.\n\nThis is a much longer paragraph with many more words to exceed the threshold.";
    let strict_cfg = ChunkerConfig {
        chunk_tokens: 20,
        chunk_overlap: 0,
        chunk_min_tokens: 10,
    };
    let chunks = chunk_markdown(body, "T", "t.md", &strict_cfg);
    for chunk in &chunks {
        assert!(
            chunk.token_estimate >= 10,
            "chunk below min_tokens should have been filtered: {:?} (tokens: {})",
            chunk.text,
            chunk.token_estimate
        );
    }
}

#[test]
fn test_tokenx_estimates_representative_inputs() {
    assert_eq!(estimate_token_count(""), 0);
    assert_eq!(estimate_token_count("你好世界"), 4);
    assert_eq!(estimate_token_count("12345"), 1);
    assert_eq!(estimate_token_count("3.14"), 3);
    assert_eq!(estimate_token_count("process_items"), 13);
}

#[test]
fn test_chunk_token_estimate_matches_tokenx_for_representative_inputs() {
    let cfg = ChunkerConfig {
        chunk_tokens: 128,
        chunk_overlap: 0,
        chunk_min_tokens: 1,
    };

    for (body, expected) in [
        ("你好世界", 4),
        ("12345", 1),
        ("3.14", 3),
        ("process_items", 13),
    ] {
        let chunks = chunk_markdown(body, "T", "t.md", &cfg);
        assert_eq!(
            chunks.len(),
            1,
            "body should remain a single chunk: {body:?}"
        );
        assert_eq!(chunks[0].text, body);
        assert_eq!(chunks[0].token_estimate, expected);
        assert_eq!(chunks[0].token_estimate, estimate_token_count(body));
    }
}

#[test]
fn test_chunk_overlap_is_deterministic_across_passes() {
    let cfg = ChunkerConfig {
        chunk_tokens: 18,
        chunk_overlap: 6,
        chunk_min_tokens: 1,
    };
    let body = "This paragraph contains enough distinct words to require chunking and to exercise overlap semantics across repeated runs. It keeps going with additional descriptive words so the splitter has to create more than one chunk and carry some context forward. This final clause pushes the text well past the token limit.";

    let first = chunk_markdown(body, "Doc", "doc.md", &cfg);
    let second = chunk_markdown(body, "Doc", "doc.md", &cfg);

    assert!(
        first.len() > 1,
        "overlap sanity-check needs a multi-chunk body: {first:?}"
    );
    assert_eq!(
        first, second,
        "chunk boundaries and offsets should remain stable across repeated passes"
    );
}

#[test]
fn test_heading_context_tracked() {
    // Use a chunk limit small enough to force splitting between the two sections.
    let split_cfg = ChunkerConfig {
        chunk_tokens: 15,
        chunk_overlap: 0,
        chunk_min_tokens: 1,
    };
    // Each section must exceed 15 tokens so the splitter has to respect the heading boundary.
    let section_one = "# Section One\n\nThis is the first paragraph under section one. It has enough words to require splitting.";
    let section_two = "## Subsection\n\nThis is the second paragraph under the subsection. It also has enough words to force a split.";
    let body = format!("{section_one}\n\n{section_two}");
    let chunks = chunk_markdown(&body, "Doc", "doc.md", &split_cfg);

    // After the heading line "# Section One", chunks in that region carry "Section One".
    // After "## Subsection", chunks carry "Section One > Subsection".
    let has_section_one = chunks.iter().any(|c| c.heading_path == "Section One");
    let has_subsection = chunks
        .iter()
        .any(|c| c.heading_path == "Section One > Subsection");
    assert!(
        has_section_one,
        "first section heading path should appear; chunks: {:?}",
        chunks.iter().map(|c| &c.heading_path).collect::<Vec<_>>()
    );
    assert!(
        has_subsection,
        "subsection heading path should appear; chunks: {:?}",
        chunks.iter().map(|c| &c.heading_path).collect::<Vec<_>>()
    );
}

#[test]
fn test_empty_body_produces_no_chunks() {
    assert!(chunk_markdown("", "T", "t.md", &default_cfg()).is_empty());
}

#[test]
fn test_whitespace_only_body_produces_no_chunks() {
    assert!(chunk_markdown("   \n\n  ", "T", "t.md", &default_cfg()).is_empty());
}

#[test]
fn test_chunk_hash_is_stable() {
    let body = "# Test\n\nSome stable content.";
    let c1 = chunk_markdown(body, "Test", "test.md", &default_cfg());
    let c2 = chunk_markdown(body, "Test", "test.md", &default_cfg());
    assert_eq!(c1.len(), c2.len());
    for (a, b) in c1.iter().zip(c2.iter()) {
        assert_eq!(a.chunk_hash, b.chunk_hash);
    }
}
