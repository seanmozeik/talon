use super::RenderOptions;
use super::style::{cs, wrap_words};
use anstyle::{AnsiColor, Effects, Style};
use eyre::Result;
use std::io::Write;
use talon_core::SearchResult;

/// Formats search results as compact cards for human reading.
///
/// # Errors
///
/// Returns an error if writing to `w` fails.
pub fn format_search_human(
    w: &mut impl Write,
    resp: &talon_core::SearchResponse,
    opts: RenderOptions,
) -> Result<()> {
    let heading = cs(
        opts.colors,
        Style::new().bold().fg_color(Some(AnsiColor::Cyan.into())),
    );
    let bold = cs(opts.colors, Style::new().effects(Effects::BOLD));
    let dim = cs(opts.colors, Style::new().effects(Effects::DIMMED));

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
        writeln!(
            w,
            "  {dim}expanded: {}{dim:#}",
            resp.expanded_queries.join("  ·  ")
        )?;
    }

    if resp.results.is_empty() {
        writeln!(w)?;
        writeln!(w, "  {dim}No results found.{dim:#}")?;
        return Ok(());
    }

    writeln!(w)?;
    for (i, r) in resp.results.iter().enumerate() {
        format_search_card(w, i + 1, r, opts, &bold, &dim)?;
    }
    Ok(())
}

fn format_search_card(
    w: &mut impl Write,
    rank: usize,
    r: &SearchResult,
    opts: RenderOptions,
    bold: &Style,
    dim: &Style,
) -> Result<()> {
    let path = r.vault_path.as_str();
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
    writeln!(
        w,
        " {bold}{rank:>2}{bold:#}  {bold}{path}{bold:#}{dim}{scope_suffix}{dim:#}"
    )?;

    writeln!(w, "     {dim}{kind_str}  ·  {:.3}{dim:#}", r.score)?;

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
