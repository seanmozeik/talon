//! DOM-matchable text builder for match anchors.
//!
//! Ports `buildMatchText` from obsidian-hybrid-search (MIT licensed,
//! src/chunker.ts:164-199). Strips Markdown syntax to produce the same text
//! that `textContent` would return from a rendered Obsidian DOM block.

/// Returns the first 80 characters of `text` after stripping Markdown syntax
/// so that downstream consumers can match the string against a rendered DOM.
///
/// Strips (in order):
/// - Heading markers (`# …` lines)
/// - Fenced code block fences (`` ``` `` and `~~~`)
/// - Callout-only lines (`> [!type]`)
/// - HTML tags (`<…>`)
/// - Footnote references (`[^1]`)
/// - Embed wikilinks (`![[…]]`)
/// - Image embeds (`![alt](url)`)
/// - Regular wikilinks (resolved to alias or target: `[[Target|Alias]]` → `Alias`)
/// - Inline links (keep inner text: `[text](url)` → `text`)
/// - Bold/italic (`**`, `__`, `*`, `_`)
/// - Inline code (`` `code` ``)
/// - List markers (`- `, `* `, `1. `)
/// - Task checkboxes (`[ ] `, `[x] `)
#[must_use]
pub fn build_match_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;

    for line in text.lines() {
        let trimmed = line.trim();

        // Fenced code block toggle
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }

        // Heading lines
        if trimmed.starts_with('#') {
            continue;
        }

        // Callout-only lines: `> [!type]` or `> [!type] Title`
        {
            let s = trimmed.strip_prefix('>').map_or("", str::trim);
            if s.starts_with("[!") {
                continue;
            }
        }

        // Strip the line-level syntax and accumulate
        let cleaned = strip_inline(line);
        if !cleaned.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&cleaned);
        }
    }

    // Return first 80 chars (char-boundary safe)
    if out.chars().count() <= 80 {
        out
    } else {
        out.chars().take(80).collect()
    }
}

/// Strips inline Markdown syntax from a single line.
fn strip_inline(line: &str) -> String {
    let s = line.trim();

    // Blockquote prefix
    let s = s.strip_prefix('>').map_or(s, str::trim);

    // List markers: `- `, `* `, `+ `, `1. `
    let s = strip_list_marker(s);

    // Task checkboxes: `[ ] ` / `[x] ` / `[X] `
    let s = strip_task_checkbox(s);

    // Now process char-by-char for inline syntax
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            // HTML tags
            '<' => {
                skip_until(&mut chars, '>');
            }
            // Embed wikilink `![[…]]` or image `![alt](url)`
            '!' => {
                if chars.peek() == Some(&'[') {
                    chars.next(); // consume `[`
                    if chars.peek() == Some(&'[') {
                        // embed wikilink — skip until `]]`
                        chars.next();
                        skip_until_double(&mut chars, ']');
                    } else {
                        // image `![alt](url)` — skip alt and url
                        skip_until(&mut chars, ']');
                        if chars.peek() == Some(&'(') {
                            chars.next();
                            skip_until(&mut chars, ')');
                        }
                    }
                } else {
                    result.push(c);
                }
            }
            // Wikilink `[[Target|Alias]]` or `[[Target]]`, inline link `[text](url)`,
            // footnote ref `[^…]`
            '[' => {
                if chars.peek() == Some(&'[') {
                    // Wikilink
                    chars.next(); // consume second `[`
                    let inner = collect_until_double(&mut chars, ']');
                    // Use alias if present, else target
                    let display = inner
                        .split_once('|')
                        .map_or(inner.as_str(), |(_, alias)| alias.trim());
                    result.push_str(display);
                } else if chars.peek() == Some(&'^') {
                    // Footnote reference — skip entirely
                    skip_until(&mut chars, ']');
                } else {
                    // Inline link [text](url)
                    let text = collect_until(&mut chars, ']');
                    result.push_str(&text);
                    if chars.peek() == Some(&'(') {
                        chars.next();
                        skip_until(&mut chars, ')');
                    }
                }
            }
            // Bold/italic: `**`, `__`, `*`, `_`
            '*' | '_' => {
                if chars.peek() == Some(&c) {
                    chars.next(); // consume second marker
                }
                // skip — delimiters only, no content
            }
            // Inline code `` `code` ``
            '`' => {
                let delim = if chars.peek() == Some(&'`') {
                    chars.next();
                    if chars.peek() == Some(&'`') {
                        chars.next();
                        "```"
                    } else {
                        "``"
                    }
                } else {
                    "`"
                };
                let code = collect_until_str(&mut chars, delim);
                result.push_str(&code);
            }
            other => result.push(other),
        }
    }

    // Collapse runs of whitespace that emerge after stripping inline syntax (e.g.
    // `![[embed]]` leaves the surrounding spaces, producing `word  word`).
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_list_marker(s: &str) -> &str {
    // Unordered
    if let Some(rest) = s
        .strip_prefix("- ")
        .or_else(|| s.strip_prefix("* "))
        .or_else(|| s.strip_prefix("+ "))
    {
        return rest;
    }
    // Ordered: digits followed by `. `
    let mut end = 0;
    for (i, c) in s.char_indices() {
        if c.is_ascii_digit() {
            end = i + 1;
        } else {
            break;
        }
    }
    if end > 0
        && let Some(rest) = s[end..].strip_prefix(". ")
    {
        return rest;
    }
    s
}

fn strip_task_checkbox(s: &str) -> &str {
    // Matches `[ ] `, `[x] `, `[X] `
    if s.starts_with('[') && s.len() >= 4 {
        let third = s.as_bytes().get(2).copied();
        let fourth = s.as_bytes().get(3).copied();
        if s.as_bytes().get(1).copied() != Some(b']')
            && s.as_bytes().get(1).copied().is_some()
            && third == Some(b']')
            && fourth == Some(b' ')
        {
            return &s[4..];
        }
    }
    s
}

/// Advance `chars` past the next occurrence of `end`.
fn skip_until<I: Iterator<Item = char>>(chars: &mut I, end: char) {
    for c in chars.by_ref() {
        if c == end {
            break;
        }
    }
}

/// Advance `chars` past the next `]]`.
fn skip_until_double<I: Iterator<Item = char>>(chars: &mut std::iter::Peekable<I>, end: char) {
    while let Some(c) = chars.next() {
        if c == end && chars.peek() == Some(&end) {
            chars.next();
            break;
        }
    }
}

/// Collect chars until `end`, returning the collected string (exclusive of `end`).
fn collect_until<I: Iterator<Item = char>>(chars: &mut I, end: char) -> String {
    let mut buf = String::new();
    for c in chars.by_ref() {
        if c == end {
            break;
        }
        buf.push(c);
    }
    buf
}

/// Collect chars until `]]`, returning the collected string.
fn collect_until_double<I: Iterator<Item = char>>(
    chars: &mut std::iter::Peekable<I>,
    end: char,
) -> String {
    let mut buf = String::new();
    while let Some(c) = chars.next() {
        if c == end && chars.peek() == Some(&end) {
            chars.next();
            break;
        }
        buf.push(c);
    }
    buf
}

/// Collect chars until string delimiter `delim`, returning the collected string.
fn collect_until_str<I: Iterator<Item = char>>(
    chars: &mut std::iter::Peekable<I>,
    delim: &str,
) -> String {
    let mut buf = String::new();
    // Simple single-char delimiters only (backtick is always 1 char in the actual delim string).
    let end_char = delim.chars().next().unwrap_or('`');
    while let Some(c) = chars.next() {
        if c == end_char {
            // For multi-char delimiters (`` `` ``), consume remaining
            for _ in 1..delim.chars().count() {
                if chars.peek() == Some(&end_char) {
                    chars.next();
                }
            }
            break;
        }
        buf.push(c);
    }
    buf
}

#[cfg(test)]
mod tests;
