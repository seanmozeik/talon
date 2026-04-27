use super::RenderOptions;
use super::recall::format_recall_human;
use super::search::format_search_human;
use eyre::Result;
use std::io::{self, Write};
use talon_core::{
    LintResponse, MetaResponse, ReadResponse, RelatedResponse, SyncResponse, TalonEnvelope,
    TalonResponseData,
};

pub(super) fn emit(envelope: &TalonEnvelope) -> Result<()> {
    let opts = RenderOptions::for_terminal();
    match envelope.data.as_ref() {
        Some(TalonResponseData::Search(resp)) => {
            format_search_human(&mut io::stdout(), resp, opts)?;
        }
        Some(TalonResponseData::Sync(resp)) => format_sync_human(&mut io::stdout(), resp)?,
        Some(TalonResponseData::Status(resp)) => {
            format_status_human(&mut io::stdout(), resp)?;
        }
        Some(TalonResponseData::Read(resp)) => emit_read(resp)?,
        Some(TalonResponseData::Related(resp)) => emit_related(resp)?,
        Some(TalonResponseData::Meta(resp)) => emit_meta(resp)?,
        Some(TalonResponseData::Changes(resp)) => emit_changes(resp)?,
        Some(TalonResponseData::Lint(resp)) => format_lint_human(&mut io::stdout(), resp)?,
        Some(TalonResponseData::Recall(resp)) => {
            format_recall_human(&mut io::stdout(), resp, opts)?;
        }
        None => {
            if let Some(err) = &envelope.error {
                writeln!(io::stderr(), "Error [{}]: {}", err.code, err.message)?;
            }
        }
    }
    Ok(())
}

/// Formats a sync response for human reading.
///
/// # Errors
///
/// Returns an error if writing to `w` fails.
pub fn format_sync_human(w: &mut impl Write, resp: &SyncResponse) -> Result<()> {
    writeln!(
        w,
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
            w,
            "Embed: {embed_label} ({}/{} succeeded, {} failed)",
            resp.embedded,
            resp.embedded + resp.embed_failed,
            resp.embed_failed
        )?;
        if let Some(remediation) = resp.embed_remediation.as_deref() {
            writeln!(w, "  ! {remediation}")?;
        }
        for line in resp.embed_diagnostics.iter().take(5) {
            writeln!(w, "  - {line}")?;
        }
    }
    Ok(())
}

/// Formats a status response for human reading.
///
/// # Errors
///
/// Returns an error if writing to `w` fails.
pub fn format_status_human(w: &mut impl Write, resp: &talon_core::StatusResponse) -> Result<()> {
    writeln!(w, "Status: {:?}", resp.state)?;
    if let Some(reason) = &resp.reason {
        writeln!(w, "  Reason: {reason}")?;
    }
    writeln!(
        w,
        "  Notes: {}  Chunks: {}  Failed: {}",
        resp.index.active_notes, resp.index.chunk_count, resp.index.failed_embeddings,
    )?;
    if let Some(dims) = resp.index.vector_dimensions {
        writeln!(w, "  Dimensions: {dims}")?;
    }
    Ok(())
}

/// Formats a lint response for human reading.
///
/// # Errors
///
/// Returns an error if writing to `w` fails.
pub fn format_lint_human(w: &mut impl Write, resp: &LintResponse) -> Result<()> {
    writeln!(
        w,
        "Lint ({:?}): {} findings",
        resp.check,
        resp.findings.len()
    )?;
    for f in resp.findings.iter().take(20) {
        if let Some(line) = f.line {
            writeln!(w, "  - {}:{} {}", f.path.as_str(), line, f.message)?;
        } else {
            writeln!(w, "  - {} {}", f.path.as_str(), f.message)?;
        }
    }
    Ok(())
}

fn emit_read(resp: &ReadResponse) -> Result<()> {
    for result in &resp.results {
        if !result.found {
            writeln!(io::stdout(), "Not found: {}", result.vault_path.as_str())?;
            continue;
        }
        let title = result
            .title
            .as_deref()
            .unwrap_or(result.vault_path.as_str());
        writeln!(io::stdout(), "# {title}")?;
        writeln!(io::stdout(), "Path: {}", result.vault_path.as_str())?;
        if !result.tags.is_empty() {
            writeln!(io::stdout(), "Tags: {}", result.tags.join(", "))?;
        }
        if !result.links.is_empty() {
            writeln!(io::stdout(), "Links: {}", result.links.join(", "))?;
        }
        if !result.backlinks.is_empty() {
            writeln!(io::stdout(), "Backlinks: {}", result.backlinks.join(", "))?;
        }
        writeln!(io::stdout())?;
        if let Some(content) = &result.content {
            writeln!(io::stdout(), "{content}")?;
        }
    }
    Ok(())
}

fn emit_related(resp: &RelatedResponse) -> Result<()> {
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

fn emit_meta(resp: &MetaResponse) -> Result<()> {
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

fn emit_changes(resp: &talon_core::ChangesResponse) -> Result<()> {
    writeln!(
        io::stdout(),
        "Changes: {} added, {} modified, {} deleted",
        resp.added.len(),
        resp.modified.len(),
        resp.deleted.len()
    )?;
    Ok(())
}
