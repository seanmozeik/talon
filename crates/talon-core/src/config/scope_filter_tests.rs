use std::path::PathBuf;

use super::ScopeFilter;
use crate::config::{
    ChunkerConfig, ExpansionConfig, InferenceConfig, InferenceModels, InspectConfig, RerankConfig,
    Scope, ScopeGlob, ScopePriority, ScopesConfig, SearchConfig, TalonConfig,
};
use crate::error::TalonError;

fn scope(glob: &str, priority: ScopePriority, default: bool) -> Scope {
    Scope {
        glob: ScopeGlob::Single(glob.to_string()),
        priority,
        default,
        inspect: true,
    }
}

fn config_with(scopes: Vec<(&str, Scope)>) -> TalonConfig {
    let mut map = ScopesConfig::new();
    for (name, s) in scopes {
        map.insert(name.to_string(), s);
    }
    TalonConfig {
        vault_path: PathBuf::from("/vault"),
        db_path: PathBuf::from("/vault/.talon/index.db"),
        config_file_path: None,
        include_patterns: Vec::new(),
        ignore_patterns: Vec::new(),
        inference: InferenceConfig {
            base_url: "http://localhost:8080".to_string(),
            models: InferenceModels {
                query_embedding: "embed-q".to_string(),
                query_embedding_context_tokens: 512,
                document_embedding: "embed-d".to_string(),
                chunk_embedding: "embed-c".to_string(),
                reranker: "rerank".to_string(),
                reranker_context_tokens: 512,
            },
            rerank: RerankConfig::default(),
        },
        expansion: ExpansionConfig {
            provider: "openai-compatible".to_string(),
            base_url: "http://localhost:8080".to_string(),
            model: "expand".to_string(),
            context_tokens: 32768,
            max_output_tokens: Some(256),
        },
        ask: crate::config::AskConfig::default(),
        mcp: crate::config::McpConfig::default(),
        scopes: map,
        search: SearchConfig::default(),
        inspect: crate::config::InspectConfig::default(),
        chunker: ChunkerConfig::default(),
    }
}

fn karpathy_config() -> TalonConfig {
    config_with(vec![
        ("wiki", scope("wiki/**", ScopePriority::Boosted, true)),
        (
            "projects",
            scope("projects/**", ScopePriority::Elevated, true),
        ),
        ("daily", scope("daily/**", ScopePriority::Muted, false)),
        ("private", scope("private/**", ScopePriority::Buried, false)),
    ])
}

#[test]
fn default_filter_includes_only_default_true_scopes() {
    let cfg = karpathy_config();
    let filter = ScopeFilter::default_for(&cfg);
    assert!(filter.accepts("wiki/Sauce Mothers.md"));
    assert!(filter.accepts("projects/Spring Menu.md"));
    assert!(!filter.accepts("daily/2026-04-25.md"));
    assert!(!filter.accepts("private/Lease.md"));
}

#[test]
fn unscoped_paths_pass_default_filter() {
    let cfg = karpathy_config();
    let filter = ScopeFilter::default_for(&cfg);
    assert!(filter.accepts("README.md"));
    assert!(filter.accepts("notebooks/a.md"));
}

#[test]
fn scope_additive_includes_extra_scope() {
    let cfg = karpathy_config();
    let filter =
        ScopeFilter::from_args(&cfg, &["private".to_string()], &[], false).expect("build filter");
    assert!(filter.accepts("wiki/x.md"));
    assert!(filter.accepts("private/Lease.md"));
    assert!(!filter.accepts("daily/2026-04-25.md"));
}

#[test]
fn scope_only_replaces_pool() {
    let cfg = karpathy_config();
    let filter =
        ScopeFilter::from_args(&cfg, &[], &["wiki".to_string()], false).expect("build filter");
    assert!(filter.accepts("wiki/x.md"));
    assert!(!filter.accepts("projects/y.md"));
    assert!(!filter.accepts("private/z.md"));
    assert!(!filter.accepts("README.md"));
}

#[test]
fn scope_all_accepts_every_path() {
    let cfg = karpathy_config();
    let filter = ScopeFilter::from_args(&cfg, &[], &[], true).expect("build filter");
    assert!(filter.accepts("wiki/x.md"));
    assert!(filter.accepts("private/Lease.md"));
    assert!(filter.accepts("daily/2026-04-25.md"));
    assert!(filter.accepts("README.md"));
}

#[test]
fn inspect_excluded_honors_scope_lint_and_global_ignore() {
    let mut daily = scope("daily/**", ScopePriority::Muted, true);
    daily.inspect = false;
    let mut cfg = config_with(vec![
        ("daily", daily),
        ("wiki", scope("wiki/**", ScopePriority::Boosted, true)),
    ]);
    cfg.inspect = InspectConfig {
        ignore: vec!["README.md".to_string()],
    };

    assert!(cfg.inspect_excluded(std::path::Path::new("daily/2026-04-29.md")));
    assert!(cfg.inspect_excluded(std::path::Path::new("README.md")));
    assert!(!cfg.inspect_excluded(std::path::Path::new("wiki/Sauce.md")));
}

#[test]
fn scope_and_scope_only_are_mutually_exclusive() {
    let cfg = karpathy_config();
    let err = ScopeFilter::from_args(
        &cfg,
        &["wiki".to_string()],
        &["projects".to_string()],
        false,
    )
    .expect_err("should reject combined flags");
    assert!(matches!(err, TalonError::InvalidInput { .. }));
}

#[test]
fn scope_all_combined_with_scope_only_errors() {
    let cfg = karpathy_config();
    let err = ScopeFilter::from_args(&cfg, &[], &["wiki".to_string()], true)
        .expect_err("should reject combined flags");
    assert!(matches!(err, TalonError::InvalidInput { .. }));
}

#[test]
fn unknown_scope_name_errors() {
    let cfg = karpathy_config();
    let err = ScopeFilter::from_args(&cfg, &["typo".to_string()], &[], false)
        .expect_err("should reject unknown name");
    match err {
        TalonError::InvalidScope { name } => assert_eq!(name, "typo"),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn resolved_set_for_default_includes_all_default_scopes() {
    let cfg = karpathy_config();
    let filter = ScopeFilter::default_for(&cfg);
    let mut names = filter.resolved_set();
    names.sort();
    assert_eq!(names, vec!["projects".to_string(), "wiki".to_string()]);
}

#[test]
fn resolved_set_for_scope_appends_extras() {
    let cfg = karpathy_config();
    let filter =
        ScopeFilter::from_args(&cfg, &["private".to_string()], &[], false).expect("build filter");
    let mut names = filter.resolved_set();
    names.sort();
    assert_eq!(
        names,
        vec![
            "private".to_string(),
            "projects".to_string(),
            "wiki".to_string()
        ]
    );
}

#[test]
fn resolved_set_for_scope_only_returns_named_set() {
    let cfg = karpathy_config();
    let filter =
        ScopeFilter::from_args(&cfg, &[], &["wiki".to_string()], false).expect("build filter");
    assert_eq!(filter.resolved_set(), vec!["wiki".to_string()]);
}

#[test]
fn resolved_set_for_scope_all_returns_every_scope() {
    let cfg = karpathy_config();
    let filter = ScopeFilter::from_args(&cfg, &[], &[], true).expect("build filter");
    let mut names = filter.resolved_set();
    names.sort();
    assert_eq!(
        names,
        vec![
            "daily".to_string(),
            "private".to_string(),
            "projects".to_string(),
            "wiki".to_string()
        ]
    );
}
