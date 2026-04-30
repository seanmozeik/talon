use serde::Serialize;
use talon_core::{RecallResponse, query::RecallDiagnostics};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AgentRecall<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault: Option<&'a str>,
    pub notes: Vec<AgentRecallNote<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<&'a RecallDiagnostics>,
}

#[derive(Debug, Serialize)]
pub(super) struct AgentRecallNote<'a> {
    pub path: &'a str,
    pub title: &'a str,
    pub snippet: &'a str,
    pub score: f64,
}

impl<'a> From<&'a RecallResponse> for AgentRecall<'a> {
    fn from(recall: &'a RecallResponse) -> Self {
        let notes = recall.vault_recall.as_ref().map_or_else(Vec::new, |vault| {
            vault
                .active_notes
                .iter()
                .map(|note| AgentRecallNote {
                    path: note.vault_path.as_str(),
                    title: &note.title,
                    snippet: &note.snippet,
                    score: super::round_score(note.score),
                })
                .collect()
        });
        Self {
            vault: recall.vault.as_ref().map(talon_core::ContainerPath::as_str),
            notes,
            skipped: recall.skipped.then_some(true),
            diagnostics: recall.diagnostics.as_ref(),
        }
    }
}
