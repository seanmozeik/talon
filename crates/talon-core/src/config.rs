//! Configuration model for standalone and federated Talon processes.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Priority tier for scope-based ranking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ScopePriority {
    /// Strong promotion (3.0x multiplier).
    Boosted,
    /// Mild promotion (1.5x multiplier).
    Elevated,
    /// Neutral (1.0x multiplier).
    #[default]
    Normal,
    /// Mild demotion (0.3x multiplier).
    Muted,
    /// Strong demotion (0.05x multiplier).
    Buried,
}

impl ScopePriority {
    /// Returns the calibrated post-rerank score multiplier.
    ///
    /// Multipliers are not user-tunable.
    #[must_use]
    pub const fn multiplier(self) -> f64 {
        match self {
            Self::Boosted => 3.0,
            Self::Elevated => 1.5,
            Self::Normal => 1.0,
            Self::Muted => 0.3,
            Self::Buried => 0.05,
        }
    }
}

/// Resolution result for a file-to-scope lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeResolution {
    /// Resolved priority tier.
    pub priority: ScopePriority,
    /// Whether this scope is in the default search set.
    pub default: bool,
}

impl Default for ScopeResolution {
    fn default() -> Self {
        Self {
            priority: ScopePriority::Normal,
            default: true,
        }
    }
}

/// Glob patterns for a scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScopeGlob {
    /// Single glob string.
    Single(String),
    /// Array of glob strings.
    Multiple(Vec<String>),
}

impl ScopeGlob {
    /// Returns all glob patterns for this scope.
    #[must_use]
    pub fn patterns(&self) -> Vec<&str> {
        match self {
            Self::Single(g) => vec![g.as_str()],
            Self::Multiple(g) => g.iter().map(String::as_str).collect(),
        }
    }
}

/// Scope name keyed map.
pub type ScopesConfig = std::collections::BTreeMap<String, Scope>;

/// A single scope definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    /// Glob pattern(s) matching files in this scope.
    pub glob: ScopeGlob,
    /// Priority tier for ranking.
    pub priority: ScopePriority,
    /// Whether this scope is included in the default search set.
    pub default: bool,
}

/// Full Talon runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TalonConfig {
    /// Host or standalone vault path.
    #[serde(alias = "vault_path")]
    pub vault_path: PathBuf,
    /// `SQLite` index path.
    #[serde(alias = "db_path")]
    pub db_path: PathBuf,
    /// Glob-style include patterns.
    #[serde(default, alias = "include_patterns")]
    pub include_patterns: Vec<String>,
    /// Glob-style ignore patterns.
    #[serde(default, alias = "ignore_patterns")]
    pub ignore_patterns: Vec<String>,
    /// Embedding and rerank endpoint configuration.
    pub inference: InferenceConfig,
    /// Query expansion endpoint configuration.
    pub expansion: ExpansionConfig,
    /// Named scopes for vault partitioning and ranking.
    #[serde(default)]
    pub scopes: ScopesConfig,
    /// Chunker settings from the `[indexer]` table.
    #[serde(default, alias = "indexer")]
    pub chunker: ChunkerConfig,
}

impl TalonConfig {
    /// Returns the configured vault path.
    #[must_use]
    pub fn vault_path(&self) -> &Path {
        &self.vault_path
    }

    /// Returns the configured database path.
    #[must_use]
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Returns the resolved scope for a file path.
    ///
    /// Walks scopes in declaration order; first match wins.
    /// Returns the default scope if no scope matches.
    #[must_use]
    pub fn resolve_scope(&self, path: &Path) -> ScopeResolution {
        for scope in self.scopes.values() {
            if matches_path_glob(path, &scope.glob) {
                return ScopeResolution {
                    priority: scope.priority,
                    default: scope.default,
                };
            }
        }
        // Unmatched files fall into synthetic unscoped bucket: normal priority, default true
        ScopeResolution::default()
    }

    /// Returns the set of scope names that are in the default search set.
    #[must_use]
    pub fn default_scope_names(&self) -> Vec<&String> {
        self.scopes
            .iter()
            .filter(|(_, s)| s.default)
            .map(|(n, _)| n)
            .collect()
    }

    /// Returns the scope with the given name, or an error.
    ///
    /// # Errors
    ///
    /// Returns [`TalonError::InvalidScope`] if the scope name is not found.
    pub fn get_scope(&self, name: &str) -> Result<&Scope, crate::error::TalonError> {
        self.scopes
            .get(name)
            .ok_or_else(|| crate::error::TalonError::InvalidScope {
                name: name.to_string(),
            })
    }
}

/// Checks whether a path matches any of the glob patterns in a scope.
fn matches_path_glob(path: &Path, glob: &ScopeGlob) -> bool {
    let path_str = path.to_string_lossy();
    match glob {
        ScopeGlob::Single(g) => glob::Pattern::new(g).is_ok_and(|p| p.matches(&path_str)),
        ScopeGlob::Multiple(globs) => globs
            .iter()
            .any(|g| glob::Pattern::new(g).is_ok_and(|p| p.matches(&path_str))),
    }
}

/// TEI-compatible inference endpoint configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceConfig {
    /// Base URL for TEI-compatible routes.
    #[serde(alias = "base_url")]
    pub base_url: String,
    /// Model names used by the endpoint.
    pub models: InferenceModels,
}

/// Inference model names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceModels {
    /// Query embedding model.
    #[serde(alias = "query_embedding")]
    pub query_embedding: String,
    /// Document embedding model.
    #[serde(alias = "document_embedding")]
    pub document_embedding: String,
    /// Chunk embedding model.
    #[serde(alias = "chunk_embedding")]
    pub chunk_embedding: String,
    /// Reranker model.
    pub reranker: String,
}

/// Chunker knobs for the `[indexer]` section of `talon.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkerConfig {
    /// Target chunk size in tokens (default 512).
    #[serde(
        default = "ChunkerConfig::default_chunk_tokens",
        alias = "chunk_tokens"
    )]
    pub chunk_tokens: usize,
    /// Overlap in tokens between adjacent chunks (default 64, must be < `chunk_tokens`).
    #[serde(
        default = "ChunkerConfig::default_chunk_overlap",
        alias = "chunk_overlap"
    )]
    pub chunk_overlap: usize,
    /// Minimum token count; chunks below this are discarded after splitting (default 16).
    #[serde(
        default = "ChunkerConfig::default_chunk_min_tokens",
        alias = "chunk_min_tokens"
    )]
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

/// OpenAI-compatible query expansion configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpansionConfig {
    /// Provider label, such as `openai-compatible`.
    pub provider: String,
    /// Chat-completions-compatible base URL.
    #[serde(alias = "base_url")]
    pub base_url: String,
    /// Expansion model name.
    pub model: String,
}
