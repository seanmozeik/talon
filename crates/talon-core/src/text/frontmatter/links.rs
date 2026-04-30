use super::{FENCE_PATTERN, HEADING_PATTERN, INLINE_TAG_PATTERN, WIKILINK_PATTERN, WikiLink};
use regex::Regex;
use std::sync::LazyLock;

static INLINE_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(INLINE_TAG_PATTERN, "inline tag"));
static WIKILINK_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(WIKILINK_PATTERN, "wikilink"));
static FENCE_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(FENCE_PATTERN, "fence"));
static HEADING_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(HEADING_PATTERN, "heading"));

fn compile_regex(pattern: &str, name: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|error| panic!("invalid {name} regex: {error}"))
}

/// Extracts inline `#tag` syntax from markdown body (outside code fences).
pub(super) fn extract_inline_tags(content: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut inside_fence = false;

    for line in content.lines() {
        let trimmed = line.trim_start();

        if is_fence_line(trimmed) {
            inside_fence = !inside_fence;
            continue;
        }

        if !inside_fence && !is_heading_line(trimmed) {
            for capture in INLINE_TAG_RE.captures_iter(trimmed) {
                if let Some(tag_match) = capture.get(2) {
                    let raw_tag = tag_match.as_str();
                    let tag = raw_tag
                        .trim_end_matches(|c: char| {
                            [')', ']', '}', '.', ',', ';', ':', '!', '?'].contains(&c)
                        })
                        .to_string();
                    if !tag.is_empty() {
                        tags.push(tag);
                    }
                }
            }
        }
    }

    tags
}

/// Extracts wikilinks from markdown content.
///
/// # Panics
///
/// Panics if the internal wikilink regex fails to compile (should never happen).
#[must_use]
pub fn extract_wikilinks(content: &str) -> Vec<WikiLink> {
    let mut links = Vec::new();
    let mut inside_fence = false;
    let mut char_offset = 0;

    for (line_index, raw_line) in content.split_inclusive('\n').enumerate() {
        let line = trim_line_ending(raw_line);
        let trimmed = line.trim_start();

        if is_fence_line(trimmed) {
            inside_fence = !inside_fence;
            char_offset += raw_line.len();
            continue;
        }

        if !inside_fence {
            let inline_code_ranges = inline_code_ranges(line);
            for caps in WIKILINK_RE.captures_iter(line) {
                let Some(full_match) = caps.get(0) else {
                    continue;
                };
                if inline_code_ranges
                    .iter()
                    .any(|(start, end)| (*start..*end).contains(&full_match.start()))
                {
                    continue;
                }

                let raw_target = caps.get(1).map_or("", |m| m.as_str());
                let parsed = parse_wiki_link(raw_target);
                let line_start = line_number(line_index);
                let line_end = line_start;

                links.push(WikiLink {
                    alias: parsed.alias,
                    char_end: char_offset + full_match.end(),
                    char_start: char_offset + full_match.start(),
                    heading: parsed.heading,
                    line_end,
                    line_start,
                    raw_target: parsed.raw_target,
                    text: full_match.as_str().to_string(),
                    target: parsed.target,
                });
            }
        }

        char_offset += raw_line.len();
    }

    links
}

fn inline_code_ranges(line: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut cursor = 0;

    while let Some(relative_start) = line[cursor..].find('`') {
        let start = cursor + relative_start;
        let tick_count = count_backticks(&line[start..]);
        let code_start = start + tick_count;
        let Some(relative_end) = line[code_start..].find(&"`".repeat(tick_count)) else {
            break;
        };
        let end = code_start + relative_end + tick_count;
        ranges.push((start, end));
        cursor = end;
    }

    ranges
}

fn count_backticks(text: &str) -> usize {
    text.bytes().take_while(|byte| *byte == b'`').count()
}

fn trim_line_ending(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

fn line_number(line_index: usize) -> u32 {
    u32::try_from(line_index)
        .unwrap_or(u32::MAX)
        .saturating_add(1)
}

/// Parses a raw wikilink string into components.
fn parse_wiki_link(raw: &str) -> WikiLink {
    let split_index = raw.find('|');
    let target_part = split_index.map_or(raw, |i| &raw[..i]);
    let alias_part = split_index.map_or_else(String::new, |i| raw[i + 1..].trim().to_string());
    let heading_index = target_part.find('#');
    let target = heading_index.map_or_else(
        || target_part.trim().to_string(),
        |i| target_part[..i].trim().to_string(),
    );
    let heading = heading_index.and_then(|i| {
        let h = target_part[i + 1..].trim();
        if h.is_empty() {
            None
        } else {
            Some(h.to_string())
        }
    });
    let alias = if alias_part.is_empty() {
        None
    } else {
        Some(alias_part)
    };

    WikiLink {
        alias,
        char_end: 0,
        char_start: 0,
        heading,
        line_end: 0,
        line_start: 0,
        raw_target: target_part.trim().to_string(),
        text: format!("[[{raw}]]"),
        target,
    }
}
/// Checks if a line is a code fence.
#[must_use]
fn is_fence_line(line: &str) -> bool {
    FENCE_RE.is_match(line)
}

/// Checks if a line is a heading.
#[must_use]
fn is_heading_line(line: &str) -> bool {
    HEADING_RE.is_match(line)
}
