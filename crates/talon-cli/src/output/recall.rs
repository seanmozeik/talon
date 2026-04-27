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
