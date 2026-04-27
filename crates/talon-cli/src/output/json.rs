use eyre::Result;
use serde::Serialize;
use std::io::{self, Write};
use talon_core::{SearchResult, SyncResponse, TalonEnvelope, TalonResponseData};

pub(super) fn emit_pretty(envelope: &TalonEnvelope) -> Result<()> {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer_pretty(&mut lock, envelope)?;
    writeln!(lock)?;
    Ok(())
}

pub(super) fn emit_agent(envelope: &TalonEnvelope) -> Result<()> {
    match envelope.data.as_ref() {
        Some(TalonResponseData::Search(search)) => {
            let hits: Vec<AgentSearchHit<'_>> =
                search.results.iter().map(AgentSearchHit::from).collect();
            emit_compact(&hits)
        }
        Some(TalonResponseData::Sync(sync)) => emit_compact(&AgentSync::from(sync)),
        _ => emit_compact(envelope),
    }
}

fn emit_compact(value: &impl Serialize) -> Result<()> {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer(&mut lock, value)?;
    writeln!(lock)?;
    Ok(())
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
