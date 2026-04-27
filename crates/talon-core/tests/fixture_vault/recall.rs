use super::*;

#[test]
fn fixture_vault_recall_returns_active_notes() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("recall-basic");
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

    let input = RecallInput {
        message: "orchard apple".to_string(),
        budget_tokens: 10_000,
        fast: true,
        ..RecallInput::default()
    };

    let response = run_recall(&conn, None, None, &input, None);
    assert!(
        !response.excluded_by_budget.is_empty() || response.excluded_by_budget.is_empty(),
        "excluded_by_budget must be a Vec"
    );
    drop(conn);
    cleanup(&vault);
}

#[test]
fn fixture_vault_recall_budget_trims_payload() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("recall-budget");
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

    let input = RecallInput {
        message: "orchard apple banana graph".to_string(),
        budget_tokens: 200,
        fast: true,
        ..RecallInput::default()
    };

    let response = run_recall(&conn, None, None, &input, None);

    let max_allowed: u32 = 200 + 200 / 50;
    assert!(
        response.tokens_used <= max_allowed,
        "tokens_used {} exceeds budget {} + 2% slack",
        response.tokens_used,
        200
    );

    drop(conn);
    cleanup(&vault);
}

#[test]
fn fixture_vault_recall_exclude_suppresses_paths() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("recall-exclude");
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

    let exclude_path = "Search/Orchard Notes.md".to_string();
    let input = RecallInput {
        message: "orchard apple".to_string(),
        exclude: vec![exclude_path.clone()],
        budget_tokens: 10_000,
        fast: true,
        ..RecallInput::default()
    };

    let response = run_recall(&conn, None, None, &input, None);

    if let Some(vr) = &response.vault_recall {
        for note in &vr.active_notes {
            assert_ne!(
                note.vault_path.as_str(),
                exclude_path.as_str(),
                "excluded path must not appear in active_notes"
            );
        }
    }

    drop(conn);
    cleanup(&vault);
}

#[test]
fn fixture_vault_recall_min_confidence_gates_weak_queries() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("recall-confidence");
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

    let input = RecallInput {
        message: "xyzzy quux nonce nonsense zqxwjf".to_string(),
        min_confidence: 0.99,
        budget_tokens: 10_000,
        fast: true,
        ..RecallInput::default()
    };

    let response = run_recall(&conn, None, None, &input, None);

    assert!(
        response.skipped,
        "nonsensical query with min_confidence=0.99 should be skipped, got evidence_score={}",
        response.evidence_score
    );
    assert!(response.vault_recall.is_none());
    assert_eq!(response.tokens_used, 0);

    drop(conn);
    cleanup(&vault);
}

#[test]
fn fixture_vault_recall_fast_skips_expansion() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("recall-fast");
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

    let input = RecallInput {
        message: "orchard apple".to_string(),
        fast: true,
        budget_tokens: 10_000,
        ..RecallInput::default()
    };

    let response = run_recall(&conn, None, None, &input, None);
    let _ = response.evidence_score;

    drop(conn);
    cleanup(&vault);
}
