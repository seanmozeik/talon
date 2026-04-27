use super::*;

/// Test that FTS5 tokenchars '+#' allows searching for C++, C#, and F#.
/// These special characters should be treated as part of tokens rather than
/// delimiters, enabling exact matching of language names.
#[test]
#[allow(clippy::similar_names, clippy::too_many_lines)]
fn fts_tokenchars_cpp_search() {
    register_sqlite_vec().unwrap();
    let vault = unique_path("fts-tokenchars-cpp");
    let db = vault.join("idx.sqlite");
    let lock = vault.join(".talon").join("sync.lock");

    fs_err::create_dir_all(&vault).unwrap();

    fs_err::write(
        vault.join("cpp-notes.md"),
        "# C++ Programming Notes\n\nLearn modern C++ with best practices.\n",
    )
    .unwrap();
    fs_err::write(
        vault.join("csharp-notes.md"),
        "# C# Programming Notes\n\nMastering C# for .NET development.\n",
    )
    .unwrap();
    fs_err::write(
        vault.join("fsharp-notes.md"),
        "# F# Programming Notes\n\nFunctional programming with F#.\n",
    )
    .unwrap();
    fs_err::write(
        vault.join("general-notes.md"),
        "# General Programming\n\nCommon programming concepts and patterns.\n",
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
    assert_eq!(stats.indexed, 4, "all 4 notes must be indexed");

    // Test C++ search
    let input = SearchInput {
        query: Some("C++".to_string()),
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
        where_: Vec::new(),
        since: None,
        anchors: None,
    };

    let response = run_search(&conn, &input, None, None, None);
    assert!(
        !response.results.is_empty(),
        "search for C++ must return results"
    );
    assert_eq!(
        response.results[0].vault_path.as_str(),
        "cpp-notes.md",
        "C++ Programming Notes must be the top result for C++ search"
    );

    // Test C# search
    let mut input = SearchInput {
        query: Some("C#".to_string()),
        ..input
    };

    let response = run_search(&conn, &input, None, None, None);
    assert!(
        !response.results.is_empty(),
        "search for C# must return results"
    );
    assert_eq!(
        response.results[0].vault_path.as_str(),
        "csharp-notes.md",
        "C# Programming Notes must be the top result for C# search"
    );

    // Test F# search
    input.query = Some("F#".to_string());

    let response = run_search(&conn, &input, None, None, None);
    assert!(
        !response.results.is_empty(),
        "search for F# must return results"
    );
    assert_eq!(
        response.results[0].vault_path.as_str(),
        "fsharp-notes.md",
        "F# Programming Notes must be the top result for F# search"
    );

    drop(conn);
    cleanup(&vault);
}
