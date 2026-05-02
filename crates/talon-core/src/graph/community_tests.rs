use std::collections::BTreeMap;

use crate::graph::{GraphEdge, GraphNode, GraphSnapshot, detect_communities};

#[test]
fn communities_are_stable_for_two_clusters_and_bridge() {
    let mut snapshot = fixture_snapshot();
    let first = detect_communities(&mut snapshot);
    let first_assignments = assignments(&snapshot);

    let mut second_snapshot = fixture_snapshot();
    let second = detect_communities(&mut second_snapshot);
    assert_eq!(first, second);
    assert_eq!(first_assignments, assignments(&second_snapshot));
    assert!(first.len() >= 2);
}

#[test]
fn bridge_node_records_neighboring_communities() {
    let mut snapshot = fixture_snapshot();
    let _ = detect_communities(&mut snapshot);

    let Some(bridge) = snapshot.nodes.get("Bridge.md") else {
        panic!("missing bridge node");
    };
    assert!(bridge.community_neighbor_count >= 1);
    assert!(bridge.bridge_weight > 0.0);
}

#[test]
fn source_overlap_does_not_create_community_edges() {
    let mut snapshot = GraphSnapshot {
        nodes: BTreeMap::from([("A.md".into(), node("A.md")), ("B.md".into(), node("B.md"))]),
        source_citations: BTreeMap::from([(
            "Shared.md".into(),
            ["A.md".into(), "B.md".into()].into_iter().collect(),
        )]),
        ..GraphSnapshot::default()
    };
    let communities = detect_communities(&mut snapshot);
    assert_eq!(communities.len(), 2);
}

fn assignments(snapshot: &GraphSnapshot) -> BTreeMap<String, Option<u32>> {
    snapshot
        .nodes
        .iter()
        .map(|(path, node)| (path.clone(), node.community_id))
        .collect()
}

fn fixture_snapshot() -> GraphSnapshot {
    let paths = [
        "A1.md",
        "A2.md",
        "A3.md",
        "Bridge.md",
        "B1.md",
        "B2.md",
        "B3.md",
    ];
    GraphSnapshot {
        nodes: paths
            .into_iter()
            .map(|path| (path.to_string(), node(path)))
            .collect(),
        edges: vec![
            edge("A1.md", "A2.md", 3),
            edge("A2.md", "A3.md", 3),
            edge("A1.md", "A3.md", 3),
            edge("B1.md", "B2.md", 3),
            edge("B2.md", "B3.md", 3),
            edge("B1.md", "B3.md", 3),
            edge("A3.md", "Bridge.md", 1),
            edge("Bridge.md", "B1.md", 1),
        ],
        ..GraphSnapshot::default()
    }
}

fn node(path: &str) -> GraphNode {
    GraphNode {
        vault_path: path.into(),
        title: path.into(),
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

fn edge(from_path: &str, to_path: &str, weight: u32) -> GraphEdge {
    GraphEdge {
        from_path: from_path.into(),
        to_path: to_path.into(),
        link_text: to_path.into(),
        weight,
    }
}
