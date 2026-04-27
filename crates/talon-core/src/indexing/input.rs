//! Indexing tool input types.

use serde::{Deserialize, Serialize};

/// Lint check type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintCheck {
    /// Run every lint check.
    All,
    /// Files with no incoming wikilinks.
    Orphans,
    /// Links whose targets don't resolve to indexed files.
    BrokenLinks,
    /// Frontmatter `sources:` pointing to non-existent paths.
    DanglingRefs,
    /// Files with no incoming AND no outgoing wikilinks.
    Unreferenced,
}

/// Sync request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncInput {
    /// Specific paths to sync (empty = full pass).
    #[serde(default)]
    pub paths: Vec<String>,
    /// Skip embeddings (lexical-only pass).
    #[serde(default)]
    pub fast: bool,
    /// Reset vector state before syncing.
    #[serde(default)]
    pub force: bool,
    /// Return immediately if sync is already running.
    #[serde(default)]
    pub no_wait: bool,
}

/// Status request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StatusInput {
    /// Emit JSON output.
    #[serde(default)]
    pub json: bool,
}

/// Lint check request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LintInput {
    /// Which lint check to run.
    pub check: LintCheck,
    /// Scope names to include.
    #[serde(default)]
    pub scope: Vec<String>,
    /// Scope names to search exclusively.
    #[serde(default)]
    pub scope_only: Vec<String>,
    /// Include every configured scope, overriding `default = false`.
    #[serde(default)]
    pub scope_all: bool,
}
