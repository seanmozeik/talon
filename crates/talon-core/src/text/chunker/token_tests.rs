use super::*;
use tokenx_rs::estimate_token_count;

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
