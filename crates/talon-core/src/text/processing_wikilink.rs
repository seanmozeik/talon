//! Wikilink parsing helpers.

/// Minimum length for outer quote stripping.
const MIN_QUOTED_LENGTH: usize = 2;

/// Parsed components of a wikilink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedWikiLink {
    /// Display alias (if `[[target|alias]]`).
    pub alias: Option<String>,
    /// Section heading anchor (if `[[target#heading]]`).
    pub heading: Option<String>,
    /// Raw target part before `|` or `#`.
    pub raw_target: String,
    /// The resolved target (without alias or heading).
    pub target: String,
}

/// Parses a raw wikilink string into components.
///
/// Handles `[[target]]`, `[[target|alias]]`, and `[[target#heading]]`.
///
/// # Examples
///
/// ```
/// use talon_core::text::parse_wikilink;
///
/// let link = parse_wikilink("My Note");
/// assert_eq!(link.target, "My Note");
/// assert_eq!(link.alias, None);
/// assert_eq!(link.heading, None);
///
/// let link = parse_wikilink("Target|alias");
/// assert_eq!(link.target, "Target");
/// assert_eq!(link.alias, Some("alias".to_string()));
///
/// let link = parse_wikilink("Target#heading");
/// assert_eq!(link.target, "Target");
/// assert_eq!(link.heading, Some("heading".to_string()));
/// ```
#[must_use]
pub fn parse_wikilink(raw: &str) -> ParsedWikiLink {
    let (target_part, alias_part) = raw
        .find('|')
        .map_or((raw, ""), |i| (&raw[..i], &raw[i + 1..]));
    let (target, heading) = target_part.find('#').map_or_else(
        || (target_part.trim(), None),
        |i| {
            let target = target_part[..i].trim();
            let heading = target_part[i + 1..].trim();
            (
                target,
                if heading.is_empty() {
                    None
                } else {
                    Some(heading.to_string())
                },
            )
        },
    );
    let alias = (!alias_part.is_empty()).then(|| alias_part.trim().to_string());

    ParsedWikiLink {
        alias,
        heading,
        raw_target: target_part.trim().to_string(),
        target: target.to_string(),
    }
}

/// Strips outer matching quotes from a string.
///
/// Only strips if the string starts and ends with the same quote character
/// (`"` or `'`) and has at least 2 characters after trimming.
///
/// # Examples
///
/// ```
/// use talon_core::text::strip_outer_quotes;
///
/// assert_eq!(strip_outer_quotes("\"hello\""), "hello");
/// assert_eq!(strip_outer_quotes("'hello'"), "hello");
/// assert_eq!(strip_outer_quotes("hello"), "hello");
/// assert_eq!(strip_outer_quotes("\""), "\"");
/// ```
#[must_use]
pub fn strip_outer_quotes(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() < MIN_QUOTED_LENGTH {
        return trimmed.to_string();
    }
    let first = trimmed.chars().next().unwrap_or('\0');
    let last = trimmed.chars().last().unwrap_or('\0');
    if (first == '"' || first == '\'') && first == last {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}
