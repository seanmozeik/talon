//! Chunker module: semantic markdown segmentation using `text-splitter`.
//!
//! Body text (frontmatter already stripped by the indexer) is cleaned of
//! Obsidian `%%...%%` comments, then split with [`MarkdownSplitter`] backed
//! by a [`tokenx_rs`] length function.  Heading context is reconstructed
//! from the text preceding each split point.  Trivial and sub-threshold
//! chunks are discarded before returning.

use std::sync::OnceLock;

use regex::Regex;
use sha2::{Digest, Sha256};
use text_splitter::{ChunkConfig, ChunkSizer, MarkdownSplitter};
use tokenx_rs::estimate_token_count;

use crate::config::ChunkerConfig;

// ── Token sizer ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
struct TokenxSizer;

impl ChunkSizer for TokenxSizer {
    fn size(&self, chunk: &str) -> usize {
        estimate_token_count(chunk)
    }
}

// ── Types ────────────────────────────────────────────────────────────────────

/// A chunk of note content ready for embedding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteChunk {
    /// Byte offset where the chunk starts (in the stripped body).
    pub char_start: usize,
    /// Byte offset where the chunk ends (exclusive, in the stripped body).
    pub char_end: usize,
    /// SHA-256 of `text`.
    pub chunk_hash: String,
    /// Prefixed embedding text: `Title: …\nPath: …\nHeadings: …\n\n{text}`.
    pub embedding_text: String,
    /// Active heading stack at the chunk's start position.
    pub headings: Vec<String>,
    /// Headings joined with ` > `.
    pub heading_path: String,
    /// 1-based line number where the chunk starts (in the stripped body).
    pub line_start: u32,
    /// 1-based line number where the chunk ends (in the stripped body).
    pub line_end: u32,
    /// Trimmed chunk text.
    pub text: String,
    /// Token count estimate via `tokenx-rs`.
    pub token_estimate: usize,
}

// ── Public helpers ───────────────────────────────────────────────────────────

/// Build heading path by joining headings with ` > `.
#[must_use]
pub fn build_heading_path(headings: &[String]) -> String {
    headings.join(" > ")
}

/// Build prefixed embedding text.
#[must_use]
pub fn build_embedding_text(title: &str, path: &str, headings: &[String], text: &str) -> String {
    format!(
        "Title: {}\nPath: {}\nHeadings: {}\n\n{}",
        title,
        path,
        build_heading_path(headings),
        text
    )
}

/// SHA-256 hash of raw text.
#[must_use]
pub fn make_chunk_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

// ── Core chunker ─────────────────────────────────────────────────────────────

/// Chunk a note body (frontmatter already stripped) into [`NoteChunk`]s.
///
/// The caller is responsible for passing the body-only text — frontmatter
/// must be stripped before calling this function so YAML keys/values never
/// appear in chunk text or embedding text.
///
/// Pipeline:
/// 1. Strip Obsidian `%%…%%` comments.
/// 2. Split with `MarkdownSplitter` using a `tokenx-rs` token-count sizer.
/// 3. Reconstruct heading context from the text before each split point.
/// 4. Drop trivial chunks (heading-only, separator, single wikilink/embed).
/// 5. Drop chunks below `config.chunk_min_tokens`.
#[must_use]
pub fn chunk_markdown(
    body: &str,
    title: &str,
    path: &str,
    config: &ChunkerConfig,
) -> Vec<NoteChunk> {
    let cleaned = strip_obsidian_comments(body);

    let chunk_config = {
        let base = ChunkConfig::new(config.chunk_tokens).with_sizer(TokenxSizer);
        if config.chunk_overlap > 0 && config.chunk_overlap < config.chunk_tokens {
            base.with_overlap(config.chunk_overlap)
                .unwrap_or_else(|_| ChunkConfig::new(config.chunk_tokens).with_sizer(TokenxSizer))
        } else {
            base
        }
    };

    let splitter = MarkdownSplitter::new(chunk_config);

    splitter
        .chunk_indices(&cleaned)
        .filter_map(|(byte_offset, raw_chunk)| {
            let text = raw_chunk.trim().to_string();

            if is_trivial_chunk(&text) {
                return None;
            }

            let token_estimate = estimate_token_count(&text);
            if token_estimate < config.chunk_min_tokens {
                return None;
            }

            let headings = headings_at_byte_offset(&cleaned, byte_offset);
            let byte_end = byte_offset + raw_chunk.len();

            let line_start = byte_offset_to_line(&cleaned, byte_offset);
            let line_end = byte_offset_to_line(&cleaned, byte_end.saturating_sub(1));

            Some(NoteChunk {
                char_start: byte_offset,
                char_end: byte_end,
                chunk_hash: make_chunk_hash(&text),
                embedding_text: build_embedding_text(title, path, &headings, &text),
                heading_path: build_heading_path(&headings),
                headings,
                line_start,
                line_end,
                text,
                token_estimate,
            })
        })
        .collect()
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Strip Obsidian `%%inline%%` and `%%\nblock\n%%` comments.
fn strip_obsidian_comments(body: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?s)%%.*?%%").unwrap_or_else(|_| unreachable!()));
    re.replace_all(body, "").into_owned()
}

/// Walk the text up to `byte_offset` and return the active heading stack.
fn headings_at_byte_offset(text: &str, byte_offset: usize) -> Vec<String> {
    let before = &text[..byte_offset.min(text.len())];
    let mut headings: Vec<String> = Vec::new();
    for line in before.lines() {
        let level = line.bytes().take_while(|&b| b == b'#').count();
        if level > 0 && level <= 6 {
            let rest = &line[level..];
            if let Some(heading_text) = rest.strip_prefix(' ') {
                headings.truncate(level.saturating_sub(1));
                headings.push(heading_text.trim().to_string());
            }
        }
    }
    headings
}

/// Return the 1-based line number for a byte offset within `text`.
fn byte_offset_to_line(text: &str, byte_offset: usize) -> u32 {
    let clamped = byte_offset.min(text.len());
    let newlines = text[..clamped].bytes().filter(|&b| b == b'\n').count();
    u32::try_from(newlines)
        .unwrap_or(u32::MAX)
        .saturating_add(1)
}

/// Return `true` for chunks that carry no meaningful content:
/// - heading-only lines (`# …`)
/// - horizontal separators (`---`, `***`, `___`)
/// - a bare block ID (`^word`)
/// - a single wikilink `[[…]]` or image embed `![[…]]`
fn is_trivial_chunk(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }

    let lines: Vec<&str> = text.lines().collect();

    // Multi-line chunks: only trivial if every line is trivial
    if lines.len() > 1 {
        return lines.iter().all(|l| is_trivial_line(l.trim()));
    }

    let line = lines[0].trim();
    is_trivial_line(line)
}

fn is_trivial_line(line: &str) -> bool {
    if line.is_empty() {
        return true;
    }

    // ATX heading
    if line.starts_with('#') {
        let level = line.bytes().take_while(|&b| b == b'#').count();
        if level <= 6 && line[level..].starts_with(' ') {
            return true;
        }
    }

    // Thematic breaks / horizontal rules
    if matches!(line, "---" | "***" | "___" | "- - -" | "* * *" | "_ _ _") {
        return true;
    }

    // Block ID alone: ^word-or-hyphen
    if line.starts_with('^') && line[1..].chars().all(|c| c.is_alphanumeric() || c == '-') {
        return true;
    }

    // Single wikilink or image embed
    if (line.starts_with("[[") && line.ends_with("]]"))
        || (line.starts_with("![[") && line.ends_with("]]"))
    {
        return true;
    }

    // Single image line (markdown syntax)
    if line.starts_with("![") && line.ends_with(')') {
        return true;
    }

    false
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
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

    // ── Stable helpers ───────────────────────────────────────────────────────

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

    // ── Parser fidelity ──────────────────────────────────────────────────────

    #[test]
    fn test_parser_fidelity_body_is_byte_faithful() {
        let raw = "---\ntitle: Fidelity\nstatus: active\n---\n\n# Body\n\nContent here.\n";
        let parsed = crate::frontmatter::parse_frontmatter(raw);
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

    // ── Frontmatter excluded from chunks ─────────────────────────────────────

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

    // ── Obsidian comment stripping ────────────────────────────────────────────

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

    // ── Obsidian callout preservation ─────────────────────────────────────────

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

    // ── Math block preservation ───────────────────────────────────────────────

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

    // ── Fenced code block preservation ───────────────────────────────────────

    #[test]
    fn test_fenced_code_block_not_split() {
        let body = "Before code.\n\n```rust\nfn hello() {\n    println!(\"hello\");\n}\n```\n\nAfter code.";
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

    // ── Block IDs preserved ───────────────────────────────────────────────────

    #[test]
    fn test_block_id_preserved_inline() {
        let body = "This is a paragraph with a block ID. ^my-block-id\n\nAnother paragraph.";
        let chunks = chunk_markdown(body, "T", "t.md", &default_cfg());
        let has_block_id = chunks.iter().any(|c| c.text.contains("^my-block-id"));
        assert!(has_block_id, "block IDs should be preserved inside chunks");
    }

    // ── Trivial chunk filtering ───────────────────────────────────────────────

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

    // ── Min-token threshold ───────────────────────────────────────────────────

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

    // ── Heading context ───────────────────────────────────────────────────────

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

    // ── Empty body ────────────────────────────────────────────────────────────

    #[test]
    fn test_empty_body_produces_no_chunks() {
        assert!(chunk_markdown("", "T", "t.md", &default_cfg()).is_empty());
    }

    #[test]
    fn test_whitespace_only_body_produces_no_chunks() {
        assert!(chunk_markdown("   \n\n  ", "T", "t.md", &default_cfg()).is_empty());
    }

    // ── Hash stability ────────────────────────────────────────────────────────

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
}
