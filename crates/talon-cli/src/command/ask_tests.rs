use std::path::PathBuf;

use talon_core::{AskSource, VaultPath};

use super::{
    ASK_SYNTHESIS_INPUT_DENOMINATOR, ASK_SYNTHESIS_INPUT_NUMERATOR, ask_synthesis_tokens,
    trim_ask_sources_to_budget,
};

#[test]
fn trim_ask_sources_greedily_packs_ranked_sources_into_input_budget() {
    let mut config = crate::config::default_config_for_vault(PathBuf::from("/tmp/vault"));
    config.chat.ask.context_tokens = 1_000;
    config.chat.ask.max_output_tokens = 100;
    let question = "topic";
    let queries = vec!["topic".to_string()];
    let mut sources = vec![
        ask_source("first.md", 80),
        ask_source("second.md", 80),
        ask_source("third.md", 80),
    ];

    trim_ask_sources_to_budget(question, &queries, &mut sources, &config);

    let input_budget = config.chat.ask.context_tokens as usize * ASK_SYNTHESIS_INPUT_NUMERATOR
        / ASK_SYNTHESIS_INPUT_DENOMINATOR;
    assert!(ask_synthesis_tokens(question, &queries, &sources) <= input_budget);
    assert_eq!(
        sources
            .iter()
            .map(|source| source.vault_path.as_str())
            .collect::<Vec<_>>(),
        vec!["first.md", "second.md"]
    );
}

fn ask_source(path: &str, words: usize) -> AskSource {
    AskSource {
        vault_path: VaultPath::parse(path).unwrap_or_else(|err| panic!("valid vault path: {err}")),
        title: path.to_string(),
        snippet: std::iter::repeat_n("word", words)
            .collect::<Vec<_>>()
            .join(" "),
        score: 1.0,
    }
}
