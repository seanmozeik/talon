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
    clippy::cast_precision_loss // all cast sites are small counts (n ≤ 5)
)]

mod eval;

use serde_json::json;
use talon_core::{
    ChunkerConfig, ExpansionClient, PositiveCount, SearchInput, SearchMode,
    embed::EmbedPassOptions, indexer::IndexerConfig, inference::InferenceClient, open_database,
    run_search, run_sync_with_chunker, vec_ext::register_sqlite_vec,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use eval::{
    SemanticEmbedChunkedResponder, SemanticQueryEmbedResponder, SemanticRerankResponder,
    aggregate_metrics, cleanup, hit_at_k, load_golden_set, mrr, ndcg, seed_fixture_vault,
    unique_path,
};

// ── Floor thresholds ──────────────────────────────────────────────────────
//
// Calibrated against post-US-013b output. Raise on quality improvements.
// These match (or are slightly below) the TS reference floors where applicable.

// Baseline measured 2026-04-26 against post-US-013b chunker:
//   fast  : nDCG@5=0.922  MRR=1.000  Hit@5=1.000
//   default: nDCG@5=0.499  MRR=0.533  Hit@5=1.000  (mock expansion injects noise)
//   golden : nDCG@5=0.952  MRR=0.957  Hit@5=1.000  Recall@10=1.000

/// nDCG@5 minimum for fast mode (BM25 + semantic, no expansion/rerank).
const FAST_NDCG5_FLOOR: f64 = 0.83;
/// MRR minimum for fast mode.
const FAST_MRR_FLOOR: f64 = 0.90;
/// Hit@5 minimum for fast mode.
const FAST_HIT5_FLOOR: f64 = 0.95;

/// nDCG@5 minimum for default mode (hybrid + expansion + rerank).
/// Lower than fast because mock expansion always emits off-topic variants,
/// which injects RRF noise; real expansion would improve this.
const DEFAULT_NDCG5_FLOOR: f64 = 0.45;
/// MRR minimum for default mode.
const DEFAULT_MRR_FLOOR: f64 = 0.45;
/// Hit@5 minimum for default mode.
const DEFAULT_HIT5_FLOOR: f64 = 0.90;

/// nDCG@5 floor for the full 35-query golden set (hybrid fast).
const GOLDEN_NDCG5_FLOOR: f64 = 0.85;
/// MRR floor for the golden set.
const GOLDEN_MRR_FLOOR: f64 = 0.85;
/// Hit@5 floor for the golden set.
const GOLDEN_HIT5_FLOOR: f64 = 0.95;
/// Recall@10 floor for the golden set.
const GOLDEN_RECALL10_FLOOR: f64 = 0.90;

// ── 5 benchmark queries ported from ranking-regression.test.ts ────────────

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

// ── Test 1: fast mode (BM25 + semantic, no expansion/rerank) ─────────────

#[test]
fn ranking_regression_fast_mode_meets_floors() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("rrfast");
    seed_fixture_vault(&vault);
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = rt.block_on(MockServer::start());
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .respond_with(SemanticQueryEmbedResponder)
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(SemanticEmbedChunkedResponder)
            .mount(&server),
    );

    let client = InferenceClient::new(server.uri()).unwrap();
    let mut conn = open_database(&db).unwrap();
    run_sync_with_chunker(
        &mut conn,
        &vault,
        &lock,
        &IndexerConfig::index_all(),
        Some(EmbedPassOptions::defaults()),
        Some(&client),
        &fixture_chunker(),
    )
    .unwrap();

    let n = RANKING_BENCHMARKS.len() as f64;
    let mut sum_ndcg5 = 0.0_f64;
    let mut sum_mrr = 0.0_f64;
    let mut sum_hit5 = 0.0_f64;

    eprintln!("\nFast mode ranking metrics (5 TS benchmarks):");
    for b in RANKING_BENCHMARKS {
        let input = SearchInput {
            query: Some(b.query.to_string()),
            mode: SearchMode::Hybrid,
            fast: true,
            limit: PositiveCount::new(10, "limit").unwrap(),
            ..SearchInput::default()
        };
        let results: Vec<String> = run_search(&conn, &input, Some(&client), None, None)
            .results
            .into_iter()
            .map(|r| r.vault_path.as_str().to_string())
            .collect();
        let refs: Vec<&str> = results.iter().map(String::as_str).collect();
        let q_ndcg5 = ndcg(&refs, b.relevant, &[], 5);
        let q_mrr = mrr(&refs, b.relevant);
        let q_hit5 = hit_at_k(&refs, b.relevant, 5);
        sum_ndcg5 += q_ndcg5;
        sum_mrr += q_mrr;
        sum_hit5 += q_hit5;
        eprintln!(
            "  {:20} nDCG@5={:.3} MRR={:.3}  top3={:?}",
            b.query,
            q_ndcg5,
            q_mrr,
            &results[..results.len().min(3)],
        );
    }

    let avg_ndcg5 = sum_ndcg5 / n;
    let avg_mrr = sum_mrr / n;
    let avg_hit5 = sum_hit5 / n;
    eprintln!(
        "  Summary: nDCG@5={avg_ndcg5:.3} (floor {FAST_NDCG5_FLOOR})  MRR={avg_mrr:.3} (floor {FAST_MRR_FLOOR})  Hit@5={avg_hit5:.3} (floor {FAST_HIT5_FLOOR})"
    );

    drop(conn);
    cleanup(&vault);

    assert!(
        avg_ndcg5 >= FAST_NDCG5_FLOOR,
        "fast nDCG@5 {avg_ndcg5:.3} < floor {FAST_NDCG5_FLOOR}"
    );
    assert!(
        avg_mrr >= FAST_MRR_FLOOR,
        "fast MRR {avg_mrr:.3} < floor {FAST_MRR_FLOOR}"
    );
    assert!(
        avg_hit5 >= FAST_HIT5_FLOOR,
        "fast Hit@5 {avg_hit5:.3} < floor {FAST_HIT5_FLOOR}"
    );
}

// ── Test 2: default mode (hybrid + mock expansion + mock rerank) ──────────

#[allow(clippy::too_many_lines)]
#[test]
fn ranking_regression_default_mode_meets_floors() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("rrdefault");
    seed_fixture_vault(&vault);
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = rt.block_on(MockServer::start());
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .respond_with(SemanticQueryEmbedResponder)
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(SemanticEmbedChunkedResponder)
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": "{\"queries\":[\"orchard\",\"banana grove\"]}"}}]
            })))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/rerank"))
            .respond_with(SemanticRerankResponder)
            .mount(&server),
    );

    let client = InferenceClient::new(server.uri()).unwrap();
    let expansion = ExpansionClient::new(server.uri(), "test").unwrap();
    let mut conn = open_database(&db).unwrap();
    run_sync_with_chunker(
        &mut conn,
        &vault,
        &lock,
        &IndexerConfig::index_all(),
        Some(EmbedPassOptions::defaults()),
        Some(&client),
        &fixture_chunker(),
    )
    .unwrap();

    let n = RANKING_BENCHMARKS.len() as f64;
    let mut sum_ndcg5 = 0.0_f64;
    let mut sum_mrr = 0.0_f64;
    let mut sum_hit5 = 0.0_f64;

    eprintln!("\nDefault mode ranking metrics (5 TS benchmarks):");
    for b in RANKING_BENCHMARKS {
        let input = SearchInput {
            query: Some(b.query.to_string()),
            mode: SearchMode::Hybrid,
            fast: false,
            limit: PositiveCount::new(10, "limit").unwrap(),
            ..SearchInput::default()
        };
        let results: Vec<String> = run_search(&conn, &input, Some(&client), Some(&expansion), None)
            .results
            .into_iter()
            .map(|r| r.vault_path.as_str().to_string())
            .collect();
        let refs: Vec<&str> = results.iter().map(String::as_str).collect();
        let q_ndcg5 = ndcg(&refs, b.relevant, &[], 5);
        let q_mrr = mrr(&refs, b.relevant);
        let q_hit5 = hit_at_k(&refs, b.relevant, 5);
        sum_ndcg5 += q_ndcg5;
        sum_mrr += q_mrr;
        sum_hit5 += q_hit5;
        eprintln!(
            "  {:20} nDCG@5={:.3} MRR={:.3}  top3={:?}",
            b.query,
            q_ndcg5,
            q_mrr,
            &results[..results.len().min(3)],
        );
    }

    let avg_ndcg5 = sum_ndcg5 / n;
    let avg_mrr = sum_mrr / n;
    let avg_hit5 = sum_hit5 / n;
    eprintln!(
        "  Summary: nDCG@5={avg_ndcg5:.3} (floor {DEFAULT_NDCG5_FLOOR})  MRR={avg_mrr:.3} (floor {DEFAULT_MRR_FLOOR})  Hit@5={avg_hit5:.3} (floor {DEFAULT_HIT5_FLOOR})"
    );

    drop(conn);
    cleanup(&vault);

    assert!(
        avg_ndcg5 >= DEFAULT_NDCG5_FLOOR,
        "default nDCG@5 {avg_ndcg5:.3} < floor {DEFAULT_NDCG5_FLOOR}"
    );
    assert!(
        avg_mrr >= DEFAULT_MRR_FLOOR,
        "default MRR {avg_mrr:.3} < floor {DEFAULT_MRR_FLOOR}"
    );
    assert!(
        avg_hit5 >= DEFAULT_HIT5_FLOOR,
        "default Hit@5 {avg_hit5:.3} < floor {DEFAULT_HIT5_FLOOR}"
    );
}

// ── Test 3: golden-set floor regression ──────────────────────────────────

#[test]
fn ranking_regression_golden_set_meets_floors() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("rrgolden");
    seed_fixture_vault(&vault);
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = rt.block_on(MockServer::start());
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .respond_with(SemanticQueryEmbedResponder)
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(SemanticEmbedChunkedResponder)
            .mount(&server),
    );

    let client = InferenceClient::new(server.uri()).unwrap();
    let mut conn = open_database(&db).unwrap();
    run_sync_with_chunker(
        &mut conn,
        &vault,
        &lock,
        &IndexerConfig::index_all(),
        Some(EmbedPassOptions::defaults()),
        Some(&client),
        &fixture_chunker(),
    )
    .unwrap();

    let golden = load_golden_set();
    let all_results: Vec<Vec<String>> = golden
        .iter()
        .map(|q| {
            let input = SearchInput {
                query: Some(q.query.clone()),
                mode: SearchMode::Hybrid,
                fast: true,
                limit: PositiveCount::new(10, "limit").unwrap(),
                ..SearchInput::default()
            };
            run_search(&conn, &input, Some(&client), None, None)
                .results
                .into_iter()
                .map(|r| r.vault_path.as_str().to_string())
                .collect()
        })
        .collect();

    let metrics = aggregate_metrics(&golden, &all_results);

    eprintln!(
        "\nGolden set ({} queries, hybrid fast):",
        metrics.query_count
    );
    eprintln!(
        "  nDCG@5:    {:.3} (floor: {GOLDEN_NDCG5_FLOOR})",
        metrics.ndcg_at_5
    );
    eprintln!("  nDCG@10:   {:.3}", metrics.ndcg_at_10);
    eprintln!(
        "  MRR:       {:.3} (floor: {GOLDEN_MRR_FLOOR})",
        metrics.mrr
    );
    eprintln!(
        "  Hit@5:     {:.3} (floor: {GOLDEN_HIT5_FLOOR})",
        metrics.hit_at_5
    );
    eprintln!("  Hit@10:    {:.3}", metrics.hit_at_10);
    eprintln!(
        "  Recall@10: {:.3} (floor: {GOLDEN_RECALL10_FLOOR})",
        metrics.recall_at_10
    );

    drop(conn);
    cleanup(&vault);

    assert!(
        metrics.ndcg_at_5 >= GOLDEN_NDCG5_FLOOR,
        "golden nDCG@5 {:.3} < floor {GOLDEN_NDCG5_FLOOR}",
        metrics.ndcg_at_5
    );
    assert!(
        metrics.mrr >= GOLDEN_MRR_FLOOR,
        "golden MRR {:.3} < floor {GOLDEN_MRR_FLOOR}",
        metrics.mrr
    );
    assert!(
        metrics.hit_at_5 >= GOLDEN_HIT5_FLOOR,
        "golden Hit@5 {:.3} < floor {GOLDEN_HIT5_FLOOR}",
        metrics.hit_at_5
    );
    assert!(
        metrics.recall_at_10 >= GOLDEN_RECALL10_FLOOR,
        "golden Recall@10 {:.3} < floor {GOLDEN_RECALL10_FLOOR}",
        metrics.recall_at_10
    );
}
