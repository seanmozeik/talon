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
// Intentional divergence from OHS `chunker.ts:23-35` and `chunker.ts:110`:
// `tokenx-rs` preserves Unicode-aware estimates for CJK/Hangul/Cyrillic/
// fullwidth text, and we keep its overlap wiring instead of porting the
// coarser OHS heuristic.
use tokenx_rs::estimate_token_count;

use crate::config::ChunkerConfig;

#[derive(Debug, Clone, Copy)]
struct TokenxSizer;

impl ChunkSizer for TokenxSizer {
    fn size(&self, chunk: &str) -> usize {
        estimate_token_count(chunk)
    }
}

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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
