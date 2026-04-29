use eyre::Result;
use serde::Serialize;
use std::collections::BTreeMap;
use talon_core::{
    ChangeEntry, ChangesResponse, MetaEntry, MetaResponse, ReadResult, RelatedResult, SearchResult,
    StatusResponse, SyncResponse, TalonEnvelope, TalonResponseData, TombstoneEntry,
};

mod lint;
mod recall;

pub(super) fn emit(envelope: &TalonEnvelope) -> Result<()> {
    match envelope.data.as_ref() {
        Some(TalonResponseData::Search(search)) => {
            super::emit_compact(&AgentSearchResponse::from(search))
        }
        Some(TalonResponseData::Sync(sync)) => super::emit_compact(&AgentSync::from(sync)),
        Some(TalonResponseData::Status(status)) => super::emit_compact(&AgentStatus::from(status)),
        Some(TalonResponseData::Read(read)) => super::emit_compact(&AgentReadResponse::from(read)),
        Some(TalonResponseData::Related(related)) => {
            super::emit_compact(&AgentRelatedResponse::from(related))
        }
        Some(TalonResponseData::Meta(meta)) => super::emit_compact(&AgentMeta::from(meta)),
        Some(TalonResponseData::Changes(changes)) => {
            super::emit_compact(&AgentChanges::from(changes))
        }
        Some(TalonResponseData::Lint(lint)) => super::emit_compact(&lint::AgentLint::from(lint)),
        Some(TalonResponseData::Recall(recall)) => {
            super::emit_compact(&recall::AgentRecall::from(recall))
        }
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
struct AgentSearchResponse<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    vault: Option<&'a str>,
    results: Vec<AgentSearchHit<'a>>,
}

impl<'a> From<&'a talon_core::SearchResponse> for AgentSearchResponse<'a> {
    fn from(search: &'a talon_core::SearchResponse) -> Self {
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
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    is_index: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    citations: Vec<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    backlinks: Vec<&'a str>,
}

impl<'a> From<&'a SearchResult> for AgentSearchHit<'a> {
    fn from(result: &'a SearchResult) -> Self {
        Self {
            path: result.vault_path.as_str(),
            title: &result.title,
            snippet: &result.snippet,
            score: round_score(result.score),
            is_index: result.is_index,
            citations: result.citations.iter().map(String::as_str).collect(),
            backlinks: result.backlinks.iter().map(String::as_str).collect(),
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
    #[serde(skip_serializing_if = "Option::is_none")]
    vault_path: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_path: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    db_path: Option<&'a str>,
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
            vault_path: status.vault_path.as_deref(),
            config_path: status.config_path.as_deref(),
            db_path: status.db_path.as_deref(),
        }
    }
}

// ── Read ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct AgentReadResponse<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    vault: Option<&'a str>,
    results: Vec<AgentReadResult<'a>>,
}

impl<'a> From<&'a talon_core::ReadResponse> for AgentReadResponse<'a> {
    fn from(read: &'a talon_core::ReadResponse) -> Self {
        Self {
            vault: read.vault.as_ref().map(talon_core::ContainerPath::as_str),
            results: read.results.iter().map(AgentReadResult::from).collect(),
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

// ── Related ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct AgentRelatedResponse<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    vault: Option<&'a str>,
    results: Vec<AgentRelatedResult<'a>>,
}

impl<'a> From<&'a talon_core::RelatedResponse> for AgentRelatedResponse<'a> {
    fn from(related: &'a talon_core::RelatedResponse) -> Self {
        Self {
            vault: related
                .vault
                .as_ref()
                .map(talon_core::ContainerPath::as_str),
            results: related
                .results
                .iter()
                .map(AgentRelatedResult::from)
                .collect(),
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

// ── Meta ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentMeta<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    vault: Option<&'a str>,
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
            vault: meta.vault.as_ref().map(talon_core::ContainerPath::as_str),
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

// ── Changes ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct AgentChanges<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    vault: Option<&'a str>,
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
            vault: changes
                .vault
                .as_ref()
                .map(talon_core::ContainerPath::as_str),
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
