//! Ranking quality regression suite.
//!
//! Ports tests/services/talon/ranking-regression.test.ts (MIT, original by
//! seanmozeik) to Rust. The TS-parity bar (top-K paths match within tolerance)
//! is one sub-assertion; the primary floor is the nDCG/MRR/Hit metric suite
//! running against the full golden set in tests/fixtures/golden-set.json.
//!
//! FLOOR thresholds below were calibrated against post-US-013b chunker output.
//! Raise them if quality improves. Never lower.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::cast_precision_loss // all cast sites are small counts (n <= 5)
)]

mod eval;

#[path = "ranking_regression/default.rs"]
mod default;
#[path = "ranking_regression/fast.rs"]
mod fast;
#[path = "ranking_regression/golden.rs"]
mod golden;

use serde_json::json;
use talon_core::{
    ChunkerConfig, ExpansionClient, PositiveCount, SearchInput, SearchMode,
    embed::EmbedPassOptions, indexer::IndexerConfig, open_database, run_search,
    run_sync_with_chunker, vec_ext::register_sqlite_vec,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use eval::{
    EvalMetrics, GoldenQuery, SemanticEmbedChunkedResponder, SemanticQueryEmbedResponder,
    SemanticRerankResponder, aggregate_metrics, cleanup, hit_at_k, load_golden_set, mrr, ndcg,
    seed_fixture_vault, unique_path,
};

const FAST_NDCG5_FLOOR: f64 = 0.83;
const FAST_MRR_FLOOR: f64 = 0.90;
const FAST_HIT5_FLOOR: f64 = 0.95;

const DEFAULT_NDCG5_FLOOR: f64 = 0.85;
const DEFAULT_MRR_FLOOR: f64 = 0.85;
const DEFAULT_HIT5_FLOOR: f64 = 0.95;

const GOLDEN_NDCG5_FLOOR: f64 = 0.85;
const GOLDEN_MRR_FLOOR: f64 = 0.84;
const GOLDEN_HIT5_FLOOR: f64 = 0.95;
const GOLDEN_RECALL10_FLOOR: f64 = 0.90;

struct Benchmark {
    query: &'static str,
    relevant: &'static [&'static str],
}

const RANKING_BENCHMARKS: &[Benchmark] = &[
    Benchmark {
        query: "orchard",
        relevant: &[
            "Search/Fruit Orchard.md",
            "Atlas/Alpha.md",
            "Atlas/Beta.md",
            "Atlas/Gamma.md",
        ],
    },
    Benchmark {
        query: "banana",
        relevant: &["Search/Banana Grove.md"],
    },
    Benchmark {
        query: "graph",
        relevant: &[
            "Graph/Hub.md",
            "Graph/Child.md",
            "Graph/Inbound.md",
            "Graph/Grandchild.md",
        ],
    },
    Benchmark {
        query: "cafe",
        relevant: &["Search/Cafe Note.md", "Atlas/Gamma.md"],
    },
    Benchmark {
        query: "fruit basket",
        relevant: &["Search/Fruit Orchard.md", "Search/Banana Grove.md"],
    },
];

fn fixture_chunker() -> ChunkerConfig {
    ChunkerConfig {
        chunk_min_tokens: 1,
        ..ChunkerConfig::default()
    }
}
