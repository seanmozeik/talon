//! Deterministic graph ranking over a persisted graph snapshot.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::ScopePriority;
use crate::search::Direction;

use super::GraphSnapshot;
use super::scoring_support::{
    SignalInput, add_source_overlap_candidates, collect_link_candidates, compare_ranked, edge_map,
    score_candidate, undirected_neighbors,
};

const SCORE_EPSILON: f64 = 0.000_001;

/// Input for graph ranking from a source note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRankInput {
    /// Source note path.
    pub source_path: String,
    /// Direction for direct link candidate generation.
    pub direction: Direction,
    /// Link traversal depth used for link-derived candidates.
    pub depth: u8,
    /// Maximum ranked candidates returned.
    pub limit: usize,
    /// Scope priority by resolved scope name.
    pub scope_priorities: BTreeMap<String, ScopePriority>,
}

/// Compact graph relation label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GraphRelation {
    /// Source links to candidate.
    Outgoing,
    /// Candidate links to source.
    Backlink,
    /// Candidate is related indirectly.
    Related,
}

/// Per-signal score details for JSON diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphSignalBreakdown {
    /// Source to candidate link strength.
    pub direct_out: f64,
    /// Candidate to source link strength.
    pub direct_backlink: f64,
    /// IDF-weighted source overlap.
    pub shared_sources: f64,
    /// Adamic-Adar common-neighbor score.
    pub common_neighbors: f64,
    /// Same-community support.
    pub community_affinity: f64,
    /// Same frontmatter `type` support.
    pub type_affinity: f64,
    /// Structural page penalty.
    pub structural_penalty: f64,
}

/// Ranked graph candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphRankedNode {
    /// Candidate note path.
    pub vault_path: String,
    /// Candidate title.
    pub title: String,
    /// Representative direct link text, when available.
    pub link_text: String,
    /// Dominant relation between source and candidate.
    pub relation: GraphRelation,
    /// Direct edge count/strength between source and candidate.
    pub count: u32,
    /// Shortest link-derived hop count.
    pub hops: u8,
    /// Final graph score.
    pub score: f64,
    /// Per-signal breakdown.
    pub signals: GraphSignalBreakdown,
}

/// Ranks graph-related notes from a source path.
#[must_use]
pub fn rank_related(snapshot: &GraphSnapshot, input: &GraphRankInput) -> Vec<GraphRankedNode> {
    let Some(source) = snapshot.nodes.get(&input.source_path) else {
        return Vec::new();
    };

    let outgoing = edge_map(snapshot, true);
    let backlinks = edge_map(snapshot, false);
    let mut candidates = collect_link_candidates(snapshot, input, &outgoing, &backlinks);
    if input.direction == Direction::Both {
        add_source_overlap_candidates(snapshot, source.sources.as_slice(), &mut candidates);
    }

    let source_neighbors = undirected_neighbors(snapshot, &input.source_path);
    let mut ranked = Vec::new();
    for (path, candidate) in candidates {
        let Some(node) = snapshot.nodes.get(&path) else {
            continue;
        };
        if path == input.source_path {
            continue;
        }
        let candidate_neighbors = undirected_neighbors(snapshot, &path);
        let signals = super::scoring_support::build_signals(&SignalInput {
            snapshot,
            source_community: source.community_id,
            candidate_community: node.community_id,
            source_type: source.note_type.as_deref(),
            candidate_type: node.note_type.as_deref(),
            source_sources: source.sources.as_slice(),
            candidate_sources: node.sources.as_slice(),
            source_neighbors: &source_neighbors,
            candidate_neighbors: &candidate_neighbors,
            structural: node.structural,
            community_neighbor_count: node.community_neighbor_count,
            bridge_weight: node.bridge_weight,
            candidate: &candidate,
        });
        let raw_score = score_candidate(&signals, candidate.two_hop);
        let score = input
            .scope_priorities
            .get(&node.scope)
            .map_or(raw_score, |priority| priority.apply_to_score(raw_score));
        if score <= SCORE_EPSILON {
            continue;
        }
        ranked.push(GraphRankedNode {
            vault_path: path,
            title: node.title.clone(),
            link_text: candidate.link_text,
            relation: candidate.relation,
            count: candidate.count,
            hops: candidate.hops,
            score,
            signals,
        });
    }

    ranked.sort_by(compare_ranked);
    ranked.truncate(input.limit);
    ranked
}
