use super::*;

#[test]
fn fixture_vault_sync_indexes_all_notes() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("sync");
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

    let (stats, embed_stats) = run_sync_with_chunker(
        &mut conn,
        &vault,
        &lock,
        &config,
        Some(EmbedPassOptions::defaults()),
        Some(&client),
        &fixture_chunker(),
    )
    .unwrap();

    assert_eq!(stats.indexed, 21, "all 21 fixture notes must be indexed");
    assert_eq!(stats.deleted, 0);

    let embed = embed_stats.expect("embed pass must run when not --fast");
    assert_eq!(embed.succeeded, 21, "all notes must embed successfully");
    assert_eq!(embed.failed, 0);

    let active: i64 = conn
        .query_row("SELECT COUNT(*) FROM notes WHERE active = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(active, 21);

    let vec_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
        .unwrap();
    assert!(
        vec_count >= 21,
        "vec_chunks must have at least one row per note"
    );

    drop(conn);
    cleanup(&vault);
}

#[test]
fn fixture_vault_status_counts() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("status");
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

    let talon_config = minimal_config(&vault);
    let response = query_status(&conn, &talon_config);

    assert_eq!(
        response.index.active_notes, 21,
        "status must report 21 active notes"
    );
    assert!(
        response.index.chunk_count >= 21,
        "at least one chunk per note"
    );
    assert_eq!(response.index.failed_embeddings, 0, "no embedding failures");
    assert_eq!(
        response.index.vector_dimensions,
        Some(5),
        "vector dimensions must match the 5-dim mock"
    );

    drop(conn);
    cleanup(&vault);
}
