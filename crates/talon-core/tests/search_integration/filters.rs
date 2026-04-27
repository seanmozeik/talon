use super::*;

#[test]
fn search_where_filter_applies() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("search-where");
    seed_vault(&vault);
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let server = rt.block_on(MockServer::start());
    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
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

    let where_clause = WhereClause {
        key: "type".to_string(),
        op: WhereOperator::Equals,
        value: Some("method".to_string()),
    };

    let input = SearchInput {
        query: Some("notes".to_string()),
        queries: Vec::new(),
        mode: SearchMode::Fulltext,
        fast: true,
        limit: talon_core::PositiveCount::new(10, "limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: talon_core::Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        where_: vec![where_clause],
        since: None,
        anchors: None,
    };

    let response = run_search(&conn, &input, None, None, None);

    assert!(
        response.results.is_empty(),
        "where filter should exclude all results when no match"
    );

    drop(conn);
    cleanup(&vault);
}

#[test]
fn search_since_filter_applies() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("search-since");
    seed_vault(&vault);
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let server = rt.block_on(MockServer::start());
    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
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
        query: Some("notes".to_string()),
        queries: Vec::new(),
        mode: SearchMode::Fulltext,
        fast: true,
        limit: talon_core::PositiveCount::new(10, "limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: talon_core::Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        where_: Vec::new(),
        since: Some("9999999999999".to_string()),
        anchors: None,
    };

    let response = run_search(&conn, &input, None, None, None);

    assert!(
        response.results.is_empty(),
        "since filter with far-future timestamp should return no results"
    );

    drop(conn);
    cleanup(&vault);
}

#[test]
fn search_empty_query_returns_empty() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("search-empty");
    seed_vault(&vault);
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let server = rt.block_on(MockServer::start());
    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
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
        query: Some(String::new()),
        queries: Vec::new(),
        mode: SearchMode::Hybrid,
        fast: false,
        limit: talon_core::PositiveCount::new(10, "limit").unwrap(),
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: 1,
        direction: talon_core::Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        where_: Vec::new(),
        since: None,
        anchors: None,
    };

    let response = run_search(&conn, &input, Some(&client), None, None);

    assert!(response.results.is_empty());
    assert_eq!(response.total, 0);

    drop(conn);
    cleanup(&vault);
}
