//! Frontmatter store types and parsing.
//!
//! Implements YAML frontmatter extraction from markdown files, with Obsidian-compatible
//! scalar quoting, tag extraction (frontmatter + inline), and alias normalization.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

// ── Constants ───────────────────────────────────────────────────────────────

/// Pattern: `---\n...\n---` or `---\n...\n...`
const FRONTMATTER_DELIM_START: &str = "---";

/// Inline tag pattern: `#tag-name` in markdown body (outside code fences).
const INLINE_TAG_PATTERN: &str = r"(?u)(^|\s)#([\p{L}\p{N}_/.-]+)";

/// Wikilink pattern: `[[target|alias]]` or `[[target]]`.
const WIKILINK_PATTERN: &str = r"\[\[([^\]]+)\]\]";

/// Fence pattern: triple backtick or triple tilde lines.
const FENCE_PATTERN: &str = r"(?u)^(```+|~~~+)\s*.*$";

/// Heading pattern: `# ` through `###### `.
const HEADING_PATTERN: &str = r"(?u)^#{1,6}\s+(.*)$";

/// Minimum length for outer quote stripping.
const MIN_QUOTED_LENGTH: usize = 2;

/// Token-to-character ratio for rough token estimation.
pub const TOKEN_CHAR_RATIO: u8 = 4;

// ── Frontmatter parsing ─────────────────────────────────────────────────────

/// Result of frontmatter extraction from a markdown file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrontmatterExtract {
    /// The document body (content after the closing `---`).
    pub body: String,
    /// Parsed frontmatter key-value pairs.
    pub frontmatter: BTreeMap<String, FrontmatterValue>,
    /// Raw YAML text between delimiters.
    pub frontmatter_raw: String,
    /// Normalized aliases extracted from `aliases` / `alias` keys.
    pub aliases: Vec<String>,
    /// Tags from both frontmatter (`tags` / `tag`) and inline `#tag` syntax.
    pub tags: Vec<String>,
    /// Raw wikilinks found in the document body.
    pub links: Vec<WikiLink>,
}

/// A single frontmatter value, typed for storage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FrontmatterValue {
    /// String value.
    String(String),
    /// Numeric value (int or float).
    Number(f64),
    /// Boolean value.
    Boolean(bool),
    /// Date/time value (ISO 8601 string).
    Date(String),
    /// List of string values (flattened).
    List(Vec<String>),
}

impl Eq for FrontmatterValue {}

/// A single wikilink extracted from markdown content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiLink {
    /// Display alias (if `[[target|alias]]`).
    pub alias: Option<String>,
    /// Character offset where the link ends.
    pub char_end: usize,
    /// Character offset where the link starts.
    pub char_start: usize,
    /// Section heading anchor (if `[[target#heading]]`).
    pub heading: Option<String>,
    /// Line number where the link appears (1-indexed).
    pub line_end: u32,
    /// Line number where the link starts (1-indexed).
    pub line_start: u32,
    /// Raw target part before `|` or `#`.
    pub raw_target: String,
    /// The link text (`[[...]]` full match).
    pub text: String,
    /// The resolved target (without alias or heading).
    pub target: String,
}

/// Normalizes a Talon keyword for comparison: NFD normalization + lowercase + trim.
///
/// Matches the TS `normalizeTalonKeyword` behavior exactly.
#[must_use]
pub fn normalize_keyword(value: &str) -> String {
    value
        .to_lowercase()
        .replace(char::is_whitespace, "")
        .trim()
        .to_string()
}

/// Normalizes a vault path: backslashes to forward slashes, NFD normalization, lowercase.
#[must_use]
pub fn normalize_vault_path(value: &str) -> String {
    value
        .replace('\\', "/")
        .nfd()
        .collect::<String>()
        .to_lowercase()
}

/// Parses YAML frontmatter from markdown content.
///
/// Returns the body, parsed frontmatter map, and raw YAML text.
/// Handles Obsidian scalar quoting (values with colons get double-quoted).
///
/// # Algorithm
/// 1. Match `---\n...\n---` or `---\n...\n...` at start of content.
/// 2. Normalize Obsidian scalars (values containing `:` get quoted).
/// 3. Parse with `serde_yaml_ng`.
/// 4. If parsing fails, try raw parse, then return empty frontmatter.
#[must_use]
pub fn parse_frontmatter(content: &str) -> FrontmatterExtract {
    let (body, raw) = extract_frontmatter_yaml(content);

    let parsed = normalize_obsidian_scalars(&raw);
    let mut frontmatter: BTreeMap<String, FrontmatterValue> = BTreeMap::new();

    if let Ok(map) = serde_yaml_ng::from_str::<BTreeMap<String, serde_yaml_ng::Value>>(&parsed) {
        for (key, value) in map {
            frontmatter.insert(key, serde_value_to_fm(value));
        }
    } else if let Ok(map) = serde_yaml_ng::from_str::<BTreeMap<String, serde_yaml_ng::Value>>(&raw)
    {
        for (key, value) in map {
            frontmatter.insert(key, serde_value_to_fm(value));
        }
    }

    let aliases = extract_aliases(&frontmatter);
    let (fm_tags, inline_tags) = extract_tags(&frontmatter, &body);
    let tags = normalize_unique_list(fm_tags, inline_tags);
    let links = extract_wikilinks(&body);

    FrontmatterExtract {
        body,
        frontmatter,
        frontmatter_raw: raw,
        aliases,
        tags,
        links,
    }
}

/// Extracts YAML frontmatter block from markdown content.
fn extract_frontmatter_yaml(content: &str) -> (String, String) {
    let start_marker = FRONTMATTER_DELIM_START;
    let start = content.find(start_marker);

    let Some(start_pos) = start else {
        return (content.to_string(), String::new());
    };

    let after_start = &content[start_pos + start_marker.len()..];
    // Skip leading newline
    let after_start = after_start.strip_prefix('\n').unwrap_or(after_start);

    // Find end marker (--- or ...)
    let Some(end) = find_end_marker(after_start) else {
        return (content.to_string(), String::new());
    };

    let raw = &after_start[..end];
    let _body = content[start_pos + start_marker.len()..]
        .trim_start()
        .to_string();

    // Find the actual end position in original content
    let total_prefix = start_pos + start_marker.len() + 1; // +1 for newline
    let body_start = total_prefix + end;
    let body_end = body_start + 3; // Both --- and ... are 3 chars

    let body = content[body_end..].to_string();

    (body, raw.to_string())
}

fn find_end_marker(s: &str) -> Option<usize> {
    let lines: Vec<&str> = s.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed == "---" || trimmed == "..." {
            // Calculate position including the newline
            let mut pos = 0;
            for (j, l) in lines.iter().enumerate() {
                if j < i {
                    pos += l.len() + 1; // +1 for newline
                } else {
                    break;
                }
            }
            return Some(pos);
        }
    }
    None
}

/// Normalizes Obsidian YAML scalars: values containing `:` get double-quoted.
fn normalize_obsidian_scalars(raw: &str) -> String {
    let mut out = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim_start();
        // Match key: value pattern
        if let Some(colon_pos) = trimmed.find(':') {
            let key = &trimmed[..colon_pos];
            let value = &trimmed[colon_pos + 1..];

            // Skip if key has spaces (not a scalar)
            if key.contains(' ') {
                out.push(line.to_string());
                continue;
            }

            let value = value.strip_prefix(' ').unwrap_or(value);
            if needs_scalar_quoting(value) {
                let escaped = escape_double_quoted(value);
                out.push(format!("{}: \"{}\"", line[..colon_pos].trim_end(), escaped));
            } else {
                out.push(line.to_string());
            }
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

fn needs_scalar_quoting(value: &str) -> bool {
    let t = value.trim();
    if t.is_empty() {
        return false;
    }
    // Already quoted
    if (t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\'')) {
        return false;
    }
    // Starts with [ or { (list/mapping)
    if t.starts_with('[') || t.starts_with('{') {
        return false;
    }
    // Block scalar indicators
    if t.starts_with('|') || t.starts_with('>') {
        return false;
    }
    // Anchor/alias
    if t.starts_with('&') || t.starts_with('*') {
        return false;
    }
    // YAML booleans/null
    if matches!(
        t.to_lowercase().as_str(),
        "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~"
    ) {
        return false;
    }
    t.contains(':')
}

fn escape_double_quoted(value: &str) -> String {
    value.trim().replace('\\', "\\\\").replace('"', "\\\"")
}

fn serde_value_to_fm(value: serde_yaml_ng::Value) -> FrontmatterValue {
    match value {
        serde_yaml_ng::Value::String(s) => FrontmatterValue::String(s),
        serde_yaml_ng::Value::Number(n) => FrontmatterValue::Number(n.as_f64().unwrap_or_default()),
        serde_yaml_ng::Value::Bool(b) => FrontmatterValue::Boolean(b),
        serde_yaml_ng::Value::Null | serde_yaml_ng::Value::Tagged(_) => {
            FrontmatterValue::String(String::new())
        }
        serde_yaml_ng::Value::Sequence(seq) => {
            let flattened = flatten_list_value(&seq);
            FrontmatterValue::List(flattened)
        }
        serde_yaml_ng::Value::Mapping(map) => {
            let inner: Vec<serde_yaml_ng::Value> = map.iter().map(|(_, v)| v.clone()).collect();
            let flattened = flatten_list_value(&inner);
            FrontmatterValue::List(flattened)
        }
    }
}

/// Flattens a list of values into a list of strings.
fn flatten_list_value(values: &[serde_yaml_ng::Value]) -> Vec<String> {
    let mut result = Vec::new();
    for value in values {
        match value {
            serde_yaml_ng::Value::String(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    continue;
                }
                // Check if it's JSON that needs parsing
                if (trimmed.starts_with('{')
                    || trimmed.starts_with('[')
                    || (trimmed.starts_with('"') && trimmed.ends_with('"')))
                    && let Ok(parsed) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(trimmed)
                {
                    result.extend(flatten_list_value(&[parsed]));
                    continue;
                }
                // Split comma/newline separated values
                for item in split_list_text(trimmed) {
                    result.push(item);
                }
            }
            serde_yaml_ng::Value::Number(n) => {
                result.push(n.to_string());
            }
            serde_yaml_ng::Value::Bool(b) => {
                result.push(b.to_string());
            }
            serde_yaml_ng::Value::Sequence(seq) => {
                result.extend(flatten_list_value(seq));
            }
            serde_yaml_ng::Value::Mapping(map) => {
                let inner: Vec<serde_yaml_ng::Value> = map.iter().map(|(_, v)| v.clone()).collect();
                result.extend(flatten_list_value(&inner));
            }
            serde_yaml_ng::Value::Tagged(_) | serde_yaml_ng::Value::Null => {
                // Skip tagged and null values
            }
        }
    }
    result
}

fn split_list_text(value: &str) -> Vec<String> {
    value
        .split([',', '\n'])
        .map(|item| strip_outer_quotes(item.trim()))
        .filter(|item| !item.is_empty())
        .collect()
}

fn strip_outer_quotes(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() < MIN_QUOTED_LENGTH {
        return trimmed.to_string();
    }
    let first = trimmed.chars().next().unwrap_or('\0');
    let last = trimmed.chars().last().unwrap_or('\0');
    if (first == '"' || first == '\'') && first == last {
        return trimmed[1..trimmed.len() - 1].to_string();
    }
    trimmed.to_string()
}

/// Extracts aliases from frontmatter (aliases / alias keys).
fn extract_aliases(frontmatter: &BTreeMap<String, FrontmatterValue>) -> Vec<String> {
    let mut values = Vec::new();
    for key in &["aliases", "alias"] {
        if let Some(FrontmatterValue::List(list)) = frontmatter.get(*key) {
            values.extend(list.iter().cloned());
        }
        if let Some(FrontmatterValue::String(s)) = frontmatter.get(*key) {
            values.extend(split_list_text(s));
        }
    }
    normalize_unique_list(values, Vec::new())
}

/// Extracts tags from frontmatter and inline markdown.
fn extract_tags(
    frontmatter: &BTreeMap<String, FrontmatterValue>,
    body: &str,
) -> (Vec<String>, Vec<String>) {
    let mut fm_tags = Vec::new();
    for key in &["tags", "tag"] {
        if let Some(FrontmatterValue::List(list)) = frontmatter.get(*key) {
            fm_tags.extend(list.iter().cloned());
        }
        if let Some(FrontmatterValue::String(s)) = frontmatter.get(*key) {
            fm_tags.extend(split_list_text(s));
        }
    }

    let inline_tags = extract_inline_tags(body);

    (fm_tags, inline_tags)
}

/// Extracts inline `#tag` syntax from markdown body (outside code fences).
#[allow(clippy::expect_used)]
fn extract_inline_tags(content: &str) -> Vec<String> {
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

/// Normalizes and deduplicates a list of strings using keyword normalization.
fn normalize_unique_list(fm_tags: Vec<String>, inline_tags: Vec<String>) -> Vec<String> {
    let mut all = fm_tags;
    all.extend(inline_tags);

    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();

    for value in all {
        let normalized = normalize_keyword(&value);
        if !normalized.is_empty() && !seen.contains(&normalized) {
            seen.insert(normalized);
            result.push(value.trim().to_string());
        }
    }

    result
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

// ── Store types ─────────────────────────────────────────────────────────────

/// Frontmatter entry stored in the database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrontmatterEntry {
    /// Vault-relative path of the note.
    pub path: String,
    /// Frontmatter key.
    pub key: String,
    /// Frontmatter value type.
    #[serde(rename = "type")]
    pub value_type: FrontmatterValueType,
    /// String representation of the value.
    pub value: String,
}

/// Frontmatter value type for database storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrontmatterValueType {
    String,
    Number,
    Bool,
    Date,
    List,
}

impl From<FrontmatterValue> for FrontmatterValueType {
    fn from(value: FrontmatterValue) -> Self {
        match value {
            FrontmatterValue::String(_) => Self::String,
            FrontmatterValue::Number(_) => Self::Number,
            FrontmatterValue::Boolean(_) => Self::Bool,
            FrontmatterValue::Date(_) => Self::Date,
            FrontmatterValue::List(_) => Self::List,
        }
    }
}

/// Reverse index: maps normalized frontmatter values to source paths.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrontmatterReverseIndex {
    /// Maps `key:value` to set of paths.
    pub index: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
}

impl FrontmatterReverseIndex {
    /// Adds a frontmatter entry to the reverse index.
    pub fn insert(&mut self, path: &str, key: &str, value: &str) {
        let normalized_key = normalize_keyword(key);
        let normalized_value = normalize_keyword(value);
        let composite = format!("{normalized_key}:{normalized_value}");
        self.index
            .entry(composite)
            .or_default()
            .insert(path.to_string());
    }

    /// Looks up paths by normalized key:value.
    #[must_use]
    pub fn lookup(&self, key: &str, value: &str) -> Option<&std::collections::BTreeSet<String>> {
        let normalized_key = normalize_keyword(key);
        let normalized_value = normalize_keyword(value);
        self.index
            .get(&format!("{normalized_key}:{normalized_value}"))
    }

    /// Looks up paths by key only (all values for that key).
    #[must_use]
    pub fn lookup_key(&self, key: &str) -> std::collections::BTreeSet<String> {
        let normalized_key = normalize_keyword(key);
        let mut result = std::collections::BTreeSet::new();
        for (composite, paths) in &self.index {
            if composite.starts_with(&format!("{normalized_key}:")) {
                result.extend(paths.iter().cloned());
            }
        }
        result
    }
}

/// Reverse-source index: maps a path to all paths that reference it in frontmatter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReverseSourceIndex {
    /// Maps target path → set of source paths that reference it.
    pub sources: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
}

impl ReverseSourceIndex {
    /// Adds a reverse source reference.
    pub fn insert(&mut self, source: String, target: String) {
        self.sources.entry(target).or_default().insert(source);
    }

    /// Gets all sources that reference a target path.
    #[must_use]
    pub fn get(&self, target: &str) -> Option<&std::collections::BTreeSet<String>> {
        self.sources.get(target)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_keyword() {
        assert_eq!(normalize_keyword("Hello World"), "helloworld");
        assert_eq!(normalize_keyword("  Test  "), "test");
        assert_eq!(normalize_keyword("CAFÉ"), "café");
    }

    #[test]
    fn test_parse_frontmatter_simple() {
        let content = "---\ntitle: Hello\nauthor: World\n---\n\nBody text.";
        let result = parse_frontmatter(content);
        assert_eq!(result.body.trim(), "Body text.");
        assert!(result.frontmatter.contains_key("title"));
        assert!(result.frontmatter.contains_key("author"));
    }

    #[test]
    fn test_parse_frontmatter_no_frontmatter() {
        let content = "No frontmatter here.";
        let result = parse_frontmatter(content);
        assert_eq!(result.body, content);
        assert!(result.frontmatter.is_empty());
    }

    #[test]
    fn test_extract_wikilinks() {
        let content = "Check [[My Note]] and [[Other#section]].\n```\n[[not a link]]\n```\nAnd [[Target|alias]].";
        let links = extract_wikilinks(content);
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].target, "My Note");
        assert_eq!(links[1].target, "Other");
        assert_eq!(links[1].heading, Some("section".to_string()));
        assert_eq!(links[2].target, "Target");
        assert_eq!(links[2].alias, Some("alias".to_string()));
    }

    #[test]
    fn test_extract_inline_tags() {
        let content = "Some text #hello #world and #foo.\n```\n#not a tag\n```";
        let tags = extract_inline_tags(content);
        assert!(tags.contains(&"hello".to_string()));
        assert!(tags.contains(&"world".to_string()));
        assert!(tags.contains(&"foo".to_string()));
    }

    #[test]
    fn test_reverse_index() {
        let mut idx = FrontmatterReverseIndex::default();
        idx.insert("path1.md", "tag", "rust");
        idx.insert("path2.md", "tag", "rust");
        idx.insert("path3.md", "tag", "python");

        let results = idx.lookup("tag", "rust");
        assert!(results.is_some());
        assert_eq!(results.unwrap().len(), 2);

        let results = idx.lookup("tag", "python");
        assert!(results.is_some());
        assert_eq!(results.unwrap().len(), 1);
    }

    #[test]
    fn test_reverse_source_index() {
        let mut idx = ReverseSourceIndex::default();
        idx.insert("a.md".to_string(), "b.md".to_string());
        idx.insert("c.md".to_string(), "b.md".to_string());

        let sources = idx.get("b.md");
        assert!(sources.is_some());
        assert_eq!(sources.unwrap().len(), 2);
    }
}
