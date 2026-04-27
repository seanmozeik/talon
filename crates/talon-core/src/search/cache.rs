//! Query variant helpers used by the hybrid search pipeline.

use crate::text::nfd;

/// Trims, lowercases, and dedupes a list of query variants. Empty strings
/// are dropped. Order is preserved (first occurrence wins).
#[must_use]
pub fn dedupe_query_variants(variants: &[String]) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for v in variants {
        let normalized: String = v.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() {
            continue;
        }
        let key = nfd::normalize(&normalized).to_lowercase();
        if seen.insert(key) {
            out.push(normalized);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupe_drops_empty_and_duplicates_case_insensitive() {
        let input = vec![
            "  zettelkasten ".into(),
            "Zettelkasten".into(),
            String::new(),
            "  ".into(),
            "atomic notes".into(),
            "atomic   notes".into(),
        ];
        let out = dedupe_query_variants(&input);
        assert_eq!(out, vec!["zettelkasten", "atomic notes"]);
    }

    #[test]
    fn dedupe_preserves_first_occurrence_form() {
        let input = vec!["Foo Bar".into(), "foo bar".into()];
        let out = dedupe_query_variants(&input);
        assert_eq!(out, vec!["Foo Bar"]);
    }
}
