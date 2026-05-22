//! Configuration model for standalone and federated Talon processes.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

mod auth;
mod chunker;
mod defaults;
mod endpoints;
pub mod keychain;
mod scope_filter;
mod search;
#[doc(hidden)]
pub mod test_literals;
use crate::indexer::build_include_globset;

pub use auth::{CredentialEntry, CredentialsConfig, EndpointAuthConfig, ResolvedAuth};
pub use chunker::ChunkerConfig;
pub use endpoints::{
    ChatAdapter, ChatAskConfig, ChatExpansionConfig, ChatSection, EmbeddingAdapter,
    EmbeddingConfig, McpConfig, McpHooksConfig, RerankAdapter, RerankConfig, RerankScoreScale,
};
pub use scope_filter::ScopeFilter;
pub use search::{InspectConfig, SearchConfig};

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
    /// Whether `talon inspect` reports findings for files in this scope.
    ///
    /// Files in `inspect = false` scopes are still indexed and used for link
    /// resolution (so a wikilink target in `daily/` still satisfies a wiki
    /// note's link), but no findings are emitted with `from_path` in this
    /// scope. Defaults to true.
    #[serde(default = "default_true")]
    pub inspect: bool,
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
    /// Named API credentials referenced by capability blocks.
    #[serde(default)]
    pub credentials: CredentialsConfig,
    /// Embedding endpoint configuration.
    pub embedding: EmbeddingConfig,
    /// Rerank endpoint configuration.
    pub rerank: RerankConfig,
    /// Chat endpoints for expansion and ask.
    pub chat: ChatSection,
    /// MCP runtime settings.
    #[serde(default)]
    pub mcp: McpConfig,
    /// Named scopes for vault partitioning and ranking.
    #[serde(default)]
    pub scopes: ScopesConfig,
    /// Search defaults and cache/client tunables.
    #[serde(default)]
    pub search: SearchConfig,
    /// Lint settings (global ignore globs, etc.).
    #[serde(default)]
    pub inspect: InspectConfig,
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

    /// Returns true when `path` should be excluded from `inspect` findings.
    ///
    /// Excludes paths that are either (1) in a scope with `inspect = false`, or
    /// (2) matched by any glob in `[inspect].ignore`. The global ignore list takes
    /// precedence — even paths in `inspect = true` scopes are excluded if they
    /// match an ignore glob. Excluded paths remain in the index and continue
    /// to satisfy link-target resolution.
    #[must_use]
    pub fn inspect_excluded(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        let ignored = self
            .inspect
            .ignore
            .iter()
            .any(|glob| glob_matches_path(glob, path_str.as_ref()));
        if ignored {
            return true;
        }
        for scope in self.scopes.values() {
            if matches_path_glob(path, &scope.glob) {
                return !scope.inspect;
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
    glob.patterns()
        .iter()
        .any(|pattern| glob_matches_path(pattern, path_str.as_ref()))
}

fn glob_matches_path(pattern: &str, path: &str) -> bool {
    build_include_globset(&[pattern.to_string()]).is_ok_and(|set| set.is_match(path))
}
