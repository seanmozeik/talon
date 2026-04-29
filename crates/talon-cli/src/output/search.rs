use super::RenderOptions;
use super::obsidian::title_ref;
use super::style::{cs, wrap_prefixed_words, wrap_words};
use anstyle::{AnsiColor, Effects, Style};
use eyre::Result;
use std::io::Write;
use talon_core::{SearchDiagnostics, SearchResult};

/// Formats search results as cards for human reading.
///
/// `warnings` are printed as dim notices at the top of the output (e.g. "sync skipped").
///
/// # Errors
///
/// Returns an error if writing to `w` fails.
pub fn format_search_human(
    w: &mut impl Write,
    resp: &talon_core::SearchResponse,
    opts: RenderOptions,
    warnings: &[String],
) -> Result<()> {
    let heading = cs(
        opts.colors,
        Style::new().bold().fg_color(Some(AnsiColor::Cyan.into())),
    );
    let bold = cs(opts.colors, Style::new().effects(Effects::BOLD));
    let dim = cs(opts.colors, Style::new().effects(Effects::DIMMED));

    for msg in warnings {
        writeln!(w, "{dim}~ {msg}{dim:#}")?;
    }

    let q = resp.query.as_deref().unwrap_or("(empty)");
    let mode_str = format!("{:?}", resp.mode).to_lowercase();
    let mut meta_parts: Vec<String> = vec![mode_str];
    if resp.fast {
        meta_parts.push("fast".to_string());
    }
    if resp.expanded {
        meta_parts.push("expanded".to_string());
    }
    if resp.reranked {
        meta_parts.push("reranked".to_string());
    }
    let result_word = if resp.total == 1 { "result" } else { "results" };
    let meta = meta_parts.join("  ·  ");

    writeln!(
        w,
        "{heading}Search{heading:#}  {bold}\"{q}\"{bold:#}  ·  {dim}{meta}{dim:#}  ·  {} {result_word}",
        resp.total
    )?;
    if !resp.expanded_queries.is_empty() {
        let width = (opts.width as usize).saturating_sub(2);
        for line in wrap_prefixed_words("expanded: ", &resp.expanded_queries.join("  ·  "), width)
        {
            writeln!(w, "  {dim}{line}{dim:#}")?;
        }
    }
    if let Some(diag) = resp.diagnostics.as_ref()
        && let Some(line) = format_diagnostics_line(diag)
    {
        let width = (opts.width as usize).saturating_sub(2);
        for line in wrap_words(&line, width) {
            writeln!(w, "  {dim}{line}{dim:#}")?;
        }
    }

    if resp.results.is_empty() {
        writeln!(w)?;
        writeln!(w, "  {dim}No results found.{dim:#}")?;
        return Ok(());
    }

    writeln!(w)?;
    for (i, r) in resp.results.iter().enumerate() {
        format_search_card(
            w,
            i + 1,
            r,
            opts,
            resp.vault.as_ref().map(talon_core::ContainerPath::as_str),
            &bold,
            &dim,
        )?;
    }
    Ok(())
}

fn format_diagnostics_line(diag: &SearchDiagnostics) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(score) = diag.strong_signal_score {
        parts.push(format!("strong-signal {score:.2} (skipped expansion)"));
    } else if let Some(ms) = diag.expansion_ms {
        parts.push(format!("expansion {ms}ms"));
    }
    if let (Some(count), Some(ms)) = (diag.rerank_candidates, diag.rerank_ms) {
        parts.push(format!("rerank {count}c {ms}ms"));
    } else if let Some(ms) = diag.rerank_ms {
        parts.push(format!("rerank {ms}ms"));
    } else if let Some(count) = diag.rerank_candidates {
        parts.push(format!("rerank {count}c"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("stages: {}", parts.join("  ·  ")))
    }
}

fn format_search_card(
    w: &mut impl Write,
    rank: usize,
    r: &SearchResult,
    opts: RenderOptions,
    vault: Option<&str>,
    bold: &Style,
    dim: &Style,
) -> Result<()> {
    let path = r.vault_path.as_str();
    let title_link = title_ref(vault, path, &r.title, opts.colors);

    writeln!(w, " {bold}{rank:>2}{bold:#}  {bold}{title_link}{bold:#}")?;

    if opts.compact {
        let scope_part = r
            .scope
            .as_deref()
            .map_or_else(String::new, |s| format!("  ·  {s}"));
        writeln!(w, "     {dim}{path}{scope_part}  ·  {:.3}{dim:#}", r.score)?;
        writeln!(w)?;
        return Ok(());
    }

    let kind_str = match r.match_kind {
        talon_core::MatchKind::Fulltext => "fulltext",
        talon_core::MatchKind::Semantic => "semantic",
        talon_core::MatchKind::Title => "title",
        talon_core::MatchKind::Alias => "alias",
        talon_core::MatchKind::Related => "related",
    };
    let scope_suffix = r
        .scope
        .as_deref()
        .map_or_else(String::new, |s| format!("  ·  {s}"));

    writeln!(w, "     {dim}{path}{dim:#}")?;
    writeln!(
        w,
        "     {dim}{kind_str}  ·  {:.3}{scope_suffix}{dim:#}",
        r.score
    )?;

    let indent = "     ";
    let available = (opts.width as usize).saturating_sub(indent.len());
    if !r.snippet.is_empty() {
        for line in wrap_words(&r.snippet, available) {
            writeln!(w, "{indent}{line}")?;
        }
    }
    writeln!(w)?;
    Ok(())
}
