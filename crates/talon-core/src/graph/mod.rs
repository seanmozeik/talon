//! Persisted vault graph snapshot and graph intelligence helpers.

mod build;
mod build_suggestions;
#[cfg(test)]
mod build_tests;
mod community;
#[cfg(test)]
mod community_tests;
mod health;
mod scoring;
mod scoring_support;
#[cfg(test)]
mod scoring_tests;
mod snapshot;
mod storage;
mod suggest;
mod suggest_llm;
#[cfg(test)]
mod suggest_llm_tests;
#[cfg(test)]
mod suggest_tests;

pub use build::{GraphBuildInput, GraphBuildStats, rebuild_graph, rebuild_graph_with_suggester};
pub use community::{CommunityInfo, detect_communities};
pub use health::graph_health;
pub use scoring::{
    GraphRankInput, GraphRankedNode, GraphRelation, GraphSignalBreakdown, rank_related,
};
pub use snapshot::{GraphEdge, GraphNode, GraphSnapshot, load_graph_snapshot};
pub use suggest::{LinkSuggestion, build_missing_link_suggestions};
pub use suggest_llm::GraphSuggestionClient;
