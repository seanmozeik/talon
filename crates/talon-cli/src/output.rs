//! Stdout emission for CLI responses.

use crate::exit_codes;
use eyre::Result;
use serde::Serialize;
use std::io::{self, Write};
use talon_core::{
    LintResponse, MetaResponse, RelatedResponse, SearchResult, SyncResponse, TalonEnvelope,
    TalonResponseData,
};

/// CLI output mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Human-readable formatted output (colored headings, result cards).
    Human,
    /// Full pretty JSON for debugging.
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

/// Emits a Talon envelope.
///
/// # Errors
///
/// Returns an error if serialization or stdout writes fail.
pub fn emit_response(envelope: &TalonEnvelope, mode: OutputMode) -> Result<()> {
    match mode {
        OutputMode::Human => emit_human(envelope),
        OutputMode::JsonPretty => emit_json_pretty(envelope),
        OutputMode::Agent => emit_agent(envelope),
    }
}

fn emit_human(envelope: &TalonEnvelope) -> Result<()> {
    match envelope.data.as_ref() {
        Some(TalonResponseData::Search(resp)) => emit_search_human(resp)?,
        Some(TalonResponseData::Sync(resp)) => emit_sync_human(resp)?,
        Some(TalonResponseData::Status(resp)) => emit_status_human(resp)?,
        Some(TalonResponseData::Read(_)) => emit_read_human()?,
        Some(TalonResponseData::Related(resp)) => emit_related_human(resp)?,
        Some(TalonResponseData::Meta(resp)) => emit_meta_human(resp)?,
        Some(TalonResponseData::Changes(resp)) => emit_changes_human(resp)?,
        Some(TalonResponseData::Lint(resp)) => emit_lint_human(resp)?,
        None => {
            if let Some(err) = &envelope.error {
                writeln!(io::stderr(), "Error [{}]: {}", err.code, err.message)?;
            }
        }
    }
    Ok(())
}

fn emit_search_human(resp: &talon_core::SearchResponse) -> Result<()> {
    let q = resp.query.as_deref().unwrap_or("(empty)");
    writeln!(io::stdout(), "Search: {q}")?;
    writeln!(
        io::stdout(),
        "Mode: {:?}  Fast: {}  Reranked: {}",
        resp.mode,
        resp.fast,
        resp.reranked
    )?;
    writeln!(io::stdout(), "Results: {}", resp.total)?;
    writeln!(io::stdout())?;
    for (i, r) in resp.results.iter().enumerate() {
        writeln!(
            io::stdout(),
            "  {}. {} (score: {:.3})",
            i + 1,
            r.vault_path.as_str(),
            r.score
        )?;
        if !r.snippet.is_empty() {
            writeln!(io::stdout(), "     {}", r.snippet)?;
        }
    }
    Ok(())
}

fn emit_sync_human(resp: &SyncResponse) -> Result<()> {
    writeln!(
        io::stdout(),
        "Sync: {} ({} indexed, {} skipped, {} deleted) in {}ms",
        if resp.completed { "OK" } else { "partial" },
        resp.indexed,
        resp.skipped,
        resp.deleted,
        resp.duration_ms
    )?;
    if !resp.fast {
        let embed_label = if resp.dimension_mismatch {
            "dimension mismatch"
        } else if resp.embed_failed > 0 {
            "partial"
        } else {
            "OK"
        };
        writeln!(
            io::stdout(),
            "Embed: {embed_label} ({}/{} succeeded, {} failed)",
            resp.embedded,
            resp.embedded + resp.embed_failed,
            resp.embed_failed
        )?;
        if let Some(remediation) = resp.embed_remediation.as_deref() {
            writeln!(io::stdout(), "  ! {remediation}")?;
        }
        for line in resp.embed_diagnostics.iter().take(5) {
            writeln!(io::stdout(), "  - {line}")?;
        }
    }
    Ok(())
}

fn emit_status_human(resp: &talon_core::StatusResponse) -> Result<()> {
    writeln!(io::stdout(), "Status: {:?}", resp.state)?;
    if let Some(reason) = &resp.reason {
        writeln!(io::stdout(), "  Reason: {reason}")?;
    }
    writeln!(
        io::stdout(),
        "  Notes: {}  Chunks: {}",
        resp.index.active_notes,
        resp.index.chunk_count
    )?;
    Ok(())
}

fn emit_read_human() -> Result<()> {
    writeln!(io::stdout(), "Read: complete")?;
    Ok(())
}

fn emit_related_human(resp: &RelatedResponse) -> Result<()> {
    writeln!(io::stdout(), "Related to: {}", resp.path.as_str())?;
    for r in &resp.results {
        writeln!(
            io::stdout(),
            "  - {} ({:?})",
            r.vault_path.as_str(),
            r.relation
        )?;
    }
    Ok(())
}

fn emit_meta_human(resp: &MetaResponse) -> Result<()> {
    writeln!(io::stdout(), "Frontmatter: {} entries", resp.entries.len())?;
    if let Some(counts) = &resp.tag_counts {
        writeln!(io::stdout(), "Tags: {}", counts.len())?;
        for (tag, count) in counts.iter().take(10) {
            writeln!(io::stdout(), "  {tag}: {count}")?;
        }
    }
    for e in resp.entries.iter().take(10) {
        writeln!(io::stdout(), "  - {}", e.path.as_str())?;
    }
    Ok(())
}

fn emit_changes_human(resp: &talon_core::ChangesResponse) -> Result<()> {
    writeln!(
        io::stdout(),
        "Changes: {} added, {} modified, {} deleted",
        resp.added.len(),
        resp.modified.len(),
        resp.deleted.len()
    )?;
    Ok(())
}

fn emit_lint_human(resp: &LintResponse) -> Result<()> {
    writeln!(
        io::stdout(),
        "Lint ({:?}): {} findings",
        resp.check,
        resp.findings.len()
    )?;
    for f in resp.findings.iter().take(20) {
        if let Some(line) = f.line {
            writeln!(
                io::stdout(),
                "  - {}:{} {}",
                f.path.as_str(),
                line,
                f.message
            )?;
        } else {
            writeln!(io::stdout(), "  - {} {}", f.path.as_str(), f.message)?;
        }
    }
    Ok(())
}

fn emit_json_pretty(envelope: &TalonEnvelope) -> Result<()> {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer_pretty(&mut lock, envelope)?;
    writeln!(lock)?;
    Ok(())
}

fn emit_agent(envelope: &TalonEnvelope) -> Result<()> {
    match envelope.data.as_ref() {
        Some(TalonResponseData::Search(search)) => {
            let hits: Vec<AgentSearchHit<'_>> =
                search.results.iter().map(AgentSearchHit::from).collect();
            emit_json_compact(&hits)
        }
        _ => emit_json_compact(envelope),
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
