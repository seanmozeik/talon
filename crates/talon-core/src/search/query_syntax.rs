use crate::text::normalize_keyword;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuerySyntax {
    pub query: String,
    pub tags: Vec<String>,
    pub headings: Vec<String>,
}

#[must_use]
pub fn parse_query_syntax(query: &str) -> QuerySyntax {
    let mut parsed = QuerySyntax::default();
    let mut remaining = Vec::new();

    for token in query.split_whitespace() {
        if let Some(tag) = tag_token(token) {
            parsed.tags.push(tag);
        } else if let Some(heading) = heading_token(token) {
            parsed.headings.push(heading);
        } else {
            remaining.push(token);
        }
    }

    parsed.query = remaining.join(" ");
    if parsed.query.is_empty() {
        parsed.query = parsed
            .headings
            .iter()
            .chain(parsed.tags.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
    }

    parsed
}

fn tag_token(token: &str) -> Option<String> {
    let value = token
        .strip_prefix("tag:")
        .or_else(|| token.strip_prefix('#'))?
        .trim();
    clean_query_value(value).filter(|value| !value.is_empty())
}

fn heading_token(token: &str) -> Option<String> {
    let value = token
        .strip_prefix("heading:")
        .or_else(|| token.strip_prefix("h:"))?
        .trim();
    clean_query_value(value).filter(|value| !value.is_empty())
}

fn clean_query_value(value: &str) -> Option<String> {
    let cleaned = value
        .trim_matches(|ch| matches!(ch, '"' | '\'' | '[' | ']'))
        .trim()
        .to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

#[must_use]
pub fn normalize_tag_filter(tag: &str) -> String {
    let trimmed = tag.trim();
    let without_hash = trimmed.strip_prefix('#').unwrap_or(trimmed);
    normalize_keyword(without_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_syntax_extracts_tags_and_headings() {
        let parsed = parse_query_syntax("tag:fermentation #hot-sauce heading:Targets bottle");

        assert_eq!(parsed.query, "bottle");
        assert_eq!(parsed.tags, vec!["fermentation", "hot-sauce"]);
        assert_eq!(parsed.headings, vec!["Targets"]);
    }

    #[test]
    fn query_syntax_uses_filters_as_query_when_no_free_text_remains() {
        let parsed = parse_query_syntax("#fermentation heading:Targets");

        assert_eq!(parsed.query, "Targets fermentation");
        assert_eq!(parsed.tags, vec!["fermentation"]);
        assert_eq!(parsed.headings, vec!["Targets"]);
    }
}
