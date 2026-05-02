use std::collections::{BTreeMap, BTreeSet};

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
        },
    );
    assert!(ranked.iter().any(|node| node.vault_path == "D.md"));
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
