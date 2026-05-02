//! Graph refinement for search results.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use crate::config::TalonConfig;
use crate::graph::{GraphRankInput, GraphSnapshot, load_graph_snapshot, rank_related};
use crate::search::types::{RawSearchResult, SearchScores};
use crate::search::{Direction, SearchInput, SearchMode};

use super::search::ScoredRawSearchResult;

const SEED_MIN_SCORE: f64 = 0.62;
const GRAPH_EXISTING_BLEND: f64 = 0.04;
const GRAPH_ONLY_BLEND: f64 = 0.025;
const GRAPH_ONLY_LIMIT: usize = 4;
const GRAPH_PER_COMMUNITY_LIMIT: usize = 2;

pub(super) fn refine_graph_results(
    conn: &Connection,
    input: &SearchInput,
    config: Option<&TalonConfig>,
    scored: &mut Vec<ScoredRawSearchResult>,
) {
    if !graph_refinement_enabled(input) {
        return;
    }
    let Ok(snapshot) = load_graph_snapshot(conn) else {
        return;
    };
    if snapshot.nodes.is_empty() || snapshot.edges.is_empty() {
        return;
    }

    let existing = scored
        .iter()
        .map(|result| result.raw.path.clone())
        .collect::<BTreeSet<_>>();
    let mut by_path = scored
        .iter()
        .enumerate()
        .map(|(index, result)| (result.raw.path.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let mut graph_only = Vec::new();
    let mut community_counts: BTreeMap<Option<u32>, usize> = BTreeMap::new();

    let seeds = scored
        .iter()
        .take(8)
        .filter(|result| result.raw.score >= SEED_MIN_SCORE)
        .map(|result| (result.raw.path.clone(), result.raw.score))
        .collect::<Vec<_>>();
    for (seed_path, seed_score) in seeds {
        let ranked = rank_related(
            &snapshot,
            &GraphRankInput {
                source_path: seed_path,
                direction: Direction::Both,
                depth: input.depth.clamp(1, crate::constants::RELATED_MAX_DEPTH),
                limit: 8,
                scope_priorities: scope_priorities(config),
            },
        );
        for candidate in ranked {
            let contribution = seed_score * candidate.score;
            if let Some(index) = by_path.get(&candidate.vault_path).copied() {
                scored[index].raw.score += contribution * GRAPH_EXISTING_BLEND;
                continue;
            }
            if graph_only.len() >= GRAPH_ONLY_LIMIT || existing.contains(&candidate.vault_path) {
                continue;
            }
            let community = snapshot
                .nodes
                .get(&candidate.vault_path)
                .and_then(|node| node.community_id);
            let count = community_counts.entry(community).or_default();
            if *count >= GRAPH_PER_COMMUNITY_LIMIT {
                continue;
            }
            let Some(raw) = graph_only_result(&snapshot, &candidate.vault_path, contribution)
            else {
                continue;
            };
            *count = count.saturating_add(1);
            by_path.insert(candidate.vault_path, scored.len() + graph_only.len());
            graph_only.push(raw);
        }
    }

    scored.extend(graph_only);
}

fn graph_refinement_enabled(input: &SearchInput) -> bool {
    input.related || (input.mode == SearchMode::Hybrid && !input.fast)
}

fn scope_priorities(
    config: Option<&TalonConfig>,
) -> BTreeMap<String, crate::config::ScopePriority> {
    config
        .map(|cfg| {
            cfg.scopes
                .iter()
                .map(|(name, scope)| (name.clone(), scope.priority))
                .collect()
        })
        .unwrap_or_default()
}

fn graph_only_result(
    snapshot: &GraphSnapshot,
    path: &str,
    contribution: f64,
) -> Option<ScoredRawSearchResult> {
    let node = snapshot.nodes.get(path)?;
    let score = (contribution * GRAPH_ONLY_BLEND).min(0.72);
    Some(ScoredRawSearchResult {
        raw: RawSearchResult {
            path: path.to_string(),
            title: node.title.clone(),
            tags: node.tags.clone(),
            aliases: node.aliases.clone(),
            snippet: String::new(),
            score,
            scores: SearchScores {
                hybrid: Some(score),
                ..SearchScores::default()
            },
            semantic_heading: None,
            semantic_char_start: None,
            semantic_char_end: None,
        },
        raw_score: score,
    })
}

#[cfg(test)]
mod tests {
    use rusqlite::{Connection, params};

    use crate::indexing::migrations::run_migrations;
    use crate::query::search::ScoredRawSearchResult;
    use crate::search::input::SearchInput;
    use crate::search::types::{RawSearchResult, SearchScores};

    use super::refine_graph_results;

    #[test]
    fn graph_refinement_adds_bounded_graph_only_candidate() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut conn = Connection::open_in_memory()?;
        run_migrations(&mut conn)?;
        insert_graph_node(&conn, "Seed.md")?;
        insert_graph_node(&conn, "Neighbor.md")?;
        insert_graph_edge(&conn, "Seed.md", "Neighbor.md", 2)?;
        let mut scored = vec![scored("Seed.md", 0.9)];

        refine_graph_results(&conn, &SearchInput::default(), None, &mut scored);

        assert_eq!(scored.len(), 2);
        assert_eq!(scored[1].raw.path, "Neighbor.md");
        Ok(())
    }

    #[test]
    fn graph_refinement_boosts_existing_neighbor() -> Result<(), Box<dyn std::error::Error>> {
        let mut conn = Connection::open_in_memory()?;
        run_migrations(&mut conn)?;
        insert_graph_node(&conn, "Seed.md")?;
        insert_graph_node(&conn, "Neighbor.md")?;
        insert_graph_edge(&conn, "Seed.md", "Neighbor.md", 2)?;
        let mut scored = vec![scored("Seed.md", 0.9), scored("Neighbor.md", 0.2)];

        refine_graph_results(&conn, &SearchInput::default(), None, &mut scored);

        assert!(scored[1].raw.score > 0.2);
        Ok(())
    }

    fn scored(path: &str, score: f64) -> ScoredRawSearchResult {
        ScoredRawSearchResult {
            raw: RawSearchResult {
                path: path.into(),
                title: path.into(),
                tags: Vec::new(),
                aliases: Vec::new(),
                snippet: String::new(),
                score,
                scores: SearchScores {
                    hybrid: Some(score),
                    ..SearchScores::default()
                },
                semantic_heading: None,
                semantic_char_start: None,
                semantic_char_end: None,
            },
            raw_score: score,
        }
    }

    fn insert_graph_node(conn: &Connection, path: &str) -> Result<(), rusqlite::Error> {
        conn.execute(
            "INSERT INTO graph_nodes (
               vault_path, title, aliases, tags, scope, note_type, sources,
               outgoing_degree, backlink_degree, total_degree, structural,
               community_id, community_cohesion, community_neighbor_count, bridge_weight
             ) VALUES (?1, ?1, '[]', '[]', '', NULL, '[]', 0, 0, 0, 0, NULL, 0.0, 0, 0.0)",
            params![path],
        )?;
        Ok(())
    }

    fn insert_graph_edge(
        conn: &Connection,
        from_path: &str,
        to_path: &str,
        weight: u32,
    ) -> Result<(), rusqlite::Error> {
        conn.execute(
            "INSERT INTO graph_edges (from_path, to_path, link_text, weight)
             VALUES (?1, ?2, ?2, ?3)",
            params![from_path, to_path, weight],
        )?;
        Ok(())
    }
}
