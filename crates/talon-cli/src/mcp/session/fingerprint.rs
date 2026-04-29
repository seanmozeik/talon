use std::collections::HashSet;

/// Normalized query fingerprint for turn deduplication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryFingerprint {
    pub normalized: String,
    pub token_set: HashSet<String>,
}

impl QueryFingerprint {
    #[must_use]
    pub fn from_message(message: &str) -> Self {
        let normalized = normalize(message);
        let token_set = tokenize(&normalized);
        Self {
            normalized,
            token_set,
        }
    }

    /// Jaccard similarity in [0.0, 1.0].
    #[must_use]
    pub fn similarity(&self, other: &Self) -> f64 {
        if self.token_set.is_empty() && other.token_set.is_empty() {
            return 1.0;
        }
        let intersection = self.token_set.intersection(&other.token_set).count();
        let union = self.token_set.union(&other.token_set).count();
        if union == 0 {
            1.0
        } else {
            #[expect(
                clippy::cast_precision_loss,
                reason = "precision loss is acceptable for word-token similarity scores"
            )]
            let result = intersection as f64 / union as f64;
            result
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.normalized
    }
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn tokenize(s: &str) -> HashSet<String> {
    s.split_whitespace().map(String::from).collect()
}

#[cfg(test)]
mod tests {
    use super::QueryFingerprint;

    #[test]
    fn identical_messages_have_similarity_one() {
        let a = QueryFingerprint::from_message("how does recall work");
        let b = QueryFingerprint::from_message("how does recall work");
        let sim = a.similarity(&b);
        assert!(
            (sim - 1.0).abs() < f64::EPSILON,
            "expected similarity 1.0 for identical messages, got {sim}"
        );
    }

    #[test]
    fn empty_message_similarity() {
        let a = QueryFingerprint::from_message("");
        let b = QueryFingerprint::from_message("");
        let sim = a.similarity(&b);
        assert!(
            (sim - 1.0).abs() < f64::EPSILON,
            "expected similarity 1.0 for two empty messages, got {sim}"
        );
    }

    #[test]
    fn different_messages_have_lower_similarity() {
        let a = QueryFingerprint::from_message("how does recall work in talon");
        let b = QueryFingerprint::from_message("what is the vault indexing strategy");
        let sim = a.similarity(&b);
        assert!(
            sim < 0.5,
            "expected similarity < 0.5 for very different messages, got {sim}"
        );
    }
}
