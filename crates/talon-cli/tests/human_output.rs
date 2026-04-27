//! Snapshot tests for human-readable CLI output.
//!
//! Colors are disabled (`RenderOptions { colors: false }`) so snapshots contain
//! no ANSI escape codes and can be reviewed as plain text.

use color_eyre::eyre::Result;
use talon_cli::output::{
    RenderOptions, format_lint_human, format_search_human, format_status_human, format_sync_human,
};
use talon_core::{
    ContainerPath, IndexStats, LintCheck, LintFinding, LintResponse, MatchKind, SearchMode,
    SearchResponse, SearchResult, StatusResponse, StatusState, SyncResponse, VaultPath,
};

const fn opts() -> RenderOptions {
    RenderOptions {
        width: 80,
        colors: false,
    }
}

fn make_vault_path(s: &str) -> Result<VaultPath> {
    Ok(VaultPath::parse(s)?)
}

fn make_container_path(s: &str) -> Result<ContainerPath> {
    Ok(ContainerPath::parse(s)?)
}

// ── search ────────────────────────────────────────────────────────────────────

#[test]
fn snapshot_search_empty() -> Result<()> {
    let resp = SearchResponse {
        query: Some("orchard".to_string()),
        mode: SearchMode::Hybrid,
        fast: false,
        expanded: false,
        reranked: false,
        index_version: "1".to_string(),
        total: 0,
        results: vec![],
    };
    let mut buf = Vec::new();
    format_search_human(&mut buf, &resp, opts())?;
    insta::assert_snapshot!(String::from_utf8(buf)?);
    Ok(())
}

#[test]
fn snapshot_search_results() -> Result<()> {
    let resp = SearchResponse {
        query: Some("galaxy brain".to_string()),
        mode: SearchMode::Hybrid,
        fast: false,
        expanded: true,
        reranked: true,
        index_version: "1".to_string(),
        total: 2,
        results: vec![
            SearchResult {
                vault_path: make_vault_path("Atlas/Overview.md")?,
                path: make_container_path("/vault/Atlas/Overview.md")?,
                title: "Atlas Overview".to_string(),
                snippet: "This is a snippet about galaxy brains and neural pathways that may wrap."
                    .to_string(),
                score: 0.847,
                raw_score: None,
                match_kind: MatchKind::Fulltext,
                scope: None,
                preview_anchors: None,
            },
            SearchResult {
                vault_path: make_vault_path("Graph/Hub.md")?,
                path: make_container_path("/vault/Graph/Hub.md")?,
                title: "Hub".to_string(),
                snippet: "Semantic match snippet here.".to_string(),
                score: 0.723,
                raw_score: None,
                match_kind: MatchKind::Semantic,
                scope: Some("research".to_string()),
                preview_anchors: None,
            },
        ],
    };
    let mut buf = Vec::new();
    format_search_human(&mut buf, &resp, opts())?;
    insta::assert_snapshot!(String::from_utf8(buf)?);
    Ok(())
}

// ── sync ──────────────────────────────────────────────────────────────────────

#[test]
fn snapshot_sync_fast() -> Result<()> {
    let resp = SyncResponse {
        completed: true,
        status: talon_core::SyncStatus::Ok,
        fast: true,
        force: false,
        path_count: 21,
        indexed: 5,
        skipped: 16,
        deleted: 0,
        embedded: 0,
        embed_failed: 0,
        dimension_mismatch: false,
        embed_remediation: None,
        embed_diagnostics: vec![],
        duration_ms: 42,
    };
    let mut buf = Vec::new();
    format_sync_human(&mut buf, &resp)?;
    insta::assert_snapshot!(String::from_utf8(buf)?);
    Ok(())
}

#[test]
fn snapshot_sync_with_embed() -> Result<()> {
    let resp = SyncResponse {
        completed: true,
        status: talon_core::SyncStatus::Ok,
        fast: false,
        force: false,
        path_count: 21,
        indexed: 5,
        skipped: 16,
        deleted: 1,
        embedded: 4,
        embed_failed: 1,
        dimension_mismatch: false,
        embed_remediation: Some("Re-run talon sync --force to reset vectors.".to_string()),
        embed_diagnostics: vec!["failed: Atlas/Overview.md".to_string()],
        duration_ms: 1234,
    };
    let mut buf = Vec::new();
    format_sync_human(&mut buf, &resp)?;
    insta::assert_snapshot!(String::from_utf8(buf)?);
    Ok(())
}

// ── status ────────────────────────────────────────────────────────────────────

#[test]
fn snapshot_status_ready() -> Result<()> {
    let resp = StatusResponse {
        state: StatusState::Ready,
        enabled: true,
        reason: None,
        container_mount: make_container_path("/vault")?,
        index_version: "1".to_string(),
        index: IndexStats {
            active_notes: 21,
            chunk_count: 63,
            failed_embeddings: 0,
            vector_dimensions: Some(384),
        },
        scopes: None,
    };
    let mut buf = Vec::new();
    format_status_human(&mut buf, &resp)?;
    insta::assert_snapshot!(String::from_utf8(buf)?);
    Ok(())
}

// ── lint ──────────────────────────────────────────────────────────────────────

#[test]
fn snapshot_lint_orphans() -> Result<()> {
    let resp = LintResponse {
        check: LintCheck::Orphans,
        findings: vec![
            LintFinding {
                check: LintCheck::Orphans,
                path: make_vault_path("Graph/Orphan.md")?,
                line: None,
                message: "no incoming links".to_string(),
            },
            LintFinding {
                check: LintCheck::Orphans,
                path: make_vault_path("Lifecycle/Doomed.md")?,
                line: None,
                message: "no incoming links".to_string(),
            },
        ],
    };
    let mut buf = Vec::new();
    format_lint_human(&mut buf, &resp)?;
    insta::assert_snapshot!(String::from_utf8(buf)?);
    Ok(())
}
