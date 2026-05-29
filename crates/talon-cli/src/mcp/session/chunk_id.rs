use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Derives a deterministic chunk ID from note path and snippet prefix.
///
/// # Stability warning
///
/// `DefaultHasher` is not guaranteed to be stable across Rust versions or
/// compilations. This is a **temporary** chunk ID scheme intended as a
/// placeholder until stable DB-assigned chunk IDs are available. Do not
/// persist these IDs across process restarts or compiler updates.
#[must_use]
pub fn derive_chunk_id(path: &str, rank: usize, snippet_prefix: &str) -> String {
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    rank.hash(&mut h);
    // Hash a char-boundary-safe prefix (first 64 chars) of the snippet. A raw
    // byte slice here panicked on multibyte content (e.g. an em dash whose bytes
    // straddle index 64) — TOO-44. `char_indices().nth(64)` always lands on a
    // boundary, so the slice never splits a char.
    let cap = snippet_prefix
        .char_indices()
        .nth(64)
        .map_or(snippet_prefix.len(), |(i, _)| i);
    snippet_prefix[..cap].hash(&mut h);
    format!("tmp_{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::derive_chunk_id;

    #[test]
    fn multibyte_at_truncation_boundary_does_not_panic() {
        // An em dash (3 bytes) straddling the 64th byte used to panic on a raw
        // byte slice. `.chars().take(64)` must handle it cleanly (TOO-44).
        let snippet = format!("{}— tail", "a".repeat(63));
        let id = derive_chunk_id("wiki/x.md", 0, &snippet);
        assert!(id.starts_with("tmp_"));
    }

    #[test]
    fn shorter_than_cap_is_stable() {
        let a = derive_chunk_id("wiki/x.md", 1, "short snippet");
        let b = derive_chunk_id("wiki/x.md", 1, "short snippet");
        assert_eq!(a, b);
    }
}
