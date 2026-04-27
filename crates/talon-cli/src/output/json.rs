use eyre::Result;
use serde::Serialize;
use std::io::{self, Write};
use talon_core::{SearchResult, TalonEnvelope, TalonResponseData};

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
            score: result.score,
        }
    }
}
