use eyre::Result;
use serde::Serialize;
use std::collections::BTreeMap;
use talon_core::{
    ChangeEntry, ChangesResponse, MetaEntry, MetaResponse, ReadResult, RecallResponse,
    RelatedResult, SearchResult, StatusResponse, SyncResponse, TalonEnvelope, TalonResponseData,
    TombstoneEntry,
};

mod lint;

pub(super) fn emit(envelope: &TalonEnvelope) -> Result<()> {
    match envelope.data.as_ref() {
        Some(TalonResponseData::Search(search)) => {
            let hits: Vec<AgentSearchHit<'_>> =
                search.results.iter().map(AgentSearchHit::from).collect();
            super::emit_compact(&hits)
        }
        Some(TalonResponseData::Sync(sync)) => super::emit_compact(&AgentSync::from(sync)),
        Some(TalonResponseData::Status(status)) => super::emit_compact(&AgentStatus::from(status)),
        Some(TalonResponseData::Read(read)) => {
            let results: Vec<AgentReadResult<'_>> =
                read.results.iter().map(AgentReadResult::from).collect();
            super::emit_compact(&results)
        }
        Some(TalonResponseData::Related(related)) => {
            let results: Vec<AgentRelatedResult<'_>> = related
                .results
                .iter()
                .map(AgentRelatedResult::from)
                .collect();
            super::emit_compact(&results)
        }
        Some(TalonResponseData::Meta(meta)) => super::emit_compact(&AgentMeta::from(meta)),
        Some(TalonResponseData::Changes(changes)) => {
            super::emit_compact(&AgentChanges::from(changes))
        }
        Some(TalonResponseData::Lint(lint)) => super::emit_compact(&lint::AgentLint::from(lint)),
        Some(TalonResponseData::Recall(recall)) => super::emit_compact(&AgentRecall::from(recall)),
        None => envelope.error.as_ref().map_or_else(
            || super::emit_compact(envelope),
            |e| super::emit_compact(&AgentError::from(e)),
        ),
    }
}

#[derive(Debug, Serialize)]
struct AgentError<'a> {
    code: &'a talon_core::ErrorCode,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'a serde_json::Value>,
}

impl<'a> From<&'a talon_core::ErrorEnvelope> for AgentError<'a> {
    fn from(error: &'a talon_core::ErrorEnvelope) -> Self {
        Self {
            code: &error.code,
            message: &error.message,
            detail: error.detail.as_ref(),
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
            path: result.path.as_str(),
            title: &result.title,
            snippet: &result.snippet,
            score: round_score(result.score),
        }
    }
}

fn round_score(score: f64) -> f64 {
    (score * 100.0).round() / 100.0
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentSync<'a> {
    indexed: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    deleted: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    embedded: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    embed_failed: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimension_mismatch: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remediation: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<&'a str>,
}

impl<'a> From<&'a SyncResponse> for AgentSync<'a> {
    fn from(sync: &'a SyncResponse) -> Self {
        Self {
            indexed: sync.indexed,
            skipped: non_zero(sync.skipped),
            deleted: non_zero(sync.deleted),
            embedded: non_zero(sync.embedded),
            embed_failed: non_zero(sync.embed_failed),
            dimension_mismatch: sync.dimension_mismatch.then_some(true),
            remediation: sync.embed_remediation.as_deref(),
            diagnostics: sync.embed_diagnostics.iter().map(String::as_str).collect(),
        }
    }
}

const fn non_zero(value: u32) -> Option<u32> {
    if value == 0 { None } else { Some(value) }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentStatus<'a> {
    state: &'a talon_core::StatusState,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'a str>,
    notes: u32,
    chunks: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    failed_embeddings: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vector_dimensions: Option<u16>,
}

impl<'a> From<&'a StatusResponse> for AgentStatus<'a> {
    fn from(status: &'a StatusResponse) -> Self {
        Self {
            state: &status.state,
            reason: status.reason.as_deref(),
            notes: status.index.active_notes,
            chunks: status.index.chunk_count,
            failed_embeddings: non_zero(status.index.failed_embeddings),
            vector_dimensions: status.index.vector_dimensions,
        }
    }
}

#[derive(Debug, Serialize)]
struct AgentReadResult<'a> {
    path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    found: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    links: Vec<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    backlinks: Vec<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    aliases: Vec<&'a str>,
}

impl<'a> From<&'a ReadResult> for AgentReadResult<'a> {
    fn from(result: &'a ReadResult) -> Self {
        Self {
            path: result.vault_path.as_str(),
            found: (!result.found).then_some(false),
            title: result.title.as_deref(),
            content: result.content.as_deref(),
            links: result.links.iter().map(String::as_str).collect(),
            backlinks: result.backlinks.iter().map(String::as_str).collect(),
            tags: result.tags.iter().map(String::as_str).collect(),
            aliases: result.aliases.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentRelatedResult<'a> {
    path: &'a str,
    title: &'a str,
    relation: &'a talon_core::RelationKind,
    link_text: &'a str,
}

impl<'a> From<&'a RelatedResult> for AgentRelatedResult<'a> {
    fn from(result: &'a RelatedResult) -> Self {
        Self {
            path: result.vault_path.as_str(),
            title: &result.title,
            relation: &result.relation,
            link_text: &result.link_text,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentMeta<'a> {
    entries: Vec<AgentMetaEntry<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tag_counts: Option<&'a BTreeMap<String, u32>>,
}

#[derive(Debug, Serialize)]
struct AgentMetaEntry<'a> {
    path: &'a str,
    frontmatter: &'a BTreeMap<String, serde_json::Value>,
}

impl<'a> From<&'a MetaResponse> for AgentMeta<'a> {
    fn from(meta: &'a MetaResponse) -> Self {
        Self {
            entries: meta.entries.iter().map(AgentMetaEntry::from).collect(),
            tag_counts: meta.tag_counts.as_ref(),
        }
    }
}

impl<'a> From<&'a MetaEntry> for AgentMetaEntry<'a> {
    fn from(entry: &'a MetaEntry) -> Self {
        Self {
            path: entry.path.as_str(),
            frontmatter: &entry.frontmatter,
        }
    }
}

#[derive(Debug, Serialize)]
struct AgentChanges<'a> {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    added: Vec<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    modified: Vec<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    deleted: Vec<&'a str>,
}

impl<'a> From<&'a ChangesResponse> for AgentChanges<'a> {
    fn from(changes: &'a ChangesResponse) -> Self {
        Self {
            added: changes.added.iter().map(change_path).collect(),
            modified: changes.modified.iter().map(change_path).collect(),
            deleted: changes.deleted.iter().map(tombstone_path).collect(),
        }
    }
}

fn change_path(change: &ChangeEntry) -> &str {
    change.path.as_str()
}

fn tombstone_path(change: &TombstoneEntry) -> &str {
    change.path.as_str()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentRecall<'a> {
    notes: Vec<AgentRecallNote<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped: Option<bool>,
}

#[derive(Debug, Serialize)]
struct AgentRecallNote<'a> {
    path: &'a str,
    title: &'a str,
    snippet: &'a str,
    score: f64,
}

impl<'a> From<&'a RecallResponse> for AgentRecall<'a> {
    fn from(recall: &'a RecallResponse) -> Self {
        let notes = recall.vault_recall.as_ref().map_or_else(Vec::new, |vault| {
            vault
                .active_notes
                .iter()
                .map(|note| AgentRecallNote {
                    path: note
                        .path
                        .as_ref()
                        .map_or(note.vault_path.as_str(), talon_core::ContainerPath::as_str),
                    title: &note.title,
                    snippet: &note.snippet,
                    score: round_score(note.score),
                })
                .collect()
        });
        Self {
            notes,
            skipped: recall.skipped.then_some(true),
        }
    }
}
