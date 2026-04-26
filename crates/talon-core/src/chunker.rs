//! Chunker module for markdown segmentation and chunking.
//!
//! Block-based segmentation with overlap logic, heading-aware section flushing,
//! and embedding text construction. Ported from the TypeScript Talon implementation.

use sha2::{Digest, Sha256};

use super::text::{
    LineSpan, TOKEN_CHAR_RATIO, estimate_tokens, is_fence_line, is_heading_line, split_lines,
    strip_heading_text,
};

// ── Constants ───────────────────────────────────────────────────────────────

/// Target chunk size in tokens.
const CHUNK_TOKENS: usize = 900;

/// Overlap size in tokens (`CHUNK_TOKENS` * `OVERLAP_RATIO`).
const OVERLAP_TOKENS: usize = 135;

/// Maximum characters per chunk.
const MAX_CHUNK_CHARS: usize = CHUNK_TOKENS * TOKEN_CHAR_RATIO as usize;

/// Maximum overlap characters.
const OVERLAP_CHARS: usize = OVERLAP_TOKENS * TOKEN_CHAR_RATIO as usize;

/// Length of a line feed character (used for indexing).
const LF_LENGTH: usize = 1;

// ── Types ───────────────────────────────────────────────────────────────────

/// Kind of markdown block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    Fence,
    Heading,
    Paragraph,
}

/// A contiguous block of markdown content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownBlock {
    pub char_start: usize,
    pub char_end: usize,
    pub kind: BlockKind,
    pub line_start: u32,
    pub line_end: u32,
    pub text: String,
}

/// Context for chunking operations.
#[derive(Debug, Clone)]
pub struct ChunkContext<'a> {
    pub blocks: &'a [MarkdownBlock],
    pub content: &'a str,
    pub headings: &'a [String],
    pub path: &'a str,
    pub title: &'a str,
}

/// A chunk of note content ready for embedding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteChunk {
    pub char_start: usize,
    pub char_end: usize,
    pub chunk_hash: String,
    pub embedding_text: String,
    pub headings: Vec<String>,
    pub heading_path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub text: String,
    pub token_estimate: usize,
}

// ── Helper functions ────────────────────────────────────────────────────────

/// Build heading path by joining headings with " > ".
#[must_use]
pub fn build_heading_path(headings: &[String]) -> String {
    headings.join(" > ")
}

/// Build embedding text for a chunk.
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

/// Push a block to the blocks vector.
fn push_block(
    blocks: &mut Vec<MarkdownBlock>,
    content: &str,
    lines: &[LineSpan],
    kind: BlockKind,
    start_index: usize,
    end_index: usize,
) {
    let Some(first) = lines.get(start_index) else {
        return;
    };
    let Some(last) = lines.get(end_index) else {
        return;
    };
    blocks.push(MarkdownBlock {
        char_start: first.start,
        char_end: last.end,
        kind,
        line_start: first.line_number,
        line_end: last.line_number,
        text: content[first.start..last.end].to_string(),
    });
}

/// Consume a fence block (opening fence through closing fence).
fn consume_fence_block(
    blocks: &mut Vec<MarkdownBlock>,
    content: &str,
    lines: &[LineSpan],
    start_index: usize,
) -> usize {
    let mut index = start_index + LF_LENGTH;
    while index < lines.len() {
        if is_fence_line(lines[index].text.trim()) {
            break;
        }
        index += LF_LENGTH;
    }
    if index < lines.len() {
        index += LF_LENGTH;
    }
    push_block(
        blocks,
        content,
        lines,
        BlockKind::Fence,
        start_index,
        (index - LF_LENGTH).max(start_index),
    );
    index
}

/// Consume a paragraph block.
fn consume_paragraph_block(
    blocks: &mut Vec<MarkdownBlock>,
    content: &str,
    lines: &[LineSpan],
    start_index: usize,
) -> usize {
    let mut index = start_index;
    while index < lines.len() {
        let line = &lines[index];
        if line.text.trim().is_empty()
            || is_heading_line(line.text.as_str())
            || is_fence_line(line.text.trim())
        {
            break;
        }
        index += LF_LENGTH;
    }
    push_block(
        blocks,
        content,
        lines,
        BlockKind::Paragraph,
        start_index,
        index - LF_LENGTH,
    );
    index
}

/// Collect markdown blocks from content.
#[must_use]
pub fn collect_blocks(content: &str) -> Vec<MarkdownBlock> {
    let lines = split_lines(content);
    let mut blocks = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = &lines[index];
        let trimmed = line.text.trim();
        if trimmed.is_empty() {
            index += LF_LENGTH;
        } else if is_fence_line(trimmed) {
            index = consume_fence_block(&mut blocks, content, &lines, index);
        } else if is_heading_line(line.text.as_str()) {
            push_block(
                &mut blocks,
                content,
                &lines,
                BlockKind::Heading,
                index,
                index,
            );
            index += LF_LENGTH;
        } else {
            index = consume_paragraph_block(&mut blocks, content, &lines, index);
        }
    }

    blocks
}

/// Choose the overlap start index for the next chunk.
fn choose_overlap_start(blocks: &[MarkdownBlock], end_index: usize, start_index: usize) -> usize {
    let Some(end_block) = blocks.get(end_index) else {
        return start_index;
    };

    let mut candidate = end_index;
    let mut chars = end_block.char_end - end_block.char_start;

    while candidate > start_index {
        let next = candidate - 1;
        let Some(next_block) = blocks.get(next) else {
            break;
        };
        let next_chars = next_block.char_end - next_block.char_start;
        if chars + next_chars > OVERLAP_CHARS {
            break;
        }
        candidate = next;
        chars += next_chars;
    }

    if candidate == start_index {
        (end_index + LF_LENGTH).min(blocks.len())
    } else {
        candidate
    }
}

/// Make a chunk from blocks[start..=end].
fn make_chunk(
    context: &ChunkContext<'_>,
    start_index: usize,
    end_index: usize,
) -> Option<NoteChunk> {
    let first = context.blocks.get(start_index)?;
    let last = context.blocks.get(end_index)?;
    let raw_text = &context.content[first.char_start..last.char_end];
    let text = raw_text.trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some(NoteChunk {
        char_start: first.char_start,
        char_end: last.char_end,
        chunk_hash: make_chunk_hash(raw_text),
        embedding_text: build_embedding_text(context.title, context.path, context.headings, &text),
        heading_path: build_heading_path(context.headings),
        headings: context.headings.to_vec(),
        line_start: first.line_start,
        line_end: last.line_end,
        text: raw_text.to_string(),
        token_estimate: estimate_tokens(&text),
    })
}

/// Advance chunk end until `MAX_CHUNK_CHARS` reached.
fn advance_chunk_end(blocks: &[MarkdownBlock], start: usize) -> usize {
    let mut end = start;
    let mut char_count = 0;

    while end < blocks.len() {
        let block = &blocks[end];
        let block_chars = block.char_end - block.char_start;
        if end > start && char_count + block_chars > MAX_CHUNK_CHARS {
            break;
        }
        char_count += block_chars;
        end += LF_LENGTH;
        if block.kind == BlockKind::Fence && block_chars > MAX_CHUNK_CHARS {
            break;
        }
    }

    end
}

/// Chunk blocks into `NoteChunk`s with overlap.
#[must_use]
pub fn chunk_blocks(
    content: &str,
    blocks: &[MarkdownBlock],
    title: &str,
    path: &str,
    headings: &[String],
) -> Vec<NoteChunk> {
    if blocks.is_empty() {
        return vec![];
    }

    let mut chunks = Vec::new();
    let ctx = ChunkContext {
        blocks,
        content,
        headings,
        path,
        title,
    };
    let mut start = 0;

    while start < blocks.len() {
        let end = advance_chunk_end(blocks, start);
        let last_index = if end > start { end - LF_LENGTH } else { start };
        if let Some(chunk) = make_chunk(&ctx, start, last_index) {
            chunks.push(chunk);
        }

        if end >= blocks.len() {
            break;
        }
        let next_start = choose_overlap_start(blocks, last_index, start);
        start = if next_start > start {
            next_start
        } else {
            last_index + LF_LENGTH
        };
    }

    chunks
}

/// Chunk markdown content with heading-aware section flushing.
#[must_use]
pub fn chunk_markdown(content: &str, title: &str, path: &str) -> Vec<NoteChunk> {
    let blocks = collect_blocks(content);
    if blocks.is_empty() {
        return vec![];
    }

    // Collect sections as (start_idx, end_idx, headings_snapshot)
    let mut section_starts = Vec::new();
    let mut section_ends = Vec::new();
    let mut section_headings: Vec<Vec<String>> = Vec::new();

    let mut current_headings = Vec::new();
    let mut section_start: usize = 0;

    // First pass: identify sections by walking headings
    let mut index: usize = 0;
    while index < blocks.len() {
        if blocks[index].kind == BlockKind::Heading {
            if index > section_start {
                /* has content before heading */
                section_starts.push(section_start);
                section_ends.push(index - LF_LENGTH);
                section_headings.push(current_headings.clone());
            }
            let level = blocks[index].text.chars().take_while(|&c| c == '#').count();
            current_headings.truncate(level.saturating_sub(1));
            current_headings.push(strip_heading_text(&blocks[index].text));
            section_start = index + LF_LENGTH; // content AFTER heading
        }
        index += LF_LENGTH;
    }
    // Flush final section (content after last heading)
    if blocks.len() > section_start {
        section_starts.push(section_start);
        section_ends.push(blocks.len().saturating_sub(LF_LENGTH));
        section_headings.push(current_headings);
    }

    // Second pass: chunk each section
    let mut chunks = Vec::new();
    for i in 0..section_starts.len() {
        let start = section_starts[i];
        let end = section_ends[i].min(blocks.len() - 1);
        let headings = &section_headings[i];
        let section_blocks = &blocks[start..=end];
        chunks.extend(chunk_blocks(content, section_blocks, title, path, headings));
    }

    chunks
        .into_iter()
        .filter(|c| !c.text.trim().is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(hash.len(), 64); // SHA-256 hex
        assert_eq!(
            make_chunk_hash("hello world"),
            make_chunk_hash("hello world")
        );
        assert_ne!(make_chunk_hash("hello"), make_chunk_hash("world"));
    }

    #[test]
    fn test_collect_blocks_simple() {
        let content = "# Heading\n\nPara line 1\nPara line 2";
        let blocks = collect_blocks(content);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].kind, BlockKind::Heading);
        assert_eq!(blocks[1].kind, BlockKind::Paragraph);
    }

    #[test]
    fn test_collect_blocks_with_fence() {
        let content = "# Title\n\ncode before\n```ts\nconst x = 1;\n```\nafter fence";
        let blocks = collect_blocks(content);
        assert_eq!(blocks.len(), 4);
        assert_eq!(blocks[0].kind, BlockKind::Heading);
        assert_eq!(blocks[1].kind, BlockKind::Paragraph);
        assert_eq!(blocks[2].kind, BlockKind::Fence);
        assert_eq!(blocks[3].kind, BlockKind::Paragraph);
    }

    #[test]
    fn test_collect_blocks_empty() {
        let blocks = collect_blocks("");
        assert!(blocks.is_empty());
    }

    #[test]
    fn test_chunk_markdown_single_section() {
        let content = "# Title\n\nSome paragraph text here.";
        let chunks = chunk_markdown(content, "Title", "title.md");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].heading_path, "Title");
    }

    #[test]
    fn test_chunk_markdown_two_sections() {
        let content = "# Main\n\nFirst section.\n\n## Sub\n\nSecond section.";
        let chunks = chunk_markdown(content, "Main", "main.md");
        assert!(chunks.len() >= 2);
        // First chunk has heading "Main"
        assert_eq!(chunks[0].heading_path, "Main");
        // Second chunk has heading path "Main > Sub"
        assert_eq!(chunks[1].heading_path, "Main > Sub");
    }

    #[test]
    fn test_chunk_markdown_fixture() {
        let content = r"# Chunk One

Alpha paragraph line 1.
Alpha paragraph line 2.
Alpha paragraph line 3.

```ts
const fence = ['line 1', 'line 2', 'line 3'];
```

## Chunk Two

Beta paragraph line 1.
Beta paragraph line 2.
Beta paragraph line 3.";
        let chunks = chunk_markdown(content, "Chunking Note", "chunking-note.md");
        assert!(chunks.len() >= 2);
        // Check heading paths
        let has_chunk_one = chunks.iter().any(|c| c.heading_path == "Chunk One");
        let has_chunk_two = chunks
            .iter()
            .any(|c| c.heading_path == "Chunk One > Chunk Two");
        assert!(has_chunk_one, "expected Chunk One section");
        assert!(has_chunk_two, "expected Chunk Two section");
    }

    #[test]
    fn test_chunk_blocks_overlap() {
        // Multiple paragraphs to create splittable blocks
        use std::fmt::Write as _;
        let mut big = String::from("# Title\n");
        for i in 0..100 {
            let _ = writeln!(
                big,
                "Line {i} of paragraph text with enough words to make it substantial."
            );
            if i % 5 == 4 {
                big.push('\n');
            } // blank line every 5 lines = new paragraph
        }
        let blocks = collect_blocks(&big);
        assert!(!blocks.is_empty());

        let chunks = chunk_markdown(&big, "Test", "test.md");
        // Multiple small paragraphs should span multiple chunks
        assert!(
            chunks.len() >= 2,
            "expected multiple chunks with overlap, got {}",
            chunks.len()
        );
    }

    #[test]
    fn test_chunk_markdown_fence_in_section() {
        let content = "# Section\n\nBefore fence\n```python\nprint('hello')\n```\nAfter fence";
        let blocks = collect_blocks(content);
        assert_eq!(blocks.len(), 4); // heading, para, fence, para
    }

    #[test]
    fn test_chunk_markdown_nested_headings() {
        let content = "# H1\n\nPara 1\n## H2\n\nPara 2\n### H3\n\nPara 3";
        let chunks = chunk_markdown(content, "H1", "h1.md");
        // Should have sections for H1, H1>H2, H1>H2>H3
        assert!(chunks.iter().any(|c| c.heading_path == "H1"));
        assert!(chunks.iter().any(|c| c.heading_path == "H1 > H2"));
        assert!(chunks.iter().any(|c| c.heading_path == "H1 > H2 > H3"));
    }

    #[test]
    fn test_chunk_empty_content() {
        let chunks = chunk_markdown("", "Title", "title.md");
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_only_headings() {
        let content = "# H1\n## H2\n### H3";
        let chunks = chunk_markdown(content, "H1", "h1.md");
        // Headings without body text should be filtered out
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_hash_consistency() {
        let content = "# Test\n\nSome content.";
        let chunks = chunk_markdown(content, "Test", "test.md");
        if !chunks.is_empty() {
            assert_eq!(chunks[0].chunk_hash.len(), 64);
        }
    }
}
