use serde::Serialize;
use talon_core::AskResponse;

#[derive(Debug, Serialize)]
pub(super) struct AgentAskResponse<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    vault: Option<&'a str>,
    answer: &'a str,
    sources: Vec<AgentAskSource<'a>>,
}

impl<'a> From<&'a AskResponse> for AgentAskResponse<'a> {
    fn from(ask: &'a AskResponse) -> Self {
        Self {
            vault: ask.vault.as_ref().map(talon_core::ContainerPath::as_str),
            answer: &ask.answer,
            sources: ask
                .sources
                .iter()
                .map(|s| AgentAskSource {
                    path: s.vault_path.as_str(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct AgentAskSource<'a> {
    path: &'a str,
}
