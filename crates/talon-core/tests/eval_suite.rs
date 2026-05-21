//! Eval runner: executes the golden set against a freshly-synced fixture
//! vault and writes JSON results to tests/eval/results/latest.json.
//!
//! Run with:
//!   cargo test --test `eval_suite` -- --nocapture
//!
//! Results are written to:
//!   crates/talon-core/tests/eval/results/latest.json
//!
//! To update the committed baseline:
//!   cp crates/talon-core/tests/eval/results/latest.json \
//!      crates/talon-core/tests/eval/baseline.json
//!
//! Thresholds are raised never lowered. See tests/eval/README.md for full
//! instructions on running evals and updating the baseline.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod eval;

use serde_json::json;
use talon_core::{
    ChunkerConfig, ExpansionClient, PositiveCount, SearchInput, SearchMode,
    embed::EmbedPassOptions,
    indexer::IndexerConfig,
    inference::{EmbeddingClient, RerankClient},
    open_database, run_search, run_sync_with_chunker,
    vec_ext::register_sqlite_vec,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use eval::{
    EvalMetrics, SemanticEmbedChunkedResponder, SemanticQueryEmbedResponder,
    SemanticRerankResponder, aggregate_metrics, cleanup, hit_at_k, load_golden_set, mrr, ndcg,
    recall_at_k, seed_fixture_vault, unique_path,
};

fn fixture_chunker() -> ChunkerConfig {
    ChunkerConfig {
        chunk_min_tokens: 1,
        ..ChunkerConfig::default()
    }
}

#[allow(clippy::too_many_lines)]
#[test]
fn eval_suite_run_golden_set_and_write_results() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("eval");
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

    let embedding = EmbeddingClient::tei_for_tests(server.uri(), "embed").unwrap();
    let rerank = RerankClient::tei_for_tests(server.uri(), 32).unwrap();
    let expansion = ExpansionClient::new(server.uri(), "test").unwrap();

    let mut conn = open_database(&db).unwrap();
    run_sync_with_chunker(
        &mut conn,
        &vault,
        &lock,
        &IndexerConfig::index_all(),
        Some(EmbedPassOptions::defaults()),
        Some(&embedding),
        &fixture_chunker(),
    )
    .unwrap();

    let golden = load_golden_set();

    // ── Run hybrid-fast (deterministic, no expansion/rerank) ──────────────
    let fast_results: Vec<Vec<String>> = golden
        .iter()
        .map(|q| {
            let input = SearchInput {
                query: Some(q.query.clone()),
                mode: SearchMode::Hybrid,
                fast: true,
                limit: PositiveCount::new(10, "limit").unwrap(),
                ..SearchInput::default()
            };
            run_search(&conn, &input, Some(&embedding), Some(&rerank), None, None)
                .results
                .into_iter()
                .map(|r| r.vault_path.as_str().to_string())
                .collect()
        })
        .collect();

    // ── Run hybrid-default (expansion + rerank) ───────────────────────────
    let default_results: Vec<Vec<String>> = golden
        .iter()
        .map(|q| {
            let input = SearchInput {
                query: Some(q.query.clone()),
                mode: SearchMode::Hybrid,
                fast: false,
                limit: PositiveCount::new(10, "limit").unwrap(),
                ..SearchInput::default()
            };
            run_search(
                &conn,
                &input,
                Some(&embedding),
                Some(&rerank),
                Some(&expansion),
                None,
            )
            .results
            .into_iter()
            .map(|r| r.vault_path.as_str().to_string())
            .collect()
        })
        .collect();

    let fast_metrics = aggregate_metrics(&golden, &fast_results);
    let default_metrics = aggregate_metrics(&golden, &default_results);

    // ── Per-query breakdown ───────────────────────────────────────────────
    let per_query: Vec<serde_json::Value> = golden
        .iter()
        .zip(fast_results.iter())
        .map(|(q, res)| {
            let refs: Vec<&str> = res.iter().map(String::as_str).collect();
            let exp: Vec<&str> = q.expected_paths.iter().map(String::as_str).collect();
            let par: Vec<&str> = q.partial_paths.iter().map(String::as_str).collect();
            json!({
                "id": q.id,
                "query": q.query,
                "category": q.category,
                "ndcg_at_5": ndcg(&refs, &exp, &par, 5),
                "ndcg_at_10": ndcg(&refs, &exp, &par, 10),
                "mrr": mrr(&refs, &exp),
                "hit_at_5": hit_at_k(&refs, &exp, 5),
                "recall_at_10": recall_at_k(&refs, &exp, 10),
                "top_5_paths": &res[..res.len().min(5)],
                "expected_paths": q.expected_paths,
            })
        })
        .collect();

    // ── Build result JSON ─────────────────────────────────────────────────
    let results = json!({
        "run_at": unix_ts_now(),
        "mode": "hybrid_fast",
        "query_count": golden.len(),
        "fast": fast_metrics,
        "default": default_metrics,
        "per_query": per_query,
    });

    // ── Write to tests/eval/results/latest.json ───────────────────────────
    let results_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/eval/results");
    std::fs::create_dir_all(&results_dir).unwrap();
    let results_path = results_dir.join("latest.json");
    std::fs::write(
        &results_path,
        serde_json::to_string_pretty(&results).unwrap(),
    )
    .unwrap();

    // ── Print summary ─────────────────────────────────────────────────────
    print_metrics("hybrid-fast", &fast_metrics);
    print_metrics("hybrid-default (expansion+rerank)", &default_metrics);
    eprintln!("Results written to: {}", results_path.display());

    drop(conn);
    cleanup(&vault);
}

fn print_metrics(label: &str, m: &EvalMetrics) {
    eprintln!("\n{label} ({} queries):", m.query_count);
    eprintln!("  nDCG@5:    {:.3}", m.ndcg_at_5);
    eprintln!("  nDCG@10:   {:.3}", m.ndcg_at_10);
    eprintln!("  MRR:       {:.3}", m.mrr);
    eprintln!("  Hit@5:     {:.3}", m.hit_at_5);
    eprintln!("  Hit@10:    {:.3}", m.hit_at_10);
    eprintln!("  Recall@10: {:.3}", m.recall_at_10);
}

fn unix_ts_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
