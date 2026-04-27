use super::*;

#[test]
fn fixture_vault_hybrid_search_returns_results() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("hybrid");
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
            .respond_with(ResponseTemplate::new(200).set_body_json(embed_response_5d()))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(EmbedChunkedResponder)
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
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"index": 0, "score": 0.9}
            ])))
            .mount(&server),
    );

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let expansion = talon_core::ExpansionClient::new(server.uri(), "test-model").unwrap();
    let config = IndexerConfig::index_all();

    run_sync_with_chunker(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
        &fixture_chunker(),
    )
    .unwrap();

    let input = SearchInput {
        query: Some("fruit harvest".to_string()),
        queries: Vec::new(),
        intent: None,
        mode: SearchMode::Hybrid,
        fast: false,
        limit: PositiveCount::new(10, "limit").unwrap(),
        candidate_limit: PositiveCount::new(40, "candidate_limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        where_: Vec::new(),
        since: None,
        anchors: None,
    };

    let response = run_search(&conn, &input, Some(&client), Some(&expansion), None);
    assert!(
        !response.results.is_empty(),
        "hybrid search must return results"
    );

    assert!(
        response
            .results
            .iter()
            .any(|r| r.vault_path.as_str() == "Search/Fruit Orchard.md"),
        "Fruit Orchard must appear in hybrid search for 'fruit harvest'"
    );

    drop(conn);
    cleanup(&vault);
}

#[test]
fn fixture_vault_hybrid_search_with_intent_ranks_web_performance_first() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("hybrid-intent");
    seed_fixture_vault(&vault);
    let notes = vault.join("notes");
    fs_err::create_dir_all(&notes).unwrap();
    fs_err::write(
        notes.join("sports-perf.md"),
        "# Sports Performance\n\nPerformance training improves sprint speed and recovery.",
    )
    .unwrap();
    fs_err::write(
        notes.join("web-perf.md"),
        "# Web Performance\n\nPerformance work improves web page load times and paint metrics.",
    )
    .unwrap();
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
            .respond_with(ResponseTemplate::new(200).set_body_json(embed_response_5d()))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed-chunked"))
            .respond_with(EmbedChunkedResponder)
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": "{\"queries\":[]}"}}]
            })))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/rerank"))
            .respond_with(IntentPerfRerankResponder)
            .mount(&server),
    );

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let expansion = talon_core::ExpansionClient::new(server.uri(), "test-model").unwrap();
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

    let input = SearchInput {
        query: Some("performance".to_string()),
        queries: Vec::new(),
        intent: Some("web page load".to_string()),
        mode: SearchMode::Hybrid,
        fast: false,
        limit: PositiveCount::new(5, "limit").unwrap(),
        candidate_limit: PositiveCount::new(40, "candidate_limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        where_: Vec::new(),
        since: None,
        anchors: None,
    };

    let response = run_search(&conn, &input, Some(&client), Some(&expansion), None);
    assert_eq!(
        response
            .results
            .first()
            .map(|result| result.vault_path.as_str()),
        Some("notes/web-perf.md")
    );

    drop(conn);
    cleanup(&vault);
}

struct IntentPerfRerankResponder;

impl Respond for IntentPerfRerankResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value =
            serde_json::from_slice(&request.body).unwrap_or_else(|_| json!({"texts": []}));
        let texts = body
            .get("texts")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let results: Vec<serde_json::Value> = texts
            .iter()
            .enumerate()
            .map(|(index, text)| {
                let text = text.as_str().unwrap_or_default();
                let score = if text.contains("web page load") {
                    4.0
                } else {
                    -4.0
                };
                json!({"index": index, "score": score})
            })
            .collect();
        ResponseTemplate::new(200).set_body_json(results)
    }
}
