//! Karpathy LLM-Wiki preset scopes shipped with `talon init`.
//!
//! Three knobs per scope (priority/default/inspect). See `examples/config.toml`
//! for the user-facing version of the same preset.

use talon_core::{Scope, ScopeGlob, ScopePriority, ScopesConfig};

/// Builds the Karpathy-shaped preset scopes.
pub(super) fn default_karpathy_scopes() -> ScopesConfig {
    let mut scopes = ScopesConfig::new();
    scopes.insert(
        "wiki".to_string(),
        Scope {
            glob: ScopeGlob::Multiple(vec!["wiki/**".to_string(), "concepts/**".to_string()]),
            priority: ScopePriority::Boosted,
            default: true,
            inspect: true,
        },
    );
    scopes.insert(
        "projects".to_string(),
        Scope {
            glob: ScopeGlob::Single("projects/**".to_string()),
            priority: ScopePriority::Elevated,
            default: true,
            inspect: true,
        },
    );
    scopes.insert(
        "artifacts".to_string(),
        Scope {
            glob: ScopeGlob::Single("artifacts/**".to_string()),
            priority: ScopePriority::Normal,
            default: true,
            inspect: true,
        },
    );
    scopes.insert(
        "raw".to_string(),
        Scope {
            glob: ScopeGlob::Single("raw/**".to_string()),
            priority: ScopePriority::Muted,
            default: true,
            inspect: true,
        },
    );
    scopes.insert(
        "daily".to_string(),
        Scope {
            glob: ScopeGlob::Single("daily/**".to_string()),
            priority: ScopePriority::Muted,
            default: true,
            inspect: false,
        },
    );
    scopes.insert(
        "archive".to_string(),
        Scope {
            glob: ScopeGlob::Single("archive/**".to_string()),
            priority: ScopePriority::Buried,
            default: true,
            inspect: false,
        },
    );
    scopes.insert(
        "private".to_string(),
        Scope {
            glob: ScopeGlob::Single("private/**".to_string()),
            priority: ScopePriority::Normal,
            default: false,
            inspect: false,
        },
    );
    scopes
}
