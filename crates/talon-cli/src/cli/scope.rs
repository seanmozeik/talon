//! Shared `--scope` / `--scope-only` / `--scope-all` flags for query commands.

use clap::Args;

/// Scope-selection flags shared across query commands.
///
/// - `scope` (additive): named scope appended to the default search pool.
/// - `scope_only` (exclusive): search only the named scope(s).
/// - `scope_all`: search every configured scope, overriding `default = false`.
///
/// Mutual exclusivity (`scope` ⊕ `scope_only` ⊕ `scope_all`) is enforced by
/// [`talon_core::ScopeFilter::from_args`] when each command builds its filter.
#[derive(Debug, Clone, Default, Args)]
#[command(next_help_heading = "SCOPE")]
pub struct SharedScopeArgs {
    /// Add a configured scope to the default search pool (repeatable).
    /// Required to include scopes with `default = false`.
    #[arg(short, long)]
    pub scope: Vec<String>,

    /// Search only the named scope (repeatable; mutually exclusive
    /// with --scope and --scope-all).
    #[arg(long)]
    pub scope_only: Vec<String>,

    /// Search every configured scope, overriding `default = false`.
    #[arg(long)]
    pub scope_all: bool,
}
