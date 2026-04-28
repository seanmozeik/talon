//! Scope-name-based path filter shared by query commands.
//!
//! Translates the CLI's `--scope` / `--scope-only` / `--scope-all` flags into
//! a path-acceptance predicate that resolves scope names through the
//! configured glob patterns rather than naive prefix matching.

use std::path::Path;

use crate::config::TalonConfig;
use crate::error::TalonError;

/// Filter that accepts vault paths based on the user's scope selection.
#[derive(Debug, Clone)]
pub struct ScopeFilter<'a> {
    config: &'a TalonConfig,
    mode: ScopeFilterMode,
}

/// Internal filter mode.
#[derive(Debug, Clone)]
enum ScopeFilterMode {
    /// Accept scopes with `default = true`, plus any extras the user added.
    Defaults { extras: Vec<String> },
    /// Accept only the named scopes, ignoring `default`.
    Only { names: Vec<String> },
    /// Accept every configured scope.
    All,
}

impl<'a> ScopeFilter<'a> {
    /// Builds a filter from CLI arguments.
    ///
    /// # Errors
    ///
    /// Returns [`TalonError::InvalidInput`] if both `scope` and `scope_only`
    /// are non-empty (they are mutually exclusive on a single invocation), or
    /// if `scope_all` is combined with either of them. Returns
    /// [`TalonError::InvalidScope`] if any name is not declared in config.
    pub fn from_args(
        config: &'a TalonConfig,
        scope: &[String],
        scope_only: &[String],
        scope_all: bool,
    ) -> Result<Self, TalonError> {
        let mut active = 0_u8;
        if !scope.is_empty() {
            active += 1;
        }
        if !scope_only.is_empty() {
            active += 1;
        }
        if scope_all {
            active += 1;
        }
        if active > 1 {
            return Err(TalonError::InvalidInput {
                field: "scope",
                message: "--scope, --scope-only, and --scope-all are mutually exclusive"
                    .to_string(),
            });
        }

        for name in scope.iter().chain(scope_only.iter()) {
            if !config.scopes.contains_key(name) {
                return Err(TalonError::InvalidScope { name: name.clone() });
            }
        }

        let mode = if scope_all {
            ScopeFilterMode::All
        } else if !scope_only.is_empty() {
            ScopeFilterMode::Only {
                names: scope_only.to_vec(),
            }
        } else {
            ScopeFilterMode::Defaults {
                extras: scope.to_vec(),
            }
        };

        Ok(Self { config, mode })
    }

    /// Returns `true` when the filter accepts every note regardless of scope
    /// (i.e. `--scope-all` was passed). Used to skip expensive pre-computation.
    #[must_use]
    pub const fn accepts_all(&self) -> bool {
        matches!(self.mode, ScopeFilterMode::All)
    }

    /// Builds the default filter (no flags passed): default-true scopes only.
    #[must_use]
    pub const fn default_for(config: &'a TalonConfig) -> Self {
        Self {
            config,
            mode: ScopeFilterMode::Defaults { extras: Vec::new() },
        }
    }

    /// Returns true when the path's resolved scope is accepted.
    #[must_use]
    pub fn accepts(&self, path: &str) -> bool {
        let path_buf = Path::new(path);
        let resolved_name = self.config.resolve_scope_name(path_buf);

        match (&self.mode, resolved_name) {
            (ScopeFilterMode::All, _) | (ScopeFilterMode::Defaults { .. }, None) => true,
            (ScopeFilterMode::Only { names }, Some(name)) => names.iter().any(|n| n == name),
            (ScopeFilterMode::Only { .. }, None) => false,
            (ScopeFilterMode::Defaults { extras }, Some(name)) => {
                let scope = self.config.scopes.get(name);
                scope.is_some_and(|s| s.default) || extras.iter().any(|n| n == name)
            }
        }
    }

    /// Returns the resolved active scope name set, suitable for `meta.scope_set`.
    #[must_use]
    pub fn resolved_set(&self) -> Vec<String> {
        match &self.mode {
            ScopeFilterMode::All => self.config.scopes.keys().cloned().collect(),
            ScopeFilterMode::Only { names } => names.clone(),
            ScopeFilterMode::Defaults { extras } => {
                let mut set: Vec<String> = self
                    .config
                    .default_scope_names()
                    .into_iter()
                    .cloned()
                    .collect();
                for extra in extras {
                    if !set.contains(extra) {
                        set.push(extra.clone());
                    }
                }
                set
            }
        }
    }
}

#[cfg(test)]
#[path = "scope_filter_tests.rs"]
#[allow(clippy::expect_used)]
mod tests;
