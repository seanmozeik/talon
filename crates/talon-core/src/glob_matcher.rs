//! Case-insensitive glob matching, shared by indexer scan filters and query where-filters.

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

/// Builds a case-insensitive [`GlobSet`] from a single pattern string.
#[must_use]
pub fn build_case_insensitive(pattern: &str) -> Option<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    let glob = GlobBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .ok()?;
    builder.add(glob);
    builder.build().ok()
}

/// Checks if `text` matches a case-insensitive glob `pattern`.
/// Returns `None` if the pattern is invalid.
#[must_use]
pub fn glob_match_case_insensitive(pattern: &str, text: &str) -> Option<bool> {
    build_case_insensitive(pattern).map(|set| set.is_match(text))
}

#[cfg(test)]
mod tests {
    use super::glob_match_case_insensitive as glob_match;

    #[test]
    fn case_insensitive_path_glob() {
        assert!(glob_match("wiki/**", "Wiki/base.md").unwrap_or(false));
        assert!(glob_match("WIKI/**", "wiki/nested/deep.md").unwrap_or(false));
        assert!(glob_match("Patients/**", "patients/base.md").unwrap_or(false));
    }

    #[test]
    fn case_insensitive_exact_prefix() {
        assert!(glob_match("Templates/**", "templates/Daily.md").unwrap_or(false));
        assert!(!glob_match("Templates/**", "other/file.md").unwrap_or(false));
    }

    #[test]
    fn invalid_pattern_returns_none() {
        assert!(glob_match("[invalid", "anything").is_none());
    }
}
