use super::RenderOptions;
use super::style::cs;
use anstyle::{AnsiColor, Effects, Style};
use eyre::Result;
use std::io::Write;
use talon_core::RecallResponse;

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
        let mtime_suffix = if note.mtime.is_empty() {
            String::new()
        } else {
            format!("  ({})", note.mtime)
        };
        writeln!(
            w,
            "  {}[{}]{} {}{} {:.3}",
            dim.render(),
            note.rank,
            dim.render_reset(),
            note.vault_path.as_str(),
            mtime_suffix,
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
        writeln!(w, r#"<vault_recall skipped="true"/>"#)?;
        return Ok(());
    }

    writeln!(w, r#"<vault_recall source="talon" vault="{vault}">"#)?;

    if let Some(vr) = &resp.vault_recall {
        writeln!(w, "  <active_notes>")?;
        for note in &vr.active_notes {
            writeln!(
                w,
                r#"    <note path="{}" title="{}" mtime="{}" score="{:.2}">{}</note>"#,
                xml_escape(note.vault_path.as_str()),
                xml_escape(&note.title),
                xml_escape(&note.mtime),
                note.score,
                xml_escape(&note.snippet),
            )?;
        }
        writeln!(w, "  </active_notes>")?;

        if !vr.linked_context.is_empty() {
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
        }
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
