//! Frontmatter store types and parsing.
//!
//! Implements YAML frontmatter extraction from markdown files, with Obsidian-compatible
//! scalar quoting, tag extraction (frontmatter + inline), and alias normalization.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::nfd;

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
    nfd::normalize(value.trim())
        .to_lowercase()
        .replace(char::is_whitespace, "")
}

/// Normalizes a vault path: backslashes to forward slashes, NFD normalization, lowercase.
#[must_use]
pub fn normalize_vault_path(value: &str) -> String {
    nfd::normalize(&value.replace('\\', "/")).to_lowercase()
}

mod links;
mod parse;

pub use links::extract_wikilinks;
pub use parse::parse_frontmatter;

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
mod tests;
