//! Private candidate generation and signal math for graph ranking.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::search::Direction;

use super::scoring::{GraphRankInput, GraphRankedNode, GraphRelation, GraphSignalBreakdown};
use super::{GraphEdge, GraphSnapshot};

const DIRECT_OUT_WEIGHT: f64 = 4.0;
const DIRECT_BACKLINK_WEIGHT: f64 = 3.5;
const SOURCE_OVERLAP_WEIGHT: f64 = 2.0;
const COMMON_NEIGHBOR_WEIGHT: f64 = 1.4;
const COMMUNITY_WEIGHT: f64 = 0.9;
const TYPE_AFFINITY_WEIGHT: f64 = 0.7;
const TWO_HOP_WEIGHT: f64 = 0.35;
const STRUCTURAL_PENALTY: f64 = 1.5;
const BRIDGE_BONUS: f64 = 0.6;

#[derive(Debug, Clone)]
pub(super) struct Candidate {
    pub(super) relation: GraphRelation,
    pub(super) link_text: String,
    pub(super) count: u32,
    pub(super) hops: u8,
    pub(super) two_hop: u32,
}

pub(super) fn collect_link_candidates(
    snapshot: &GraphSnapshot,
    input: &GraphRankInput,
    outgoing: &BTreeMap<String, Vec<&GraphEdge>>,
    backlinks: &BTreeMap<String, Vec<&GraphEdge>>,
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
                    update_candidate(candidate, edge, next_hops);
                })
                .or_insert_with(|| new_candidate(edge, relation, next_hops));
            if visited.insert(next.clone()) {
                queue.push_back((next, next_hops));
            }
        }
    }
    candidates
}

pub(super) fn add_source_overlap_candidates(
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

pub(super) struct SignalInput<'a> {
    pub(super) snapshot: &'a GraphSnapshot,
    pub(super) source_community: Option<u32>,
    pub(super) candidate_community: Option<u32>,
    pub(super) source_type: Option<&'a str>,
    pub(super) candidate_type: Option<&'a str>,
    pub(super) source_sources: &'a [String],
    pub(super) candidate_sources: &'a [String],
    pub(super) source_neighbors: &'a BTreeSet<String>,
    pub(super) candidate_neighbors: &'a BTreeSet<String>,
    pub(super) structural: bool,
    pub(super) community_neighbor_count: u32,
    pub(super) bridge_weight: f64,
    pub(super) candidate: &'a Candidate,
}

pub(super) fn build_signals(input: &SignalInput<'_>) -> GraphSignalBreakdown {
    GraphSignalBreakdown {
        direct_out: direct_out(input.candidate),
        direct_backlink: direct_backlink(input.candidate),
        shared_sources: shared_sources(
            input.snapshot,
            input.source_sources,
            input.candidate_sources,
        ),
        common_neighbors: common_neighbors(
            input.snapshot,
            input.source_neighbors,
            input.candidate_neighbors,
        ),
        community_affinity: community_affinity(
            input.source_community,
            input.candidate_community,
            input.community_neighbor_count,
            input.bridge_weight,
        ),
        type_affinity: type_affinity(input.source_type, input.candidate_type),
        structural_penalty: if input.structural {
            STRUCTURAL_PENALTY
        } else {
            0.0
        },
    }
}

pub(super) fn score_candidate(signals: &GraphSignalBreakdown, two_hop: u32) -> f64 {
    f64::from(two_hop).mul_add(TWO_HOP_WEIGHT, score_signals(signals))
}

pub(super) fn edge_map(
    snapshot: &GraphSnapshot,
    by_from: bool,
) -> BTreeMap<String, Vec<&GraphEdge>> {
    let mut map: BTreeMap<String, Vec<&GraphEdge>> = BTreeMap::new();
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

pub(super) fn undirected_neighbors(snapshot: &GraphSnapshot, path: &str) -> BTreeSet<String> {
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

pub(super) fn compare_ranked(
    left: &GraphRankedNode,
    right: &GraphRankedNode,
) -> std::cmp::Ordering {
    right
        .score
        .partial_cmp(&left.score)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| relation_rank(left.relation).cmp(&relation_rank(right.relation)))
        .then_with(|| left.hops.cmp(&right.hops))
        .then_with(|| left.vault_path.cmp(&right.vault_path))
}

fn update_candidate(candidate: &mut Candidate, edge: &GraphEdge, next_hops: u8) {
    candidate.count = candidate.count.saturating_add(edge.weight);
    candidate.hops = candidate.hops.min(next_hops);
    if next_hops > 1 {
        candidate.two_hop = candidate.two_hop.saturating_add(1);
    }
}

fn new_candidate(edge: &GraphEdge, relation: GraphRelation, next_hops: u8) -> Candidate {
    Candidate {
        relation: if next_hops == 1 {
            relation
        } else {
            GraphRelation::Related
        },
        link_text: edge.link_text.clone(),
        count: edge.weight,
        hops: next_hops,
        two_hop: u32::from(next_hops > 1),
    }
}

fn direct_out(candidate: &Candidate) -> f64 {
    if candidate.relation == GraphRelation::Outgoing && candidate.hops == 1 {
        f64::from(candidate.count).min(4.0)
    } else {
        0.0
    }
}

fn direct_backlink(candidate: &Candidate) -> f64 {
    if candidate.relation == GraphRelation::Backlink && candidate.hops == 1 {
        f64::from(candidate.count).min(4.0)
    } else {
        0.0
    }
}

fn shared_sources(
    snapshot: &GraphSnapshot,
    source_sources: &[String],
    candidate_sources: &[String],
) -> f64 {
    source_sources
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
        .min(3.0)
}

fn common_neighbors(
    snapshot: &GraphSnapshot,
    source_neighbors: &BTreeSet<String>,
    candidate_neighbors: &BTreeSet<String>,
) -> f64 {
    source_neighbors
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
        .min(3.0)
}

fn community_affinity(
    source_community: Option<u32>,
    candidate_community: Option<u32>,
    community_neighbor_count: u32,
    bridge_weight: f64,
) -> f64 {
    let same = if source_community.is_some() && source_community == candidate_community {
        1.0
    } else {
        0.0
    };
    let bridge = if source_community != candidate_community && community_neighbor_count > 0 {
        bridge_weight.min(3.0) * BRIDGE_BONUS
    } else {
        0.0
    };
    same + bridge
}

const fn type_affinity(source_type: Option<&str>, candidate_type: Option<&str>) -> f64 {
    let Some(source_type) = source_type else {
        return 0.0;
    };
    let Some(candidate_type) = candidate_type else {
        return 0.0;
    };
    if source_type.eq_ignore_ascii_case(candidate_type) {
        1.0
    } else {
        0.0
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
                    COMMUNITY_WEIGHT.mul_add(
                        signals.community_affinity,
                        TYPE_AFFINITY_WEIGHT
                            .mul_add(signals.type_affinity, -signals.structural_penalty),
                    ),
                ),
            ),
        ),
    )
}

fn directed_edges<'a>(
    direction: Direction,
    path: &str,
    outgoing: &'a BTreeMap<String, Vec<&'a GraphEdge>>,
    backlinks: &'a BTreeMap<String, Vec<&'a GraphEdge>>,
) -> Vec<&'a GraphEdge> {
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

const fn relation_rank(relation: GraphRelation) -> u8 {
    match relation {
        GraphRelation::Outgoing => 0,
        GraphRelation::Backlink => 1,
        GraphRelation::Related => 2,
    }
}
