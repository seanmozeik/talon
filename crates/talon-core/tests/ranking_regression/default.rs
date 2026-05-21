use super::*;
use talon_core::inference::{EmbeddingClient, RerankClient};
use wiremock::{Request, Respond};

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
    mount_default_mode_mocks(&rt, &server);

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
        let results: Vec<String> = run_search(
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

fn mount_default_mode_mocks(rt: &tokio::runtime::Runtime, server: &MockServer) {
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .respond_with(SemanticQueryEmbedResponder)
            .mount(server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(SemanticEmbedChunkedResponder)
            .mount(server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(DefaultExpansionResponder)
            .mount(server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/rerank"))
            .respond_with(SemanticRerankResponder)
            .mount(server),
    );
}

struct DefaultExpansionResponder;

impl Respond for DefaultExpansionResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value =
            serde_json::from_slice(&request.body).unwrap_or_else(|_| json!({"messages": []}));
        let query = body["messages"]
            .as_array()
            .and_then(|messages| {
                messages
                    .iter()
                    .rev()
                    .find(|message| message["role"].as_str() == Some("user"))
            })
            .and_then(|message| message["content"].as_str())
            .unwrap_or("")
            .to_lowercase();
        let queries = if query.contains("banana") {
            vec!["banana grove", "ripe banana"]
        } else if query.contains("graph") {
            vec!["graph hub", "linked notes"]
        } else if query.contains("cafe") || query.contains("café") {
            vec!["cafe note", "coffee service"]
        } else if query.contains("fruit basket") {
            vec!["fruit orchard", "banana grove"]
        } else {
            vec!["fruit orchard", "apple harvest"]
        };
        ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": json!({ "queries": queries }).to_string()}}]
        }))
    }
}
