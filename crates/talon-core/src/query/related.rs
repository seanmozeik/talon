//! Related-notes handler for the Talon CLI.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::config::{ScopeFilter, TalonConfig};
use crate::constants::RELATED_MAX_DEPTH;
use crate::contracts::{ContainerPath, VaultPath};
use crate::graph::{
    GraphRankInput, GraphRankedNode, GraphRelation, GraphSignalBreakdown, load_graph_snapshot,
    rank_related,
};
use crate::search::Direction;

mod legacy;

/// Related-note request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedInput {
    /// Path to find related notes for.
    pub path: String,
    /// Graph traversal depth.
    #[serde(default = "default_depth")]
    pub depth: u8,
    /// Traversal direction.
    #[serde(default)]
    pub direction: Direction,
    /// Scope names to include.
    #[serde(default)]
    pub scope: Vec<String>,
    /// Scope names to search exclusively.
    #[serde(default)]
    pub scope_only: Vec<String>,
    /// Include every configured scope, overriding `default = false`.
    #[serde(default)]
    pub scope_all: bool,
    /// Maximum ranked results returned by MCP callers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// Related-note response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedResponse {
    /// Vault root (absolute container path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault: Option<ContainerPath>,
    /// Source path.
    pub path: VaultPath,
    /// Direction traversed.
    pub direction: Direction,
    /// Related notes.
    pub results: Vec<RelatedResult>,
}

/// A single related-note result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedResult {
    /// Vault-relative path.
    pub vault_path: VaultPath,
    /// Display title.
    pub title: String,
    /// Link text from source.
    pub link_text: String,
    /// Direction: outgoing or backlink.
    pub relation: RelationKind,
    /// Number of distinct link rows connecting source and target. A note
    /// linked once and a note linked from three different aliases score
    /// 1 vs 3 — a rough proxy for edge strength.
    pub count: u32,
    /// Graph relevance score.
    pub score: f64,
    /// Per-signal graph score breakdown.
    pub signals: GraphSignalBreakdown,
    /// Resolved scope name, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// File modification time as RFC 3339 / ISO 8601.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime: Option<String>,
}

/// Relation kind (outgoing vs backlink).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelationKind {
    Outgoing,
    Backlink,
}

const fn default_depth() -> u8 {
    crate::constants::RELATED_DEFAULT_DEPTH
}

/// Traverses the link graph from `input.path` and returns related notes.
///
/// - Depth is clamped to [`RELATED_MAX_DEPTH`] and minimum 1.
/// - Cycles are detected via a visited set — each path appears at most once.
/// - `scope_only` filters results to notes whose vault path starts with any
///   listed prefix. `scope` and `scope_only` both empty means no filtering.
pub fn find_related(
    conn: &Connection,
    input: &RelatedInput,
    config: Option<&TalonConfig>,
) -> RelatedResponse {
    let path = input.path.trim();

    let Ok(source_path) = VaultPath::parse(path) else {
        return RelatedResponse {
            vault: None,
            path: VaultPath::parse("_").unwrap_or_else(|_| unreachable!()),
            direction: input.direction,
            results: Vec::new(),
        };
    };

    let depth = input.depth.clamp(1, RELATED_MAX_DEPTH);
    let direction = input.direction;

    let filter = config.map(|cfg| {
        ScopeFilter::from_args(cfg, &input.scope, &input.scope_only, input.scope_all)
            .unwrap_or_else(|_| ScopeFilter::default_for(cfg))
    });

    if let Ok(snapshot) = load_graph_snapshot(conn)
        && snapshot.nodes.contains_key(path)
        && !snapshot.edges.is_empty()
    {
        return graph_ranked_response(
            conn,
            input,
            source_path,
            depth,
            config,
            filter.as_ref(),
            &snapshot,
        );
    }

    RelatedResponse {
        vault: None,
        path: source_path,
        direction,
        results: legacy::legacy_related_results(
            conn,
            path,
            depth,
            direction,
            filter.as_ref(),
            config,
        ),
    }
}

fn graph_ranked_response(
    conn: &Connection,
    input: &RelatedInput,
    source_path: VaultPath,
    depth: u8,
    config: Option<&TalonConfig>,
    filter: Option<&ScopeFilter<'_>>,
    snapshot: &crate::graph::GraphSnapshot,
) -> RelatedResponse {
    let ranked = rank_related(
        snapshot,
        &GraphRankInput {
            source_path: input.path.trim().to_string(),
            direction: input.direction,
            depth,
            limit: input.limit.unwrap_or(usize::MAX),
            scope_priorities: config
                .map(|cfg| {
                    cfg.scopes
                        .iter()
                        .map(|(name, scope)| (name.clone(), scope.priority))
                        .collect()
                })
                .unwrap_or_default(),
        },
    );
    RelatedResponse {
        vault: None,
        path: source_path,
        direction: input.direction,
        results: ranked_to_results(conn, ranked, config, filter),
    }
}

fn ranked_to_results(
    conn: &Connection,
    ranked: Vec<GraphRankedNode>,
    config: Option<&TalonConfig>,
    filter: Option<&ScopeFilter<'_>>,
) -> Vec<RelatedResult> {
    ranked
        .into_iter()
        .filter(|node| filter.is_none_or(|f| f.accepts(&node.vault_path)))
        .filter_map(|node| {
            let vault_path = VaultPath::parse(&node.vault_path).ok()?;
            let scope = config
                .and_then(|cfg| cfg.resolve_scope_name(std::path::Path::new(&node.vault_path)))
                .map(str::to_string);
            let mtime = super::mtime::local_mtime_for_path(conn, &node.vault_path);
            Some(RelatedResult {
                vault_path,
                title: node.title,
                link_text: node.link_text,
                relation: match node.relation {
                    GraphRelation::Outgoing | GraphRelation::Related => RelationKind::Outgoing,
                    GraphRelation::Backlink => RelationKind::Backlink,
                },
                count: node.count,
                score: node.score,
                signals: node.signals,
                scope,
                mtime,
            })
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
