use super::*;

#[test]
fn normalize_queries_excludes_original_and_duplicates() {
    let queries = vec![
        "knife skills".to_string(),
        "Knife Skills".to_string(),
        "claw grip".to_string(),
        "julienne practice".to_string(),
    ];
    let normalized = normalize_queries("knife skills", queries, 4);
    assert_eq!(normalized, vec!["claw grip", "julienne practice"]);
}

#[test]
fn answer_prompt_preserves_ranked_source_order() {
    let sources = vec![
        ask_source("strong.md", "first relevant chunk"),
        ask_source("strong.md", "second relevant chunk"),
        ask_source("weaker.md", "weaker note chunk"),
    ];

    let message = build_answer_user_message("what matters?", &["query".to_string()], &sources);

    let first = message
        .find("[1] strong.md")
        .unwrap_or_else(|| panic!("missing first source in prompt:\n{message}"));
    let second = message
        .find("[2] strong.md")
        .unwrap_or_else(|| panic!("missing second source in prompt:\n{message}"));
    let third = message
        .find("[3] weaker.md")
        .unwrap_or_else(|| panic!("missing third source in prompt:\n{message}"));
    assert!(first < second);
    assert!(second < third);
    assert!(message.contains("Snippet: first relevant chunk"));
    assert!(message.contains("Snippet: second relevant chunk"));
}

fn ask_source(path: &str, snippet: &str) -> crate::AskSource {
    crate::AskSource {
        vault_path: crate::VaultPath::parse(path)
            .unwrap_or_else(|err| panic!("valid vault path: {err}")),
        title: path.to_string(),
        snippet: snippet.to_string(),
        score: 0.9,
    }
}
