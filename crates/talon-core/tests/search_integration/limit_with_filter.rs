use super::*;

/// Regression test for the --limit-as-retrieval-cap bug.
///
/// Seeds 50 notes that all contain the query term "protocol".
/// 30 of those also have `status: active` frontmatter.
/// With --limit 10 and --where status:active the response must contain
/// exactly 10 results (not fewer), because the retriever now fetches a
/// wide pool before the filter trims it.
#[test]
fn limit_with_where_filter_returns_full_limit() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("limit-with-filter");
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    fs_err::create_dir_all(&vault).unwrap();

    // Seed 50 notes that all match the query term "protocol".
    // The first 30 get `status: active` in their frontmatter.
    for i in 0..50_u32 {
        let status = if i < 30 { "active" } else { "archived" };
        let content = format!(
            "---\nstatus: {status}\n---\n# Protocol Note {i}\n\nThis note discusses the protocol in detail. Protocol number {i}.\n"
        );
        fs_err::write(vault.join(format!("protocol-{i:02}.md")), &content).unwrap();
    }

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
    assert_eq!(stats.indexed, 50, "all 50 notes must be indexed");

    let where_clause = WhereClause {
        key: "status".to_string(),
        op: WhereOperator::Equals,
        value: Some("active".to_string()),
    };

    let input = SearchInput {
        query: Some("protocol".to_string()),
        queries: Vec::new(),
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
        where_: vec![where_clause],
        since: None,
        anchors: None,
    };

    let response = run_search(&conn, &input, None, None, None);

    assert_eq!(
        response.results.len(),
        10,
        "must return exactly 10 results when 30 active notes match and limit=10"
    );
    assert!(
        response.total >= u32::try_from(response.results.len()).unwrap_or(u32::MAX),
        "total ({}) must be >= results.len() ({})",
        response.total,
        response.results.len()
    );
    for r in &response.results {
        let path = r.vault_path.as_str();
        assert!(
            path.starts_with("protocol-0")
                || path.starts_with("protocol-1")
                || path.starts_with("protocol-2"),
            "all results must be active (protocol-00 through protocol-29), got: {path}"
        );
    }

    drop(conn);
    cleanup(&vault);
}
