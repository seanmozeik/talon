use serde::Serialize;
use talon_core::{AskResponse, AskSource};

#[derive(Debug, Serialize)]
pub(super) struct AgentAskResponse<'a> {
    answer: &'a str,
    queries: &'a [String],
    sources: Vec<AgentAskSource<'a>>,
}

impl<'a> From<&'a AskResponse> for AgentAskResponse<'a> {
    fn from(ask: &'a AskResponse) -> Self {
        Self {
            answer: &ask.answer,
            queries: &ask.queries,
            sources: ask.sources.iter().map(AgentAskSource::from).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct AgentAskSource<'a> {
    path: &'a str,
    title: &'a str,
    snippet: &'a str,
    score: f64,
}

impl<'a> From<&'a AskSource> for AgentAskSource<'a> {
    fn from(source: &'a AskSource) -> Self {
        Self {
            path: source.vault_path.as_str(),
            title: &source.title,
            snippet: &source.snippet,
            score: round_score(source.score),
        }
    }
}

fn round_score(score: f64) -> f64 {
    (score * 1000.0).round() / 1000.0
}
