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
    // Use first 64 chars of snippet as prefix for stability
    snippet_prefix[..snippet_prefix.len().min(64)].hash(&mut h);
    format!("tmp_{:016x}", h.finish())
}
