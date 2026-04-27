use super::*;

#[test]
fn repeated_semantic_search_uses_cache_until_db_version_changes() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("search-cache");
    let db = vault.join("idx.sqlite");
    fs_err::create_dir_all(&vault).unwrap();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = rt.block_on(MockServer::start());
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .respond_with(ResponseTemplate::new(200).set_body_json(dummy_embed_response()))
            .expect(2)
            .mount(&server),
    );

    let conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
    let input = SearchInput {
        query: Some("cache probe unique query".to_string()),
        mode: SearchMode::Semantic,
        ..SearchInput::default()
    };

    let first = run_search(&conn, &input, Some(&client), None, None);
    let second = run_search(&conn, &input, Some(&client), None, None);
    bump_db_version(&conn).unwrap();
    let third = run_search(&conn, &input, Some(&client), None, None);

    assert_eq!(first, second);
    assert_eq!(third.total, first.total);

    drop(conn);
    cleanup(&vault);
}
