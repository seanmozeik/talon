use super::*;

#[test]
fn fixture_vault_search_with_anchors() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("anchors");
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

    let anchors_input = SearchInput {
        query: Some("orchard apple".to_string()),
        mode: SearchMode::Fulltext,
        limit: PositiveCount::new(5, "limit").unwrap(),
        anchors: Some(true),
        ..SearchInput::default()
    };
    let resp = run_search(&conn, &anchors_input, None, None, None);
    if !resp.results.is_empty() {
        let first = &resp.results[0];
        assert!(
            first.preview_anchors.is_some(),
            "anchors=true must populate preview_anchors; got None for {:?}",
            first.vault_path.as_str()
        );
        let anchors = first.preview_anchors.as_ref().unwrap();
        assert!(
            !anchors.is_empty(),
            "preview_anchors must have at least one entry"
        );
        let bm25_anchor = anchors.iter().find(|a| a.kind == AnchorKind::Bm25);
        if let Some(bm25) = bm25_anchor {
            assert!(
                !bm25.match_text.is_empty(),
                "BM25 anchor match_text must not be empty"
            );
            assert!(
                bm25.match_text.chars().count() <= 80,
                "match_text must be <= 80 chars"
            );
        }
    }

    let no_anchors_input = SearchInput {
        query: Some("orchard apple".to_string()),
        mode: SearchMode::Fulltext,
        limit: PositiveCount::new(5, "limit").unwrap(),
        anchors: None,
        ..SearchInput::default()
    };
    let resp_no = run_search(&conn, &no_anchors_input, None, None, None);
    for result in &resp_no.results {
        assert!(
            result.preview_anchors.is_none(),
            "anchors=None must leave preview_anchors as None on {:?}",
            result.vault_path.as_str()
        );
    }

    for result in &resp.results {
        let _ = result.snippet.len();
    }

    drop(conn);
    cleanup(&vault);
}
