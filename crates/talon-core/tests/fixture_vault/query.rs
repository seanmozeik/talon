use super::*;

#[test]
fn fixture_vault_related_hub_depth2() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("related");
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

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
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

    let input = RelatedInput {
        path: "Graph/Hub.md".to_string(),
        depth: 2,
        direction: Direction::Outgoing,
        scope: Vec::new(),
        scope_only: Vec::new(),
        scope_all: false,
        limit: None,
    };

    let response = talon_core::find_related(&conn, &input, None);

    let paths: Vec<&str> = response
        .results
        .iter()
        .map(|r| r.vault_path.as_str())
        .collect();

    assert!(paths.contains(&"Graph/Child.md"), "Hub must link to Child");
    assert!(paths.contains(&"Graph/Side.md"), "Hub must link to Side");
    assert!(
        paths.contains(&"Graph/Inbound.md"),
        "Hub must link to Inbound"
    );
    assert!(
        paths.contains(&"Graph/Grandchild.md"),
        "Hub depth=2 must reach Grandchild via Child"
    );
    assert!(
        !paths.contains(&"Graph/Hub.md"),
        "Hub must not appear in its own related results"
    );

    drop(conn);
    cleanup(&vault);
}

#[test]
fn fixture_vault_meta_where_archived() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("meta");
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

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
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

    let input = MetaInput {
        where_: vec![WhereClause {
            key: "status".to_string(),
            op: WhereOperator::Equals,
            value: Some("archived".to_string()),
        }],
        since: None,
        scope: Vec::new(),
        scope_only: Vec::new(),
        scope_all: false,
        select: Vec::new(),
        tag_counts: false,
        sources: None,
        limit: PositiveCount::new(50, "limit").unwrap(),
    };

    let response = query_meta(&conn, &input, None);

    assert_eq!(
        response.entries.len(),
        1,
        "exactly one note has status=archived"
    );
    assert_eq!(
        response.entries[0].path.as_str(),
        "Filters/Frontmatter.md",
        "only Frontmatter note has status=archived"
    );

    drop(conn);
    cleanup(&vault);
}

#[test]
fn fixture_vault_inspect_orphans() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("inspect");
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

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
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

    let input = InspectInput {
        check: InspectCheck::Orphans,
        scope: Vec::new(),
        scope_only: Vec::new(),
        scope_all: false,
        skip_llm_suggestions: false,
    };

    let response = query_inspect(&conn, &input, None);

    assert!(
        !response.findings.is_empty(),
        "orphan check must find at least one orphan"
    );

    let orphan_paths: Vec<&str> = response.findings.iter().map(|f| f.path.as_str()).collect();

    assert!(
        orphan_paths.contains(&"Search/Banana Grove.md"),
        "Banana Grove has no incoming links and must be an orphan"
    );
    assert!(
        !orphan_paths.contains(&"Graph/Child.md"),
        "Graph/Child has incoming links and must not be an orphan"
    );
    assert!(
        !orphan_paths.contains(&"Graph/Grandchild.md"),
        "Graph/Grandchild has incoming links and must not be an orphan"
    );

    drop(conn);
    cleanup(&vault);
}

#[test]
fn fixture_vault_frontmatter_excluded_from_chunks() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("fm-chunks");
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
            .and(path("/embed-chunked"))
            .respond_with(EmbedChunkedResponder)
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("POST"))
            .and(path("/embed"))
            .respond_with(ResponseTemplate::new(200).set_body_json(embed_response_5d()))
            .mount(&server),
    );

    let mut conn = open_database(&db).unwrap();
    let client = InferenceClient::new(server.uri()).unwrap();
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

    let rows: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT c.text, c.embedding_text FROM chunks c \
                 JOIN notes n ON c.note_id = n.id \
                 WHERE n.vault_path = 'Filters/Frontmatter.md' AND n.active = 1",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .filter_map(Result::ok)
            .collect()
    };

    assert!(
        !rows.is_empty(),
        "Filters/Frontmatter.md must have at least one chunk"
    );
    for (text, emb) in &rows {
        assert!(
            !text.contains("status:"),
            "chunk.text must not contain 'status:' from frontmatter YAML: {text:?}"
        );
        assert!(
            !text.contains("archived"),
            "chunk.text must not contain 'archived' (frontmatter value): {text:?}"
        );
        assert!(
            !emb.contains("status:"),
            "embedding_text must not contain 'status:': {emb:?}"
        );
    }

    let meta_input = MetaInput {
        where_: vec![WhereClause {
            key: "status".to_string(),
            op: WhereOperator::Equals,
            value: Some("archived".to_string()),
        }],
        ..MetaInput::default()
    };
    let meta_resp = query_meta(&conn, &meta_input, None);
    let paths: Vec<_> = meta_resp.entries.iter().map(|e| e.path.as_str()).collect();
    assert!(
        paths.contains(&"Filters/Frontmatter.md"),
        "meta --where status=archived must still find Filters/Frontmatter.md; got: {paths:?}"
    );

    drop(conn);
    cleanup(&vault);
}
