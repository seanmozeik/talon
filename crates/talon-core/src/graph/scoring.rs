//! Deterministic graph ranking over a persisted graph snapshot.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::search::Direction;

use super::GraphSnapshot;

const DIRECT_OUT_WEIGHT: f64 = 4.0;
const DIRECT_BACKLINK_WEIGHT: f64 = 3.5;
const SOURCE_OVERLAP_WEIGHT: f64 = 2.0;
const COMMON_NEIGHBOR_WEIGHT: f64 = 1.4;
const TWO_HOP_WEIGHT: f64 = 0.35;
const STRUCTURAL_PENALTY: f64 = 1.5;
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
        let signals = build_signals(
            snapshot,
            source.sources.as_slice(),
            node.sources.as_slice(),
            &source_neighbors,
            &candidate_neighbors,
            node.structural,
            &candidate,
        );
        let score = f64::from(candidate.two_hop).mul_add(TWO_HOP_WEIGHT, score_signals(&signals));
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

#[derive(Debug, Clone)]
struct Candidate {
    relation: GraphRelation,
    link_text: String,
    count: u32,
    hops: u8,
    two_hop: u32,
}

fn collect_link_candidates(
    snapshot: &GraphSnapshot,
    input: &GraphRankInput,
    outgoing: &BTreeMap<String, Vec<&super::GraphEdge>>,
    backlinks: &BTreeMap<String, Vec<&super::GraphEdge>>,
) -> BTreeMap<String, Candidate> {
    let depth = input.depth.clamp(1, crate::constants::RELATED_MAX_DEPTH);
    let mut queue = VecDeque::from([(input.source_path.clone(), 0_u8)]);
    let mut visited = BTreeSet::from([input.source_path.clone()]);
    let mut candidates = BTreeMap::new();

    while let Some((path, hops)) = queue.pop_front() {
        if hops >= depth {
            continue;
        }
        for edge in directed_edges(input.direction, &path, outgoing, backlinks) {
            let (next, relation) = if edge.from_path == path {
                (edge.to_path.clone(), GraphRelation::Outgoing)
            } else {
                (edge.from_path.clone(), GraphRelation::Backlink)
            };
            if !snapshot.nodes.contains_key(&next) {
                continue;
            }
            let next_hops = hops.saturating_add(1);
            candidates
                .entry(next.clone())
                .and_modify(|candidate: &mut Candidate| {
                    candidate.count = candidate.count.saturating_add(edge.weight);
                    candidate.hops = candidate.hops.min(next_hops);
                    if next_hops > 1 {
                        candidate.two_hop = candidate.two_hop.saturating_add(1);
                    }
                })
                .or_insert_with(|| Candidate {
                    relation: if next_hops == 1 {
                        relation
                    } else {
                        GraphRelation::Related
                    },
                    link_text: edge.link_text.clone(),
                    count: edge.weight,
                    hops: next_hops,
                    two_hop: u32::from(next_hops > 1),
                });
            if visited.insert(next.clone()) {
                queue.push_back((next, next_hops));
            }
        }
    }
    candidates
}

fn add_source_overlap_candidates(
    snapshot: &GraphSnapshot,
    source_sources: &[String],
    candidates: &mut BTreeMap<String, Candidate>,
) {
    for source in source_sources {
        let Some(paths) = snapshot.source_citations.get(source) else {
            continue;
        };
        for path in paths {
            candidates.entry(path.clone()).or_insert_with(|| Candidate {
                relation: GraphRelation::Related,
                link_text: String::new(),
                count: 0,
                hops: 2,
                two_hop: 0,
            });
        }
    }
}

fn build_signals(
    snapshot: &GraphSnapshot,
    source_sources: &[String],
    candidate_sources: &[String],
    source_neighbors: &BTreeSet<String>,
    candidate_neighbors: &BTreeSet<String>,
    structural: bool,
    candidate: &Candidate,
) -> GraphSignalBreakdown {
    let shared_sources = source_sources
        .iter()
        .filter(|source| candidate_sources.contains(source))
        .map(|source| {
            let citing_count = snapshot
                .source_citations
                .get(source)
                .map_or(1_u32, |paths| paths.len().try_into().unwrap_or(u32::MAX));
            1.0 / (2.0 + f64::from(citing_count)).ln()
        })
        .sum::<f64>()
        .min(3.0);
    let common_neighbors = source_neighbors
        .intersection(candidate_neighbors)
        .map(|neighbor| {
            let degree: u32 = undirected_neighbors(snapshot, neighbor)
                .len()
                .max(2)
                .try_into()
                .unwrap_or(u32::MAX);
            1.0 / f64::from(degree).ln()
        })
        .sum::<f64>()
        .min(3.0);
    GraphSignalBreakdown {
        direct_out: if candidate.relation == GraphRelation::Outgoing && candidate.hops == 1 {
            f64::from(candidate.count).min(4.0)
        } else {
            0.0
        },
        direct_backlink: if candidate.relation == GraphRelation::Backlink && candidate.hops == 1 {
            f64::from(candidate.count).min(4.0)
        } else {
            0.0
        },
        shared_sources,
        common_neighbors,
        community_affinity: 0.0,
        structural_penalty: if structural { STRUCTURAL_PENALTY } else { 0.0 },
    }
}

fn score_signals(signals: &GraphSignalBreakdown) -> f64 {
    DIRECT_OUT_WEIGHT.mul_add(
        signals.direct_out,
        DIRECT_BACKLINK_WEIGHT.mul_add(
            signals.direct_backlink,
            SOURCE_OVERLAP_WEIGHT.mul_add(
                signals.shared_sources,
                COMMON_NEIGHBOR_WEIGHT.mul_add(
                    signals.common_neighbors,
                    signals.community_affinity - signals.structural_penalty,
                ),
            ),
        ),
    )
}

fn directed_edges<'a>(
    direction: Direction,
    path: &str,
    outgoing: &'a BTreeMap<String, Vec<&'a super::GraphEdge>>,
    backlinks: &'a BTreeMap<String, Vec<&'a super::GraphEdge>>,
) -> Vec<&'a super::GraphEdge> {
    let mut edges = Vec::new();
    if matches!(direction, Direction::Outgoing | Direction::Both)
        && let Some(found) = outgoing.get(path)
    {
        edges.extend(found.iter().copied());
    }
    if matches!(direction, Direction::Backlinks | Direction::Both)
        && let Some(found) = backlinks.get(path)
    {
        edges.extend(found.iter().copied());
    }
    edges
}

fn edge_map(snapshot: &GraphSnapshot, by_from: bool) -> BTreeMap<String, Vec<&super::GraphEdge>> {
    let mut map: BTreeMap<String, Vec<&super::GraphEdge>> = BTreeMap::new();
    for edge in &snapshot.edges {
        let key = if by_from {
            &edge.from_path
        } else {
            &edge.to_path
        };
        map.entry(key.clone()).or_default().push(edge);
    }
    map
}

fn undirected_neighbors(snapshot: &GraphSnapshot, path: &str) -> BTreeSet<String> {
    let mut neighbors = BTreeSet::new();
    for edge in &snapshot.edges {
        if edge.from_path == path {
            neighbors.insert(edge.to_path.clone());
        } else if edge.to_path == path {
            neighbors.insert(edge.from_path.clone());
        }
    }
    neighbors
}

fn compare_ranked(left: &GraphRankedNode, right: &GraphRankedNode) -> std::cmp::Ordering {
    right
        .score
        .partial_cmp(&left.score)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| relation_rank(left.relation).cmp(&relation_rank(right.relation)))
        .then_with(|| left.hops.cmp(&right.hops))
        .then_with(|| left.vault_path.cmp(&right.vault_path))
}

const fn relation_rank(relation: GraphRelation) -> u8 {
    match relation {
        GraphRelation::Outgoing => 0,
        GraphRelation::Backlink => 1,
        GraphRelation::Related => 2,
    }
}
