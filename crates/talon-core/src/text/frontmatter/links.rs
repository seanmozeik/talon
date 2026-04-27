use super::{FENCE_PATTERN, HEADING_PATTERN, INLINE_TAG_PATTERN, WIKILINK_PATTERN, WikiLink};

/// Extracts inline `#tag` syntax from markdown body (outside code fences).
#[allow(clippy::expect_used)]
pub(super) fn extract_inline_tags(content: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut inside_fence = false;
    let re = regex::Regex::new(INLINE_TAG_PATTERN).expect("valid regex");

    for line in content.lines() {
        let trimmed = line.trim_start();

        // Toggle fence state
        if is_fence_line(trimmed) {
            inside_fence = !inside_fence;
            continue;
        }

        // Only parse tags outside fences and outside headings
        if !inside_fence && !is_heading_line(trimmed) {
            for capture in re.captures_iter(trimmed) {
                if let Some(tag_match) = capture.get(2) {
                    let raw_tag = tag_match.as_str();
                    // Strip trailing punctuation
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
#[allow(clippy::expect_used, clippy::cast_possible_truncation)]
pub fn extract_wikilinks(content: &str) -> Vec<WikiLink> {
    let mut links = Vec::new();
    let mut inside_fence = false;

    let re = regex::Regex::new(WIKILINK_PATTERN).expect("valid regex");

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();

        if is_fence_line(trimmed) {
            inside_fence = !inside_fence;
            continue;
        }

        if !inside_fence {
            let char_offset = content
                .lines()
                .take(line_num)
                .map(|l| l.len() + 1) // +1 for newline
                .sum::<usize>();

            for caps in re.captures_iter(line) {
                let raw_target = caps.get(1).map_or("", |m| m.as_str());

                let parsed = parse_wiki_link(raw_target);
                let line_start = (line_num + 1) as u32;
                let line_end = line_start;

                let full_match = caps.get(0).expect("capture group 0 always exists");
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
    }

    links
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
#[allow(clippy::unwrap_used)]
fn is_fence_line(line: &str) -> bool {
    regex::Regex::new(FENCE_PATTERN).unwrap().is_match(line)
}

/// Checks if a line is a heading.
#[must_use]
#[allow(clippy::unwrap_used)]
fn is_heading_line(line: &str) -> bool {
    regex::Regex::new(HEADING_PATTERN).unwrap().is_match(line)
}
