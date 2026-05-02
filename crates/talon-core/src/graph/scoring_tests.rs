use std::collections::{BTreeMap, BTreeSet};

use crate::config::ScopePriority;
use crate::graph::{GraphEdge, GraphNode, GraphRankInput, GraphSnapshot, rank_related};
use crate::search::Direction;

#[test]
fn direct_link_beats_two_hop_candidate() {
    let snapshot = fixture_snapshot();
    let ranked = rank_related(
        &snapshot,
        &GraphRankInput {
            source_path: "A.md".into(),
            direction: Direction::Both,
            depth: 2,
            limit: 10,
            scope_priorities: BTreeMap::new(),
        },
    );
    assert_eq!(ranked[0].vault_path, "B.md");
    assert!(ranked[0].score > ranked[1].score);
}

#[test]
fn shared_sources_can_surface_without_direct_link() {
    let snapshot = fixture_snapshot();
    let ranked = rank_related(
        &snapshot,
        &GraphRankInput {
            source_path: "A.md".into(),
            direction: Direction::Both,
            depth: 1,
            limit: 10,
            scope_priorities: BTreeMap::new(),
        },
    );
    assert!(ranked.iter().any(|node| node.vault_path == "D.md"));
}

#[test]
fn hub_common_neighbors_are_downweighted() {
    let snapshot = hub_snapshot();
    let ranked = rank_related(
        &snapshot,
        &GraphRankInput {
            source_path: "Seed.md".into(),
            direction: Direction::Both,
            depth: 2,
            limit: 10,
            scope_priorities: BTreeMap::new(),
        },
    );
    let Some(hub_only) = ranked.iter().find(|node| node.vault_path == "HubOnly.md") else {
        panic!("missing hub-only candidate");
    };
    let Some(focused) = ranked.iter().find(|node| node.vault_path == "Focused.md") else {
        panic!("missing focused candidate");
    };
    assert!(focused.signals.common_neighbors > hub_only.signals.common_neighbors);
}

#[test]
fn structural_page_penalty_downranks_index_pages() {
    let snapshot = structural_snapshot();
    let ranked = rank_related(
        &snapshot,
        &GraphRankInput {
            source_path: "Seed.md".into(),
            direction: Direction::Outgoing,
            depth: 1,
            limit: 10,
            scope_priorities: BTreeMap::new(),
        },
    );
    assert_eq!(ranked[0].vault_path, "Article.md");
    assert!(ranked[1].signals.structural_penalty > 0.0);
}

#[test]
fn bridge_aware_scoring_surfaces_cross_community_notes() {
    let snapshot = bridge_snapshot();
    let ranked = rank_related(
        &snapshot,
        &GraphRankInput {
            source_path: "Source.md".into(),
            direction: Direction::Both,
            depth: 1,
            limit: 10,
            scope_priorities: BTreeMap::new(),
        },
    );
    assert_eq!(ranked[0].vault_path, "Bridge.md");
    assert!(ranked[0].signals.community_affinity > 0.0);
}

#[test]
fn configured_scope_priority_affects_ranking() {
    let snapshot = scoped_snapshot();
    let ranked = rank_related(
        &snapshot,
        &GraphRankInput {
            source_path: "Seed.md".into(),
            direction: Direction::Outgoing,
            depth: 1,
            limit: 10,
            scope_priorities: BTreeMap::from([("muted".into(), ScopePriority::Muted)]),
        },
    );
    assert_eq!(ranked[0].vault_path, "Normal.md");
}

#[test]
fn matching_note_type_boosts_related_candidates() {
    let mut snapshot = snapshot_with_nodes(["Seed.md", "Different.md", "Same.md"]);
    node_mut(&mut snapshot, "Seed.md").note_type = Some("case-study".into());
    node_mut(&mut snapshot, "Same.md").note_type = Some("case-study".into());
    node_mut(&mut snapshot, "Different.md").note_type = Some("reference".into());
    snapshot.edges = vec![
        edge("Seed.md", "Different.md", 1),
        edge("Seed.md", "Same.md", 1),
    ];
    let ranked = rank_related(
        &snapshot,
        &GraphRankInput {
            source_path: "Seed.md".into(),
            direction: Direction::Outgoing,
            depth: 1,
            limit: 10,
            scope_priorities: BTreeMap::new(),
        },
    );
    assert_eq!(ranked[0].vault_path, "Same.md");
    assert!(ranked[0].signals.type_affinity > 0.99);
}

#[test]
fn deterministic_sort_order_uses_vault_path_tie_breaker() {
    let snapshot = tie_snapshot();
    let ranked = rank_related(
        &snapshot,
        &GraphRankInput {
            source_path: "Seed.md".into(),
            direction: Direction::Outgoing,
            depth: 1,
            limit: 10,
            scope_priorities: BTreeMap::new(),
        },
    );
    assert_eq!(ranked[0].vault_path, "Alpha.md");
    assert_eq!(ranked[1].vault_path, "Beta.md");
}

fn fixture_snapshot() -> GraphSnapshot {
    let nodes = ["A.md", "B.md", "C.md", "D.md"]
        .into_iter()
        .map(|path| {
            (
                path.to_string(),
                GraphNode {
                    vault_path: path.to_string(),
                    title: path.to_string(),
                    aliases: Vec::new(),
                    tags: Vec::new(),
                    scope: String::new(),
                    note_type: None,
                    sources: if matches!(path, "A.md" | "D.md") {
                        vec!["Source.md".into()]
                    } else {
                        Vec::new()
                    },
                    outgoing_degree: 0,
                    backlink_degree: 0,
                    total_degree: 0,
                    structural: false,
                    community_id: None,
                    community_cohesion: 0.0,
                    community_neighbor_count: 0,
                    bridge_weight: 0.0,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    GraphSnapshot {
        db_version: 1,
        built_at: None,
        nodes,
        edges: vec![
            edge("A.md", "B.md", 2),
            edge("B.md", "C.md", 1),
            edge("A.md", "C.md", 1),
        ],
        source_citations: BTreeMap::from([(
            "Source.md".into(),
            BTreeSet::from(["A.md".into(), "D.md".into()]),
        )]),
    }
}

fn edge(from_path: &str, to_path: &str, weight: u32) -> GraphEdge {
    GraphEdge {
        from_path: from_path.into(),
        to_path: to_path.into(),
        link_text: to_path.into(),
        weight,
    }
}

fn hub_snapshot() -> GraphSnapshot {
    let mut snapshot = snapshot_with_nodes([
        "Seed.md",
        "Hub.md",
        "FocusedNeighbor.md",
        "Focused.md",
        "HubOnly.md",
        "N1.md",
        "N2.md",
        "N3.md",
        "N4.md",
    ]);
    snapshot.edges = vec![
        edge("Seed.md", "Hub.md", 1),
        edge("Seed.md", "FocusedNeighbor.md", 1),
        edge("Hub.md", "HubOnly.md", 1),
        edge("FocusedNeighbor.md", "Focused.md", 1),
        edge("Hub.md", "N1.md", 1),
        edge("Hub.md", "N2.md", 1),
        edge("Hub.md", "N3.md", 1),
        edge("Hub.md", "N4.md", 1),
    ];
    snapshot
}

fn structural_snapshot() -> GraphSnapshot {
    let mut snapshot = snapshot_with_nodes(["Seed.md", "Index.md", "Article.md"]);
    node_mut(&mut snapshot, "Index.md").structural = true;
    snapshot.edges = vec![
        edge("Seed.md", "Index.md", 1),
        edge("Seed.md", "Article.md", 1),
    ];
    snapshot
}

fn bridge_snapshot() -> GraphSnapshot {
    let mut snapshot = snapshot_with_nodes(["Source.md", "Plain.md", "Bridge.md"]);
    node_mut(&mut snapshot, "Source.md").community_id = Some(0);
    node_mut(&mut snapshot, "Plain.md").community_id = Some(1);
    let bridge = node_mut(&mut snapshot, "Bridge.md");
    bridge.community_id = Some(1);
    bridge.community_neighbor_count = 2;
    bridge.bridge_weight = 2.0;
    snapshot.edges = vec![
        edge("Source.md", "Plain.md", 1),
        edge("Source.md", "Bridge.md", 1),
    ];
    snapshot
}

fn scoped_snapshot() -> GraphSnapshot {
    let mut snapshot = snapshot_with_nodes(["Seed.md", "Muted.md", "Normal.md"]);
    node_mut(&mut snapshot, "Muted.md").scope = "muted".into();
    snapshot.edges = vec![
        edge("Seed.md", "Muted.md", 1),
        edge("Seed.md", "Normal.md", 1),
    ];
    snapshot
}

fn tie_snapshot() -> GraphSnapshot {
    let mut snapshot = snapshot_with_nodes(["Seed.md", "Beta.md", "Alpha.md"]);
    snapshot.edges = vec![
        edge("Seed.md", "Beta.md", 1),
        edge("Seed.md", "Alpha.md", 1),
    ];
    snapshot
}

fn snapshot_with_nodes(paths: impl IntoIterator<Item = &'static str>) -> GraphSnapshot {
    GraphSnapshot {
        nodes: paths
            .into_iter()
            .map(|path| (path.to_string(), node(path)))
            .collect(),
        ..GraphSnapshot::default()
    }
}

fn node(path: &str) -> GraphNode {
    GraphNode {
        vault_path: path.to_string(),
        title: path.to_string(),
        aliases: Vec::new(),
        tags: Vec::new(),
        scope: String::new(),
        note_type: None,
        sources: Vec::new(),
        outgoing_degree: 0,
        backlink_degree: 0,
        total_degree: 0,
        structural: false,
        community_id: None,
        community_cohesion: 0.0,
        community_neighbor_count: 0,
        bridge_weight: 0.0,
    }
}

fn node_mut<'a>(snapshot: &'a mut GraphSnapshot, path: &str) -> &'a mut GraphNode {
    let Some(node) = snapshot.nodes.get_mut(path) else {
        panic!("missing node {path}");
    };
    node
}
