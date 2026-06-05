//! Text utilities for markdown parsing and chunking.
//!
//! Line splitting, fence/heading detection, token estimation, wikilink parsing,
//! and keyword/path normalization. Ported from the TypeScript Talon implementation.

use regex::Regex;

use super::nfd;

#[path = "processing_wikilink.rs"]
mod wikilink;

pub use wikilink::{ParsedWikiLink, parse_wikilink, strip_outer_quotes};

/// Token-to-character ratio for rough token estimation.
pub const TOKEN_CHAR_RATIO: u8 = 4;

/// Length of a line feed character.
const LF_LENGTH: usize = 1;

/// Heading pattern: `# ` through `###### `.
const HEADING_PATTERN: &str = r"(?u)^#{1,6}\s+(.*)$";

/// Fence pattern: triple backtick or triple tilde lines.
const FENCE_PATTERN: &str = r"(?u)^(`{3,}|~{3,})\s*.*$";

/// A line span within the original content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineSpan {
    /// Number of bytes consumed by the line break (0 for last line).
    pub break_length: usize,
    /// Byte offset where the line ends (exclusive).
    pub end: usize,
    /// 1-indexed line number.
    pub line_number: u32,
    /// Byte offset where the line starts (inclusive).
    pub start: usize,
    /// The line text (without the line break).
    pub text: String,
}

/// Splits markdown content into line spans.
///
/// Handles both LF and CRLF line endings. The last line (no trailing newline)
/// gets `break_length = 0` and `end = content.len()`.
///
/// # Examples
///
/// ```
/// use talon_core::text::{split_lines, LineSpan};
///
/// let lines = split_lines("line1\nline2\nline3");
/// assert_eq!(lines.len(), 3);
/// assert_eq!(lines[0].text, "line1");
/// assert_eq!(lines[0].line_number, 1);
/// assert_eq!(lines[2].text, "line3");
/// assert_eq!(lines[2].break_length, 0);
/// ```
#[must_use]
pub fn split_lines(content: &str) -> Vec<LineSpan> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut line_number: u32 = 1;

    let bytes = content.as_bytes();
    while start < bytes.len() {
        let end_of_line = bytes[start..].iter().position(|&b| b == b'\n');

        if let Some(offset) = end_of_line {
            let end = start + offset;
            lines.push(LineSpan {
                break_length: LF_LENGTH,
                end,
                line_number,
                start,
                text: content[start..end].to_string(),
            });
            start = end + LF_LENGTH;
            line_number += 1;
        } else {
            lines.push(LineSpan {
                break_length: 0,
                end: content.len(),
                line_number,
                start,
                text: content[start..].to_string(),
            });
            break;
        }
    }

    lines
}

/// Cached regex patterns for fence and heading detection.
struct Patterns {
    fence: Regex,
    heading: Regex,
}

impl Patterns {
    fn new() -> Self {
        Self {
            fence: Regex::new(FENCE_PATTERN).unwrap_or_else(|_| panic!("valid fence regex")),
            heading: Regex::new(HEADING_PATTERN).unwrap_or_else(|_| panic!("valid heading regex")),
        }
    }
}

thread_local! {
    static PATTERNS: Patterns = Patterns::new();
}

/// Checks if a line is a fenced code block (3+ backticks or tildes).
///
/// # Examples
///
/// ```
/// use talon_core::text::is_fence_line;
///
/// assert!(is_fence_line("```ts"));
/// assert!(is_fence_line("~~~"));
/// assert!(!is_fence_line("# heading"));
/// assert!(!is_fence_line("not a fence"));
/// ```
#[must_use]
pub fn is_fence_line(line: &str) -> bool {
    PATTERNS.with(|p| p.fence.is_match(line.trim()))
}

/// Checks if a line is an ATX heading (1-6 hash characters followed by space).
///
/// # Examples
///
/// ```
/// use talon_core::text::is_heading_line;
///
/// assert!(is_heading_line("# Title"));
/// assert!(is_heading_line("###### Deep"));
/// assert!(!is_heading_line("####### Too deep"));
/// assert!(!is_heading_line("Not a heading"));
/// ```
#[must_use]
pub fn is_heading_line(line: &str) -> bool {
    PATTERNS.with(|p| p.heading.is_match(line.trim()))
}

/// Strips heading markers from a heading line.
///
/// Removes leading `#` characters (1-6), whitespace, and trailing `#` characters.
///
/// # Examples
///
/// ```
/// use talon_core::text::strip_heading_text;
///
/// assert_eq!(strip_heading_text("# Hello World"), "Hello World");
/// assert_eq!(strip_heading_text("### Nested ##"), "Nested");
/// assert_eq!(strip_heading_text("###### Deep heading"), "Deep heading");
/// ```
#[must_use]
pub fn strip_heading_text(line: &str) -> String {
    let trimmed = line.trim();
    // Count leading # characters (up to 6)
    let hash_count = trimmed.chars().take_while(|&c| c == '#').count().min(6);
    let without_hashes = &trimmed[hash_count..];
    // Skip leading whitespace after #s
    let without_ws = without_hashes.trim_start();
    // Remove trailing # characters
    let without_trailing = without_ws.trim_end_matches('#');
    without_trailing.trim().to_string()
}

/// Estimates the number of tokens in text using a character ratio.
///
/// Uses `max(1, ceil(text.len() / TOKEN_CHAR_RATIO))` where
/// `TOKEN_CHAR_RATIO = 4`.
///
/// # Examples
///
/// ```
/// use talon_core::text::estimate_tokens;
///
/// assert_eq!(estimate_tokens(""), 1);
/// assert_eq!(estimate_tokens("hello"), 2);  // ceil(5/4) = 2
/// assert_eq!(estimate_tokens("hello world"), 3);  // ceil(11/4) = 3
/// ```
#[must_use]
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 1;
    }
    let len = text.len();
    len.div_ceil(TOKEN_CHAR_RATIO as usize).max(1)
}

/// Upper bound on text fed to yake keyword extraction.
///
/// Keyword salience plateaus well before this; the cap exists to keep
/// segtok/fancy-regex from hitting its backtrack limit on large notes (TOO-44).
/// 64 KiB covers any real note's prose comfortably.
pub const YAKE_INPUT_MAX_BYTES: usize = 64 * 1024;

/// Truncates `s` to at most `max_bytes`, rounding the cut down to a UTF-8 char
/// boundary so the returned slice never splits a multibyte character.
///
/// Used to bound input to backtracking-heavy text passes (e.g. yake keyword
/// extraction via segtok/fancy-regex, which can blow its backtrack limit on
/// very large or pathological inputs).
#[must_use]
pub fn truncate_on_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut idx = max_bytes;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    &s[..idx]
}

/// Normalizes a keyword for comparison: NFD normalization + lowercase + trim.
///
/// Matches the TypeScript `normalizeTalonKeyword` behavior exactly.
///
/// # Examples
///
/// ```
/// use talon_core::text::normalize_keyword;
///
/// assert_eq!(normalize_keyword("Hello World"), "hello world");
/// assert_eq!(normalize_keyword("  Test  "), "test");
/// assert_eq!(normalize_keyword("CAFÉ"), "cafe\u{0301}");
/// ```
#[must_use]
pub fn normalize_keyword(value: &str) -> String {
    nfd::normalize(value.trim()).to_lowercase()
}

/// Normalizes a vault path: backslashes to forward slashes, NFD normalization.
///
/// Matches the TypeScript `normalizeTalonVaultPath` behavior.
///
/// # Examples
///
/// ```
/// use talon_core::text::normalize_vault_path;
///
/// assert_eq!(normalize_vault_path("notes\\hello.md"), "notes/hello.md");
/// assert_eq!(normalize_vault_path("notes/hello.md"), "notes/hello.md");
/// ```
#[must_use]
pub fn normalize_vault_path(value: &str) -> String {
    nfd::normalize(&value.replace('\\', "/"))
}

#[cfg(test)]
mod tests;
