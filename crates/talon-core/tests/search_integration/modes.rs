use super::*;

#[test]
fn search_hybrid_mode_returns_results() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("search-hybrid");
    seed_vault(&vault);
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
            .respond_with(ResponseTemplate::new(200).set_body_json(dummy_embed_response()))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {
                        "content": "{\"queries\":[\"atomic notes\",\"knowledge management\"]}"
                    }
                }]
            })))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/rerank"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"index": 0, "score": 0.95}
            ])))
            .mount(&server),
    );

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let expansion = talon_core::ExpansionClient::new(server.uri(), "test-model").unwrap();
    let config = IndexerConfig::index_all();

    let (stats, _) = run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();
    assert_eq!(stats.indexed, 3);

    let input = SearchInput {
        query: Some("atomic notes".to_string()),
        queries: Vec::new(),
        intent: None,
        mode: SearchMode::Hybrid,
        fast: false,
        limit: talon_core::PositiveCount::new(10, "limit").unwrap(),
        candidate_limit: talon_core::PositiveCount::new(40, "candidate_limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: talon_core::Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        scope_all: false,
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
            .any(|r| r.vault_path.as_str() == "atomic-notes.md"),
        "atomic-notes.md must appear in results"
    );

    drop(conn);
    cleanup(&vault);
}

#[test]
fn search_fulltext_mode_returns_results() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("search-fulltext");
    seed_vault(&vault);
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    let (_rt, _server, client) = mock_embed_sidecar();
    let mut conn = open_database(&db).unwrap();
    let config = IndexerConfig::index_all();

    let (stats, _) = run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();
    assert_eq!(stats.indexed, 3);

    let input = SearchInput {
        query: Some("spaced repetition".to_string()),
        queries: Vec::new(),
        intent: None,
        mode: SearchMode::Fulltext,
        fast: true,
        limit: talon_core::PositiveCount::new(10, "limit").unwrap(),
        candidate_limit: talon_core::PositiveCount::new(40, "candidate_limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: talon_core::Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        scope_all: false,
        where_: Vec::new(),
        since: None,
        anchors: None,
    };

    let response = run_search(&conn, &input, None, None, None);

    assert!(
        !response.results.is_empty(),
        "fulltext search must return results"
    );
    assert!(
        response
            .results
            .iter()
            .any(|r| r.vault_path.as_str() == "spaced-repetition.md"),
        "spaced-repetition.md must appear in results"
    );

    drop(conn);
    cleanup(&vault);
}

#[test]
fn search_title_mode_returns_results() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("search-title");
    seed_vault(&vault);
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    let (_rt, _server, client) = mock_embed_sidecar();
    let mut conn = open_database(&db).unwrap();
    let config = IndexerConfig::index_all();

    let (stats, _) = run_sync(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
    )
    .unwrap();
    assert_eq!(stats.indexed, 3);

    let input = SearchInput {
        query: Some("zettelkasten".to_string()),
        queries: Vec::new(),
        intent: None,
        mode: SearchMode::Title,
        fast: true,
        limit: talon_core::PositiveCount::new(10, "limit").unwrap(),
        candidate_limit: talon_core::PositiveCount::new(40, "candidate_limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: talon_core::Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        scope_all: false,
        where_: Vec::new(),
        since: None,
        anchors: None,
    };

    let response = run_search(&conn, &input, None, None, None);

    assert!(
        !response.results.is_empty(),
        "title search must return results"
    );
    assert!(
        response
            .results
            .iter()
            .any(|r| r.vault_path.as_str() == "zettelkasten.md"),
        "zettelkasten.md must appear in results"
    );

    drop(conn);
    cleanup(&vault);
}
