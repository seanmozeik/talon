use super::*;
use talon_core::inference::{EmbeddingClient, RerankClient};

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

    let embedding = EmbeddingClient::tei_for_tests(server.uri(), "embed").unwrap();
    let rerank = RerankClient::tei_for_tests(server.uri(), 32).unwrap();
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
        let results: Vec<String> =
            run_search(&conn, &input, Some(&embedding), Some(&rerank), None, None)
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
