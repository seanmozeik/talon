//! Stdout emission for CLI responses.

use crate::exit_codes;
use eyre::Result;
use serde::Serialize;
use std::io::{self, Write};
use talon_core::{SearchResult, TalonResponse};

/// CLI output mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Full pretty JSON for humans during scaffolded development.
    JsonPretty,
    /// Compact token-efficient JSON for agents.
    Agent,
}

/// Writes bytes to stdout.
#[must_use]
pub fn write_stdout_bytes(bytes: &[u8]) -> u8 {
    match io::stdout().lock().write_all(bytes) {
        Ok(()) => exit_codes::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            exit_codes::GENERIC_ERROR
        }
    }
}

/// Emits a Talon response.
///
/// # Errors
///
/// Returns an error if serialization or stdout writes fail.
pub fn emit_response(response: &TalonResponse, mode: OutputMode) -> Result<()> {
    match mode {
        OutputMode::JsonPretty => emit_json_pretty(response),
        OutputMode::Agent => emit_agent(response),
    }
}

fn emit_json_pretty(response: &TalonResponse) -> Result<()> {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer_pretty(&mut lock, response)?;
    writeln!(lock)?;
    Ok(())
}

fn emit_agent(response: &TalonResponse) -> Result<()> {
    match response {
        TalonResponse::Search(search) => {
            let hits: Vec<AgentSearchHit<'_>> =
                search.results.iter().map(AgentSearchHit::from).collect();
            emit_json_compact(&hits)
        }
        other => emit_json_compact(other),
    }
}

fn emit_json_compact(value: &impl Serialize) -> Result<()> {
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
    score: f32,
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
