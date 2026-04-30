use serde::Serialize;
use talon_core::{SearchResponse, SearchResult};

#[derive(Debug, Serialize)]
pub(super) struct AgentSearchResponse<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    vault: Option<&'a str>,
    results: Vec<AgentSearchHit<'a>>,
}

impl<'a> From<&'a SearchResponse> for AgentSearchResponse<'a> {
    fn from(search: &'a SearchResponse) -> Self {
        Self {
            vault: search.vault.as_ref().map(talon_core::ContainerPath::as_str),
            results: search.results.iter().map(AgentSearchHit::from).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct AgentSearchHit<'a> {
    path: &'a str,
    title: &'a str,
    snippet: &'a str,
    score: f64,
}

impl<'a> From<&'a SearchResult> for AgentSearchHit<'a> {
    fn from(result: &'a SearchResult) -> Self {
        Self {
            path: result.vault_path.as_str(),
            title: &result.title,
            snippet: &result.snippet,
            score: round_score(result.score),
        }
    }
}

fn round_score(score: f64) -> f64 {
    (score * 100.0).round() / 100.0
}
