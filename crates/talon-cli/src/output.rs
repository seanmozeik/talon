//! Stdout emission for CLI responses.

use crate::exit_codes;
use anstyle::{AnsiColor, Effects, Style};
use eyre::Result;
use serde::Serialize;
use std::io::{self, Write};
use talon_core::{
    LintResponse, MetaResponse, ReadResponse, RecallResponse, RelatedResponse, SearchResult,
    SyncResponse, TalonEnvelope, TalonResponseData,
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

/// Options controlling human-readable rendering.
#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    /// Terminal column width used for wrapping.
    pub width: u16,
    /// Whether ANSI color codes should be emitted.
    pub colors: bool,
}

impl RenderOptions {
    /// Detects the current terminal width and color support.
    #[must_use]
    pub fn for_terminal() -> Self {
        use terminal_size::{Width, terminal_size};
        let width = terminal_size().map_or(80, |(Width(w), _)| w);
        Self {
            width,
            colors: crate::platform::stdout_is_tty() && crate::platform::user_accepts_ansi_color(),
        }
    }
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
    let opts = RenderOptions::for_terminal();
    match envelope.data.as_ref() {
        Some(TalonResponseData::Search(resp)) => {
            format_search_human(&mut io::stdout(), resp, opts)?;
        }
        Some(TalonResponseData::Sync(resp)) => format_sync_human(&mut io::stdout(), resp)?,
        Some(TalonResponseData::Status(resp)) => {
            format_status_human(&mut io::stdout(), resp)?;
        }
        Some(TalonResponseData::Read(resp)) => emit_read_human(resp)?,
        Some(TalonResponseData::Related(resp)) => emit_related_human(resp)?,
        Some(TalonResponseData::Meta(resp)) => emit_meta_human(resp)?,
        Some(TalonResponseData::Changes(resp)) => emit_changes_human(resp)?,
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

// ── helpers ───────────────────────────────────────────────────────────────────

/// Returns a style or the no-op style depending on whether colors are enabled.
const fn cs(colors: bool, s: Style) -> Style {
    if colors { s } else { Style::new() }
}

/// Word-wraps `text` into lines of at most `max_width` chars.
fn wrap_words(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 || text.len() <= max_width {
        return vec![text.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() {
            if current.len() + 1 + word.len() <= max_width {
                current.push(' ');
            } else {
                lines.push(current.clone());
                current.clear();
            }
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

// ── search ────────────────────────────────────────────────────────────────────

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

    // Line 1: rank + path (+ scope if set)
    let scope_suffix = r
        .scope
        .as_deref()
        .map_or_else(String::new, |s| format!("  ·  {s}"));
    writeln!(
        w,
        " {bold}{rank:>2}{bold:#}  {bold}{path}{bold:#}{dim}{scope_suffix}{dim:#}"
    )?;

    // Line 2: kind + score
    writeln!(w, "     {dim}{kind_str}  ·  {:.3}{dim:#}", r.score)?;

    // Line 3+: wrapped snippet
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

// ── sync ──────────────────────────────────────────────────────────────────────

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

// ── status ────────────────────────────────────────────────────────────────────

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

// ── lint ──────────────────────────────────────────────────────────────────────

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

// ── remaining human emitters (unchanged logic) ────────────────────────────────

fn emit_read_human(resp: &ReadResponse) -> Result<()> {
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

/// Human-readable formatter for recall responses.
///
/// # Errors
///
/// Returns an error if writing to `w` fails.
pub fn format_recall_human(
    w: &mut impl Write,
    resp: &RecallResponse,
    opts: RenderOptions,
) -> Result<()> {
    let head = cs(
        opts.colors,
        Style::new()
            .fg_color(Some(AnsiColor::Cyan.into()))
            .effects(Effects::BOLD),
    );
    let dim = cs(opts.colors, Style::new().effects(Effects::DIMMED));

    if resp.skipped {
        writeln!(
            w,
            "{}No recall context{} (evidence_score={:.2})",
            head.render(),
            head.render_reset(),
            resp.evidence_score
        )?;
        return Ok(());
    }

    writeln!(
        w,
        "{}Vault Recall{} evidence={:.2}  tokens={}",
        head.render(),
        head.render_reset(),
        resp.evidence_score,
        resp.tokens_used,
    )?;

    if let Some(vr) = &resp.vault_recall {
        recall_section_active_notes(w, &vr.active_notes, opts, &head, &dim)?;
        recall_section_linked(w, &vr.linked_context, &head, &dim)?;
        recall_section_recent_edits(w, &vr.recent_edits, &head, &dim)?;
        recall_section_fuzzy_anchors(w, &vr.fuzzy_anchors, &head)?;
    }

    if !resp.excluded_by_budget.is_empty() {
        writeln!(
            w,
            "\n{}Budget-trimmed:{} {} paths",
            dim.render(),
            dim.render_reset(),
            resp.excluded_by_budget.len()
        )?;
    }
    Ok(())
}

fn recall_section_active_notes(
    w: &mut impl Write,
    notes: &[talon_core::NoteExcerpt],
    opts: RenderOptions,
    head: &Style,
    dim: &Style,
) -> Result<()> {
    if notes.is_empty() {
        return Ok(());
    }
    writeln!(w, "\n{}Active Notes:{}", head.render(), head.render_reset())?;
    let max_width = opts.width.saturating_sub(4) as usize;
    for note in notes {
        writeln!(
            w,
            "  {}[{}]{} {} {:.3}",
            dim.render(),
            note.rank,
            dim.render_reset(),
            note.vault_path.as_str(),
            note.score
        )?;
        for line in note.snippet.trim().lines().take(3) {
            let display = if line.len() > max_width {
                format!("{}…", &line[..max_width.saturating_sub(1)])
            } else {
                line.to_string()
            };
            writeln!(w, "    {display}")?;
        }
    }
    Ok(())
}

fn recall_section_linked(
    w: &mut impl Write,
    linked: &[talon_core::LinkedNote],
    head: &Style,
    dim: &Style,
) -> Result<()> {
    if linked.is_empty() {
        return Ok(());
    }
    writeln!(
        w,
        "\n{}Linked Context:{} ({} notes)",
        head.render(),
        head.render_reset(),
        linked.len()
    )?;
    for l in linked {
        writeln!(
            w,
            "  {} {}({:?}){}",
            l.vault_path.as_str(),
            dim.render(),
            l.relation,
            dim.render_reset()
        )?;
    }
    Ok(())
}

fn recall_section_recent_edits(
    w: &mut impl Write,
    edits: &[talon_core::EditedNote],
    head: &Style,
    dim: &Style,
) -> Result<()> {
    if edits.is_empty() {
        return Ok(());
    }
    writeln!(
        w,
        "\n{}Recent Edits:{} ({} notes)",
        head.render(),
        head.render_reset(),
        edits.len()
    )?;
    for e in edits {
        writeln!(
            w,
            "  {} {}{:.0}d ago{}",
            e.vault_path.as_str(),
            dim.render(),
            e.days_since_modified,
            dim.render_reset()
        )?;
    }
    Ok(())
}

fn recall_section_fuzzy_anchors(
    w: &mut impl Write,
    anchors: &[talon_core::FuzzyAnchor],
    head: &Style,
) -> Result<()> {
    if anchors.is_empty() {
        return Ok(());
    }
    writeln!(
        w,
        "\n{}Fuzzy Anchors:{} ({} matches)",
        head.render(),
        head.render_reset(),
        anchors.len()
    )?;
    for a in anchors {
        writeln!(w, "  {} ({:.3})", a.vault_path.as_str(), a.match_score)?;
    }
    Ok(())
}

/// Renders a recall response as a `<vault_recall>` prompt-XML block.
///
/// When `resp.skipped == true`, emits a self-closing tag per spec.
///
/// # Errors
///
/// Returns an error if writing to `w` fails.
pub fn format_recall_prompt_xml(
    w: &mut impl Write,
    resp: &RecallResponse,
    vault: &str,
) -> Result<()> {
    if resp.skipped {
        writeln!(
            w,
            r#"<vault_recall skipped="true" evidence_score="{:.4}"/>"#,
            resp.evidence_score
        )?;
        return Ok(());
    }

    writeln!(
        w,
        r#"<vault_recall source="talon" vault="{vault}" evidence_score="{:.4}">"#,
        resp.evidence_score
    )?;

    if let Some(vr) = &resp.vault_recall {
        writeln!(w, "  <active_notes>")?;
        for note in &vr.active_notes {
            let snippet_escaped = xml_escape(&note.snippet);
            writeln!(
                w,
                r#"    <note path="{}" title="{}" score="{:.4}">{}</note>"#,
                xml_escape(note.vault_path.as_str()),
                xml_escape(&note.title),
                note.score,
                snippet_escaped
            )?;
        }
        writeln!(w, "  </active_notes>")?;

        writeln!(w, "  <linked_context>")?;
        for l in &vr.linked_context {
            writeln!(
                w,
                r#"    <note path="{}" title="{}" relation="{:?}" hops="{}"/>"#,
                xml_escape(l.vault_path.as_str()),
                xml_escape(&l.title),
                l.relation,
                l.hops
            )?;
        }
        writeln!(w, "  </linked_context>")?;

        writeln!(w, "  <frontmatter>")?;
        for f in &vr.frontmatter {
            writeln!(
                w,
                r#"    <fact path="{}" key="{}">{}</fact>"#,
                xml_escape(f.vault_path.as_str()),
                xml_escape(&f.key),
                xml_escape(&f.value.to_string())
            )?;
        }
        writeln!(w, "  </frontmatter>")?;

        writeln!(w, "  <recent_edits>")?;
        for e in &vr.recent_edits {
            writeln!(
                w,
                r#"    <note path="{}" title="{}" days_ago="{:.1}" score="{:.4}"/>"#,
                xml_escape(e.vault_path.as_str()),
                xml_escape(&e.title),
                e.days_since_modified,
                e.score
            )?;
        }
        writeln!(w, "  </recent_edits>")?;

        writeln!(w, "  <fuzzy_anchors>")?;
        for a in &vr.fuzzy_anchors {
            writeln!(
                w,
                r#"    <anchor path="{}" title="{}" score="{:.4}"/>"#,
                xml_escape(a.vault_path.as_str()),
                xml_escape(&a.title),
                a.match_score
            )?;
        }
        writeln!(w, "  </fuzzy_anchors>")?;
    }

    writeln!(w, "</vault_recall>")?;
    Ok(())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
