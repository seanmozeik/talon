//! Deterministic Louvain-style community detection for vault graphs.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::GraphSnapshot;

const RESOLUTION: f64 = 1.0;
const MAX_PASSES: usize = 20;
const MIN_GAIN: f64 = 0.000_000_1;

/// Persisted community summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityInfo {
    /// Stable community id.
    pub community_id: u32,
    /// Number of nodes assigned to this community.
    pub node_count: u32,
    /// Internal undirected edge density.
    pub cohesion: f64,
    /// Deterministic top nodes by internal weighted degree.
    pub top_nodes: Vec<String>,
}

/// Detects communities and writes assignments/bridge metrics into `snapshot`.
#[must_use]
pub fn detect_communities(snapshot: &mut GraphSnapshot) -> Vec<CommunityInfo> {
    if snapshot.nodes.is_empty() {
        return Vec::new();
    }
    let graph = WeightedGraph::from_snapshot(snapshot);
    let raw_assignments = optimize_modularity(&graph);
    let assignments = renumber_assignments(&raw_assignments);
    apply_assignments(snapshot, &assignments);
    let communities = summarize_communities(snapshot, &graph, &assignments);
    apply_bridge_metrics(snapshot, &graph, &assignments, &communities);
    communities
}

fn optimize_modularity(graph: &WeightedGraph) -> BTreeMap<String, String> {
    let mut assignment = graph
        .nodes
        .iter()
        .map(|node| (node.clone(), node.clone()))
        .collect::<BTreeMap<_, _>>();
    if graph.total_weight <= 0.0 {
        return assignment;
    }

    for _ in 0..MAX_PASSES {
        let mut moved = false;
        for node in &graph.nodes {
            let current = assignment[node].clone();
            let mut candidates = graph
                .neighbors
                .get(node)
                .into_iter()
                .flat_map(|neighbors| neighbors.keys())
                .filter_map(|neighbor| assignment.get(neighbor))
                .cloned()
                .collect::<BTreeSet<_>>();
            candidates.insert(current.clone());

            let mut best = current.clone();
            let mut best_gain = 0.0;
            for candidate in candidates {
                let gain = modularity_gain(graph, &assignment, node, &candidate);
                if gain > best_gain + MIN_GAIN
                    || (nearly_equal(gain, best_gain) && candidate < best)
                {
                    best = candidate;
                    best_gain = gain;
                }
            }
            if best != current {
                assignment.insert(node.clone(), best);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    assignment
}

fn modularity_gain(
    graph: &WeightedGraph,
    assignment: &BTreeMap<String, String>,
    node: &str,
    target_community: &str,
) -> f64 {
    let node_degree = graph.weighted_degree(node);
    let target_total = assignment
        .iter()
        .filter(|(path, community)| path.as_str() != node && community.as_str() == target_community)
        .map(|(path, _community)| graph.weighted_degree(path))
        .sum::<f64>();
    let links_to_target = graph
        .neighbors
        .get(node)
        .into_iter()
        .flat_map(|neighbors| neighbors.iter())
        .filter(|(neighbor, _weight)| {
            assignment
                .get(*neighbor)
                .is_some_and(|community| community == target_community)
        })
        .map(|(_neighbor, weight)| *weight)
        .sum::<f64>();
    links_to_target - RESOLUTION * target_total * node_degree / (2.0 * graph.total_weight)
}

fn renumber_assignments(raw: &BTreeMap<String, String>) -> BTreeMap<String, u32> {
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (node, community) in raw {
        grouped
            .entry(community.clone())
            .or_default()
            .push(node.clone());
    }
    let mut groups = grouped.into_values().collect::<Vec<_>>();
    groups.sort_by(|left, right| {
        right
            .len()
            .cmp(&left.len())
            .then_with(|| left.first().cmp(&right.first()))
    });
    let mut assignments = BTreeMap::new();
    for (community_id, nodes) in groups.into_iter().enumerate() {
        let id = community_id.try_into().unwrap_or(u32::MAX);
        for node in nodes {
            assignments.insert(node, id);
        }
    }
    assignments
}

fn apply_assignments(snapshot: &mut GraphSnapshot, assignments: &BTreeMap<String, u32>) {
    for (path, community_id) in assignments {
        if let Some(node) = snapshot.nodes.get_mut(path) {
            node.community_id = Some(*community_id);
        }
    }
}

fn summarize_communities(
    snapshot: &mut GraphSnapshot,
    graph: &WeightedGraph,
    assignments: &BTreeMap<String, u32>,
) -> Vec<CommunityInfo> {
    let mut grouped: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for (path, community_id) in assignments {
        grouped.entry(*community_id).or_default().push(path.clone());
    }
    let mut summaries = Vec::new();
    for (community_id, nodes) in grouped {
        let cohesion = community_cohesion(graph, &nodes);
        let top_nodes = top_internal_nodes(graph, assignments, community_id, &nodes);
        for path in &nodes {
            if let Some(node) = snapshot.nodes.get_mut(path) {
                node.community_cohesion = cohesion;
            }
        }
        summaries.push(CommunityInfo {
            community_id,
            node_count: nodes.len().try_into().unwrap_or(u32::MAX),
            cohesion,
            top_nodes,
        });
    }
    summaries
}

fn community_cohesion(graph: &WeightedGraph, nodes: &[String]) -> f64 {
    if nodes.len() <= 1 {
        return 0.0;
    }
    let node_set = nodes.iter().collect::<BTreeSet<_>>();
    let internal_edges = graph
        .undirected_edges
        .iter()
        .filter(|(left, right, _weight)| node_set.contains(left) && node_set.contains(right))
        .count();
    let possible = nodes.len().saturating_mul(nodes.len().saturating_sub(1)) / 2;
    let internal: u32 = internal_edges.try_into().unwrap_or(u32::MAX);
    let possible: u32 = possible.try_into().unwrap_or(u32::MAX);
    f64::from(internal) / f64::from(possible.max(1))
}

fn top_internal_nodes(
    graph: &WeightedGraph,
    assignments: &BTreeMap<String, u32>,
    community_id: u32,
    nodes: &[String],
) -> Vec<String> {
    let mut ranked = nodes
        .iter()
        .map(|node| {
            let internal = graph
                .neighbors
                .get(node)
                .into_iter()
                .flat_map(|neighbors| neighbors.iter())
                .filter(|(neighbor, _weight)| assignments.get(*neighbor) == Some(&community_id))
                .map(|(_neighbor, weight)| *weight)
                .sum::<f64>();
            (node.clone(), internal)
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
    ranked
        .into_iter()
        .take(5)
        .map(|(path, _score)| path)
        .collect()
}

fn apply_bridge_metrics(
    snapshot: &mut GraphSnapshot,
    graph: &WeightedGraph,
    assignments: &BTreeMap<String, u32>,
    communities: &[CommunityInfo],
) {
    let community_lookup = communities
        .iter()
        .map(|community| (community.community_id, community.cohesion))
        .collect::<BTreeMap<_, _>>();
    for path in &graph.nodes {
        let own = assignments.get(path).copied();
        let mut neighbor_communities = BTreeSet::new();
        let mut bridge_weight = 0.0;
        if let Some(neighbors) = graph.neighbors.get(path) {
            for (neighbor, weight) in neighbors {
                let neighbor_community = assignments.get(neighbor).copied();
                if neighbor_community != own {
                    if let Some(id) = neighbor_community {
                        neighbor_communities.insert(id);
                    }
                    bridge_weight += *weight;
                }
            }
        }
        if let Some(node) = snapshot.nodes.get_mut(path) {
            node.community_neighbor_count =
                neighbor_communities.len().try_into().unwrap_or(u32::MAX);
            node.bridge_weight = bridge_weight;
            if let Some(community_id) = own
                && let Some(cohesion) = community_lookup.get(&community_id)
            {
                node.community_cohesion = *cohesion;
            }
        }
    }
}

fn nearly_equal(left: f64, right: f64) -> bool {
    (left - right).abs() <= MIN_GAIN
}

#[derive(Debug, Clone, Default)]
struct WeightedGraph {
    nodes: Vec<String>,
    neighbors: BTreeMap<String, BTreeMap<String, f64>>,
    undirected_edges: Vec<(String, String, f64)>,
    total_weight: f64,
}

impl WeightedGraph {
    fn from_snapshot(snapshot: &GraphSnapshot) -> Self {
        let mut graph = Self {
            nodes: snapshot.nodes.keys().cloned().collect(),
            ..Self::default()
        };
        let mut edge_weights: BTreeMap<(String, String), f64> = BTreeMap::new();
        for edge in &snapshot.edges {
            if !snapshot.nodes.contains_key(&edge.from_path)
                || !snapshot.nodes.contains_key(&edge.to_path)
                || edge.from_path == edge.to_path
            {
                continue;
            }
            let (left, right) = if edge.from_path <= edge.to_path {
                (edge.from_path.clone(), edge.to_path.clone())
            } else {
                (edge.to_path.clone(), edge.from_path.clone())
            };
            let capped = f64::from(edge.weight.min(4));
            *edge_weights.entry((left, right)).or_default() += capped;
        }
        for ((left, right), weight) in edge_weights {
            graph
                .neighbors
                .entry(left.clone())
                .or_default()
                .insert(right.clone(), weight);
            graph
                .neighbors
                .entry(right.clone())
                .or_default()
                .insert(left.clone(), weight);
            graph.undirected_edges.push((left, right, weight));
            graph.total_weight += weight;
        }
        graph
    }

    fn weighted_degree(&self, node: &str) -> f64 {
        self.neighbors
            .get(node)
            .map_or(0.0, |neighbors| neighbors.values().sum())
    }
}
