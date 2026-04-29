use std::collections::{BTreeMap, HashSet};

use super::links::{extract_inline_tags, extract_wikilinks};
use super::{
    FRONTMATTER_DELIM_START, FrontmatterExtract, FrontmatterValue, MIN_QUOTED_LENGTH,
    normalize_keyword,
};

/// Parses YAML frontmatter from markdown content.
///
/// Returns the body, parsed frontmatter map, and raw YAML text.
/// Handles Obsidian scalar quoting (values containing `:` get double-quoted).
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
    let mut parsed_frontmatter: Option<BTreeMap<String, serde_yaml_ng::Value>> = None;

    if let Ok(map) = serde_yaml_ng::from_str::<BTreeMap<String, serde_yaml_ng::Value>>(&parsed) {
        for (key, value) in &map {
            frontmatter.insert(key.clone(), serde_value_to_fm(value.clone()));
        }
        parsed_frontmatter = Some(map);
    } else if let Ok(map) = serde_yaml_ng::from_str::<BTreeMap<String, serde_yaml_ng::Value>>(&raw)
    {
        for (key, value) in &map {
            frontmatter.insert(key.clone(), serde_value_to_fm(value.clone()));
        }
        parsed_frontmatter = Some(map);
    }

    let aliases = extract_aliases(&frontmatter);
    let (fm_tags, inline_tags) = extract_tags(&frontmatter, parsed_frontmatter.as_ref(), &body);
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
        serde_yaml_ng::Value::String(s) => {
            if is_date_value(&s) {
                FrontmatterValue::Date(s)
            } else {
                FrontmatterValue::String(s)
            }
        }
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

fn is_date_value(value: &str) -> bool {
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).is_ok()
        || time::Date::parse(
            value,
            time::macros::format_description!("[year]-[month]-[day]"),
        )
        .is_ok()
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
    parsed_frontmatter: Option<&BTreeMap<String, serde_yaml_ng::Value>>,
    body: &str,
) -> (Vec<String>, Vec<String>) {
    let mut fm_tags = Vec::new();
    if let Some(parsed) = parsed_frontmatter {
        for key in &["tags", "tag"] {
            if let Some(value) = parsed.get(*key) {
                fm_tags.extend(extract_string_tags_from_yaml(value));
            }
        }
    } else {
        for key in &["tags", "tag"] {
            if let Some(FrontmatterValue::List(list)) = frontmatter.get(*key) {
                fm_tags.extend(list.iter().cloned());
            }
            if let Some(FrontmatterValue::String(s)) = frontmatter.get(*key) {
                fm_tags.extend(split_list_text(s));
            }
        }
    }

    let inline_tags = extract_inline_tags(body);

    (fm_tags, inline_tags)
}

fn extract_string_tags_from_yaml(value: &serde_yaml_ng::Value) -> Vec<String> {
    match value {
        serde_yaml_ng::Value::String(s) => split_list_text(s),
        serde_yaml_ng::Value::Sequence(seq) => seq
            .iter()
            .filter_map(|item| match item {
                serde_yaml_ng::Value::String(s) => Some(split_list_text(s)),
                _ => None,
            })
            .flatten()
            .collect(),
        _ => Vec::new(),
    }
}

fn normalize_unique_list(fm_tags: Vec<String>, inline_tags: Vec<String>) -> Vec<String> {
    let mut all = fm_tags;
    all.extend(inline_tags);

    let mut seen = HashSet::new();
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
