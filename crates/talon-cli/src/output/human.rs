use super::RenderOptions;
use super::ask::format_ask_human;
use super::obsidian::format_ref;
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
            let warnings = envelope
                .meta
                .as_ref()
                .map_or(&[][..], |m| m.warnings.as_slice());
            format_search_human(&mut io::stdout(), resp, opts, warnings)?;
        }
        Some(TalonResponseData::Ask(resp)) => {
            let warnings = envelope
                .meta
                .as_ref()
                .map_or(&[][..], |m| m.warnings.as_slice());
            format_ask_human(&mut io::stdout(), resp, opts, warnings)?;
        }
        Some(TalonResponseData::Sync(resp)) => format_sync_human(&mut io::stdout(), resp)?,
        Some(TalonResponseData::Status(resp)) => {
            format_status_human(&mut io::stdout(), resp)?;
        }
        Some(TalonResponseData::Read(resp)) => emit_read(resp, opts)?,
        Some(TalonResponseData::Related(resp)) => emit_related(resp, opts)?,
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
    if let Some(vault) = &resp.vault_path {
        writeln!(w, "  Vault:  {vault}")?;
    }
    if let Some(config) = &resp.config_path {
        writeln!(w, "  Config: {config}")?;
    }
    if let Some(db) = &resp.db_path {
        writeln!(w, "  Index:  {db}")?;
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
/// Mirrors `format_search_human`'s card style: a styled headline summarising
/// the run, then per-check sections with numbered findings (rank + path on
/// one line, indented detail beneath).
///
/// # Errors
///
/// Returns an error if writing to `w` fails.
pub fn format_lint_human(w: &mut impl Write, resp: &LintResponse) -> Result<()> {
    use anstyle::{AnsiColor, Effects, Style};
    use talon_core::LintCheck;

    let opts = super::RenderOptions::for_terminal();
    let heading = super::style::cs(
        opts.colors,
        Style::new().bold().fg_color(Some(AnsiColor::Cyan.into())),
    );
    let bold = super::style::cs(opts.colors, Style::new().effects(Effects::BOLD));
    let dim = super::style::cs(opts.colors, Style::new().effects(Effects::DIMMED));

    let total = resp.findings.len();
    let finding_word = if total == 1 { "finding" } else { "findings" };
    writeln!(
        w,
        "{heading}Lint{heading:#}  ·  {bold}{}{bold:#}  ·  {dim}{total} {finding_word}{dim:#}",
        lint_label(resp.check)
    )?;

    if total == 0 {
        writeln!(w)?;
        writeln!(w, "  {dim}No issues found.{dim:#}")?;
        return Ok(());
    }

    writeln!(w)?;
    if resp.check == LintCheck::All {
        let mut first_section = true;
        for check in [
            LintCheck::Orphans,
            LintCheck::BrokenLinks,
            LintCheck::DanglingRefs,
            LintCheck::Unreferenced,
        ] {
            let findings: Vec<_> = resp.findings.iter().filter(|f| f.check == check).collect();
            if findings.is_empty() {
                continue;
            }
            if !first_section {
                writeln!(w)?;
            }
            first_section = false;
            writeln!(
                w,
                "{bold}{}{bold:#}  ·  {dim}{}{dim:#}",
                lint_label(check),
                findings.len()
            )?;
            for (i, f) in findings.iter().take(20).enumerate() {
                format_lint_card(w, i + 1, f, &bold, &dim)?;
            }
        }
    } else {
        for (i, f) in resp.findings.iter().take(20).enumerate() {
            format_lint_card(w, i + 1, f, &bold, &dim)?;
        }
    }
    Ok(())
}

const fn lint_label(check: talon_core::LintCheck) -> &'static str {
    match check {
        talon_core::LintCheck::All => "all",
        talon_core::LintCheck::Orphans => "orphans",
        talon_core::LintCheck::BrokenLinks => "broken-links",
        talon_core::LintCheck::DanglingRefs => "dangling-refs",
        talon_core::LintCheck::Unreferenced => "unreferenced",
    }
}

fn format_lint_card(
    w: &mut impl Write,
    rank: usize,
    f: &talon_core::LintFinding,
    bold: &anstyle::Style,
    dim: &anstyle::Style,
) -> Result<()> {
    let path = f.path.as_str();
    if let Some(line) = f.line {
        writeln!(
            w,
            " {bold}{rank:>2}{bold:#}  {bold}{path}{bold:#}{dim}:{line}{dim:#}"
        )?;
    } else {
        writeln!(w, " {bold}{rank:>2}{bold:#}  {bold}{path}{bold:#}")?;
    }
    let detail = strip_redundant_prefix(f.check, &f.message);
    writeln!(w, "     {dim}{detail}{dim:#}")?;
    Ok(())
}

/// Drops the leading `"<check>: "` prefix from a finding message when the
/// section header already conveys it. Keeps the prefix for `--all` callers
/// who consume the message without the section context.
fn strip_redundant_prefix(check: talon_core::LintCheck, msg: &str) -> &str {
    let prefix = match check {
        talon_core::LintCheck::BrokenLinks => "broken link: ",
        talon_core::LintCheck::DanglingRefs => "dangling ref: ",
        _ => return msg,
    };
    msg.strip_prefix(prefix).unwrap_or(msg)
}

fn emit_read(resp: &ReadResponse, opts: RenderOptions) -> Result<()> {
    let vault = resp.vault.as_ref().map(talon_core::ContainerPath::as_str);
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
        let path_ref = format_ref(
            vault,
            result.vault_path.as_str(),
            result.title.as_deref(),
            result
                .section
                .as_ref()
                .map(|section| section.heading.as_str()),
            opts.colors,
        );
        writeln!(io::stdout(), "Path: {path_ref}")?;
        if let Some(section) = result.section.as_ref() {
            writeln!(
                io::stdout(),
                "Section: {} (lines {}-{})",
                section.obsidian_ref,
                section.from_line,
                section.to_line
            )?;
        }
        if !result.tags.is_empty() {
            writeln!(io::stdout(), "Tags: {}", result.tags.join(", "))?;
        }
        if !result.links.is_empty() {
            writeln!(
                io::stdout(),
                "Links: {}",
                format_path_list(vault, &result.links, opts)
            )?;
        }
        if !result.backlinks.is_empty() {
            writeln!(
                io::stdout(),
                "Backlinks: {}",
                format_path_list(vault, &result.backlinks, opts)
            )?;
        }
        writeln!(io::stdout())?;
        if let Some(content) = &result.content {
            writeln!(io::stdout(), "{content}")?;
        }
    }
    Ok(())
}

fn emit_related(resp: &RelatedResponse, opts: RenderOptions) -> Result<()> {
    let vault = resp.vault.as_ref().map(talon_core::ContainerPath::as_str);
    writeln!(
        io::stdout(),
        "Related to: {}",
        format_ref(vault, resp.path.as_str(), None, None, opts.colors)
    )?;
    for r in &resp.results {
        writeln!(
            io::stdout(),
            "  - {} ({:?})",
            format_ref(
                vault,
                r.vault_path.as_str(),
                Some(&r.title),
                None,
                opts.colors
            ),
            r.relation
        )?;
    }
    Ok(())
}

fn format_path_list(vault: Option<&str>, paths: &[String], opts: RenderOptions) -> String {
    paths
        .iter()
        .map(|path| format_ref(vault, path, None, None, opts.colors))
        .collect::<Vec<_>>()
        .join(", ")
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
