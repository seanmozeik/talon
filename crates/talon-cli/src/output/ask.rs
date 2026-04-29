use super::RenderOptions;
use super::style::{cs, wrap_prefixed_words, wrap_words};
use anstyle::{AnsiColor, Effects, Style};
use eyre::Result;
use std::io::Write;

/// Formats an ask response for human reading.
///
/// # Errors
///
/// Returns an error if writing to `w` fails.
pub fn format_ask_human(
    w: &mut impl Write,
    resp: &talon_core::AskResponse,
    opts: RenderOptions,
    warnings: &[String],
) -> Result<()> {
    let heading = cs(
        opts.colors,
        Style::new().bold().fg_color(Some(AnsiColor::Cyan.into())),
    );
    let bold = cs(opts.colors, Style::new().effects(Effects::BOLD));
    let dim = cs(opts.colors, Style::new().effects(Effects::DIMMED));

    writeln!(
        w,
        "{heading}Ask{heading:#}  {bold}\"{}\"{bold:#}",
        resp.question
    )?;
    for warning in warnings {
        writeln!(w, "  {dim}! {warning}{dim:#}")?;
    }
    if !resp.queries.is_empty() {
        let width = (opts.width as usize).saturating_sub(2);
        for line in wrap_prefixed_words("queries: ", &resp.queries.join("  ·  "), width) {
            writeln!(w, "  {dim}{line}{dim:#}")?;
        }
    }
    if let Some(diagnostics) = resp.diagnostics.as_ref() {
        writeln!(
            w,
            "  {dim}ask model: {} at {}{dim:#}",
            diagnostics.model, diagnostics.endpoint
        )?;
        writeln!(
            w,
            "  {dim}stages: planning {}ms  ·  search {}ms ({} results){}{dim:#}",
            diagnostics.planning.duration_ms,
            diagnostics.search.duration_ms,
            diagnostics.search.total,
            diagnostics
                .synthesis
                .as_ref()
                .map_or_else(String::new, |s| format!(
                    "  ·  synthesis {}ms",
                    s.duration_ms
                ))
        )?;
        writeln!(w, "  {dim}planner content:{dim:#}")?;
        for line in diagnostics.planning.content.lines() {
            writeln!(w, "    {dim}{line}{dim:#}")?;
        }
    }
    writeln!(w)?;

    for line in wrap_words(&resp.answer, opts.width as usize) {
        writeln!(w, "{line}")?;
    }

    if resp.sources.is_empty() {
        return Ok(());
    }

    writeln!(w)?;
    writeln!(w, "{bold}Sources{bold:#}")?;
    let mut sources: Vec<(&str, f64)> = Vec::new();
    for source in &resp.sources {
        let path = source.vault_path.as_str();
        if let Some((_, score)) = sources.iter_mut().find(|(seen, _)| *seen == path) {
            *score = score.max(source.score);
        } else {
            sources.push((path, source.score));
        }
    }
    for (index, (path, score)) in sources.iter().enumerate() {
        writeln!(w, "{dim} {:>2}  {path}  {score:.3}{dim:#}", index + 1)?;
    }
    Ok(())
}
