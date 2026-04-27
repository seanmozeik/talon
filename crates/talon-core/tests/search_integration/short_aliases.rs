use super::*;

#[test]
fn search_title_mode_returns_short_alias_matches() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("short-aliases");
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    fs_err::create_dir_all(&vault).unwrap();
    fs_err::write(
        vault.join("alpha.md"),
        r#"---
title: Alpha Note
aliases: ["A"]
---

# Alpha Note

Short alias A.
"#,
    )
    .unwrap();
    fs_err::write(
        vault.join("go.md"),
        r#"---
title: Go Note
aliases: ["Go"]
---

# Go Note

Short alias Go.
"#,
    )
    .unwrap();
    fs_err::write(
        vault.join("csharp.md"),
        r#"---
title: CSharp Note
aliases: ["C#"]
---

# CSharp Note

Short alias C#.
"#,
    )
    .unwrap();

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
    assert_eq!(stats.indexed, 3, "all short-alias notes must be indexed");

    let mut input = SearchInput {
        query: Some("A".to_string()),
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
        where_: Vec::new(),
        since: None,
        anchors: None,
    };

    for (query, expected_path) in [("A", "alpha.md"), ("Go", "go.md"), ("C#", "csharp.md")] {
        input.query = Some(query.to_string());
        let response = run_search(&conn, &input, None, None, None);
        assert!(
            !response.results.is_empty(),
            "search for {query} must return results"
        );
        assert_eq!(
            response.results[0].vault_path.as_str(),
            expected_path,
            "short alias {query} must resolve to {expected_path}"
        );
    }

    drop(conn);
    cleanup(&vault);
}
