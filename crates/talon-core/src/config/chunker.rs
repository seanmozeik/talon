//! Chunker knobs for the `[indexer]` section of `talon.toml`.

use serde::{Deserialize, Serialize};

/// Chunker knobs for the `[indexer]` section of `talon.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkerConfig {
    /// Target chunk size in tokens (default 512).
    #[serde(default = "ChunkerConfig::default_chunk_tokens")]
    pub chunk_tokens: usize,
    /// Overlap in tokens between adjacent chunks (default 64, must be < `chunk_tokens`).
    #[serde(default = "ChunkerConfig::default_chunk_overlap")]
    pub chunk_overlap: usize,
    /// Minimum token count; chunks below this are discarded after splitting (default 16).
    #[serde(default = "ChunkerConfig::default_chunk_min_tokens")]
    pub chunk_min_tokens: usize,
}

impl ChunkerConfig {
    /// Validates chunker invariants from user configuration.
    ///
    /// # Errors
    ///
    /// Returns a message when `chunk_tokens` is zero or `chunk_overlap` is not
    /// smaller than `chunk_tokens`.
    pub fn validate(&self) -> Result<(), String> {
        if self.chunk_tokens == 0 {
            return Err("indexer.chunk_tokens must be greater than 0".to_string());
        }
        if self.chunk_overlap >= self.chunk_tokens {
            return Err("indexer.chunk_overlap must be less than indexer.chunk_tokens".to_string());
        }
        Ok(())
    }

    #[must_use]
    const fn default_chunk_tokens() -> usize {
        crate::search::constants::EMBED_CHUNK_TOKENS_DEFAULT
    }
    #[must_use]
    const fn default_chunk_overlap() -> usize {
        crate::search::constants::EMBED_CHUNK_OVERLAP_DEFAULT
    }
    #[must_use]
    const fn default_chunk_min_tokens() -> usize {
        crate::search::constants::CHUNK_MIN_TOKENS_DEFAULT
    }
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self {
            chunk_tokens: Self::default_chunk_tokens(),
            chunk_overlap: Self::default_chunk_overlap(),
            chunk_min_tokens: Self::default_chunk_min_tokens(),
        }
    }
}
