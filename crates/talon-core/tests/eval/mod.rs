//! Ranking quality metrics and shared eval infrastructure.
//!
//! Metrics ported from obsidian-hybrid-search (MIT licensed) eval/metrics.ts
//! by flowing-abyss. <https://github.com/flowing-abyss/obsidian-hybrid-search>
//! Attribution: ndcg, mrr, `hit_at_k`, `recall_at_k` formulas match metrics.ts.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    dead_code,
    clippy::cast_precision_loss // all cast sites are over small collections (n <= 100)
)]

mod fixture;
mod golden;
mod metrics;
mod responders;
mod vector;

#[cfg(test)]
mod tests;

pub use fixture::{cleanup, seed_fixture_vault, unique_path};
pub use golden::{GoldenQuery, load_golden_set};
pub use metrics::{EvalMetrics, aggregate_metrics, hit_at_k, mrr, ndcg, recall_at_k};
pub use responders::{
    SemanticEmbedChunkedResponder, SemanticQueryEmbedResponder, SemanticRerankResponder,
};
pub use vector::make_vector;
