//! Configuration model for standalone and federated Talon processes.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

mod chunker;
mod defaults;
mod endpoints;
mod scope_filter;
use defaults::{
    default_candidate_limit, default_limit, default_rerank_batch_size, default_rerank_cache_size,
    default_rerank_max_tokens, default_search_cache_size,
};

pub use chunker::ChunkerConfig;
pub use endpoints::{
    ExpansionConfig, InferenceConfig, InferenceModels, RerankConfig, RerankRequestShape,
    RerankScoreScale,
};
pub use scope_filter::ScopeFilter;

/// Priority tier for scope-based ranking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ScopePriority {
    /// Compiled knowledge scope.
    Boosted,
    /// Active-work scope.
    Elevated,
    /// Neutral (1.0x multiplier).
    #[default]
    Normal,
    /// Low-priority scope.
    Muted,
    /// Explicit-opt-in scope.
    Buried,
}

impl ScopePriority {
    /// Returns the post-rerank score multiplier.
    #[must_use]
    pub const fn multiplier(self) -> f64 {
        match self {
            Self::Boosted => 1.2,
            Self::Elevated => 1.1,
            Self::Normal => 1.0,
            Self::Muted => 0.85,
            Self::Buried => 0.5,
        }
    }

    /// Applies the multiplier only when it is allowed by the relevance gate.
    ///
    /// Positive priority boosts are gated so a weak match in a high-priority
    /// scope cannot shout over a stronger match elsewhere. Negative weights
    /// still apply below the floor because muted/buried scopes are provenance
    /// signals, not relevance claims.
    #[must_use]
    pub fn apply_to_score(self, score: f64) -> f64 {
        apply_scope_multiplier(score, self.multiplier())
    }

    /// Applies scope priority while honoring an explicit user-selected scope.
    ///
    /// `--scope NAME` is an additive request: default scopes remain in play,
    /// but matches from the requested scope should not be muted below neutral.
    #[must_use]
    pub fn apply_to_score_with_explicit(self, score: f64, explicitly_requested: bool) -> f64 {
        let multiplier = if explicitly_requested {
            self.multiplier().max(Self::Normal.multiplier())
        } else {
            self.multiplier()
        };
        apply_scope_multiplier(score, multiplier)
    }
}

fn apply_scope_multiplier(score: f64, multiplier: f64) -> f64 {
    const POSITIVE_BOOST_RELEVANCE_FLOOR: f64 = 0.4;
    if multiplier > 1.0 && score < POSITIVE_BOOST_RELEVANCE_FLOOR {
        score
    } else {
        score * multiplier
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
///
/// Uses `IndexMap` so iteration follows TOML declaration order — narrower or
/// more sensitive scopes declared above broader ones win when their globs
/// overlap (per spec §6.3).
pub type ScopesConfig = indexmap::IndexMap<String, Scope>;

/// A single scope definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    /// Glob pattern(s) matching files in this scope.
    pub glob: ScopeGlob,
    /// Priority tier for ranking.
    pub priority: ScopePriority,
    /// Whether this scope is included in the default search set.
    pub default: bool,
    /// Whether `talon lint` reports findings for files in this scope.
    ///
    /// Files in `lint = false` scopes are still indexed and used for link
    /// resolution (so a wikilink target in `daily/` still satisfies a wiki
    /// note's link), but no findings are emitted with `from_path` in this
    /// scope. Defaults to true.
    #[serde(default = "default_true")]
    pub lint: bool,
}

const fn default_true() -> bool {
    true
}

/// Full Talon runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TalonConfig {
    /// Host or standalone vault path.
    pub vault_path: PathBuf,
    /// `SQLite` index path.
    pub db_path: PathBuf,
    /// Path to the loaded config file (not serialized; injected at load time).
    #[serde(skip)]
    pub config_file_path: Option<PathBuf>,
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
    /// Search defaults and cache/client tunables.
    #[serde(default)]
    pub search: SearchConfig,
    /// Lint settings (global ignore globs, etc.).
    #[serde(default)]
    pub lint: LintConfig,
    /// Chunker settings from the `[indexer]` table.
    #[serde(default, rename = "indexer")]
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

    /// Returns the name of the scope this path resolves to, or `None` for the
    /// synthetic unscoped bucket.
    #[must_use]
    pub fn resolve_scope_name(&self, path: &Path) -> Option<&str> {
        for (name, scope) in &self.scopes {
            if matches_path_glob(path, &scope.glob) {
                return Some(name.as_str());
            }
        }
        None
    }

    /// Returns true when `path` should be excluded from `lint` findings.
    ///
    /// Excludes paths that are either (1) in a scope with `lint = false`, or
    /// (2) matched by any glob in `[lint].ignore`. The global ignore list takes
    /// precedence — even paths in `lint = true` scopes are excluded if they
    /// match an ignore glob. Excluded paths remain in the index and continue
    /// to satisfy link-target resolution.
    #[must_use]
    pub fn lint_excluded(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        let ignored = self
            .lint
            .ignore
            .iter()
            .any(|glob| glob::Pattern::new(glob).is_ok_and(|p| p.matches(&path_str)));
        if ignored {
            return true;
        }
        for scope in self.scopes.values() {
            if matches_path_glob(path, &scope.glob) {
                return !scope.lint;
            }
        }
        false
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

/// Lint settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintConfig {
    /// Glob-style patterns of file paths to skip when reporting lint findings.
    /// Takes precedence over per-scope `lint = true`. Files matching these
    /// globs are still indexed for link resolution.
    #[serde(default)]
    pub ignore: Vec<String>,
}

/// Search defaults and process-level cache/client tunables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchConfig {
    /// Candidate pool size used when no CLI flag is provided.
    #[serde(default = "default_candidate_limit")]
    pub candidate_limit: u16,
    /// Result limit used when no CLI flag is provided.
    #[serde(default = "default_limit")]
    pub limit: u16,
    /// Search-response LRU capacity.
    #[serde(default = "default_search_cache_size")]
    pub cache_size: usize,
    /// Rerank score LRU capacity.
    #[serde(default = "default_rerank_cache_size")]
    pub rerank_cache_size: usize,
    /// Reranker HTTP request batch size.
    #[serde(default = "default_rerank_batch_size")]
    pub rerank_batch_size: usize,
    /// Approximate reranker text budget.
    #[serde(default = "default_rerank_max_tokens")]
    pub rerank_max_tokens: u32,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            candidate_limit: default_candidate_limit(),
            limit: default_limit(),
            cache_size: default_search_cache_size(),
            rerank_cache_size: default_rerank_cache_size(),
            rerank_batch_size: default_rerank_batch_size(),
            rerank_max_tokens: default_rerank_max_tokens(),
        }
    }
}
