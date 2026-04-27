//! Shared `--scope` / `--scope-only` / `--scope-all` flags for query commands.

use bpaf::{Parser, long, short};

/// Scope-selection flags parsed from the CLI.
///
/// - `scope` (additive): named scope appended to the default search pool.
/// - `scope_only` (exclusive): search only the named scope(s).
/// - `scope_all`: search every configured scope, overriding `default = false`.
///
/// Mutual exclusivity (`scope` ⊕ `scope_only` ⊕ `scope_all`) is enforced by
/// [`talon_core::ScopeFilter::from_args`] when each command builds its filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopeArgs {
    /// Scope names to add to the default search pool (repeatable).
    pub scope: Vec<String>,
    /// Scope names to search exclusively (repeatable).
    pub scope_only: Vec<String>,
    /// Search every configured scope.
    pub scope_all: bool,
}

/// `bpaf` parser for the shared scope flags.
#[must_use]
pub fn scope_parser() -> impl bpaf::Parser<ScopeArgs> {
    let scope = short('s')
        .long("scope")
        .help(
            "Add a configured scope to the default search pool (repeatable). \
             Required to include scopes with `default = false`.",
        )
        .argument::<String>("NAME")
        .many();
    let scope_only = long("scope-only")
        .help(
            "Search only the named scope (repeatable; mutually exclusive \
             with --scope and --scope-all).",
        )
        .argument::<String>("NAME")
        .many();
    let scope_all = long("scope-all")
        .help("Search every configured scope, overriding `default = false`.")
        .switch();
    bpaf::construct!(ScopeArgs {
        scope,
        scope_only,
        scope_all,
    })
}
