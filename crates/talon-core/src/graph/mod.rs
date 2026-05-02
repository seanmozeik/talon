//! Persisted vault graph snapshot and graph intelligence helpers.

mod build;
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

pub use build::{GraphBuildInput, GraphBuildStats, rebuild_graph};
pub use community::{CommunityInfo, detect_communities};
pub use health::graph_health;
pub use scoring::{
    GraphRankInput, GraphRankedNode, GraphRelation, GraphSignalBreakdown, rank_related,
};
pub use snapshot::{GraphEdge, GraphNode, GraphSnapshot, load_graph_snapshot};
