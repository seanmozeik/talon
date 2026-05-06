use super::*;

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

    let metrics: EvalMetrics = aggregate_metrics(&golden, &all_results);

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
    if metrics.mrr < GOLDEN_MRR_FLOOR {
        print_low_mrr_queries(&golden, &all_results);
    }

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

fn print_low_mrr_queries(golden: &[GoldenQuery], all_results: &[Vec<String>]) {
    eprintln!("  Low-MRR queries:");
    for (query, results) in golden.iter().zip(all_results) {
        let refs: Vec<&str> = results.iter().map(String::as_str).collect();
        let expected: Vec<&str> = query.expected_paths.iter().map(String::as_str).collect();
        let score = mrr(&refs, &expected);
        if score < 1.0 {
            eprintln!(
                "    {} ({}) MRR={score:.3} top3={:?}",
                query.id,
                query.query,
                &results[..results.len().min(3)]
            );
        }
    }
}
