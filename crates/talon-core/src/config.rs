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
    pub vault_path: PathBuf,
    /// `SQLite` index path.
    pub db_path: PathBuf,
    /// Glob-style include patterns.
    #[serde(default)]
    pub include_patterns: Vec<String>,
    /// Glob-style ignore patterns.
    #[serde(default)]
    pub ignore_patterns: Vec<String>,
    /// Embedding and rerank endpoint configuration.
    pub inference: InferenceConfig,
    /// Query expansion endpoint configuration.
    pub expansion: ExpansionConfig,
    /// Named scopes for vault partitioning and ranking.
    #[serde(default)]
    pub scopes: ScopesConfig,
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
    pub base_url: String,
    /// Model names used by the endpoint.
    pub models: InferenceModels,
}

/// Inference model names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceModels {
    /// Query embedding model.
    pub query_embedding: String,
    /// Document embedding model.
    pub document_embedding: String,
    /// Chunk embedding model.
    pub chunk_embedding: String,
    /// Reranker model.
    pub reranker: String,
}

/// OpenAI-compatible query expansion configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpansionConfig {
    /// Provider label, such as `openai-compatible`.
    pub provider: String,
    /// Chat-completions-compatible base URL.
    pub base_url: String,
    /// Expansion model name.
    pub model: String,
}
