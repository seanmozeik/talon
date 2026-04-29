use crate::links::{NoteReference, resolve_wiki_link_target};
use crate::query::ReadSection;
use crate::text::{normalize_keyword, parse_wikilink, strip_heading_text};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) struct ReadTarget {
    pub(super) vault_path: String,
    pub(super) heading: Option<String>,
}

pub(super) struct SectionSlice {
    pub(super) content: String,
    pub(super) section: ReadSection,
}

pub(super) fn resolve_read_target(conn: &Connection, raw: &str) -> ReadTarget {
    let parsed = parse_read_reference(raw);
    let vault_path = resolve_note_path(conn, &parsed.target).unwrap_or(parsed.target);
    ReadTarget {
        vault_path,
        heading: parsed.heading,
    }
}

pub(super) fn find_heading_section(
    body: &str,
    vault_path: &str,
    requested_heading: &str,
) -> Option<SectionSlice> {
    let requested = normalize_keyword(requested_heading);
    let mut start_index = None;
    let mut start_level = 0_usize;
    let mut actual_heading = String::new();
    let lines: Vec<&str> = body.lines().collect();

    for (index, line) in lines.iter().enumerate() {
        let Some((level, heading)) = parse_heading_line(line) else {
            continue;
        };
        if normalize_keyword(&heading) == requested {
            start_index = Some(index);
            start_level = level;
            actual_heading = heading;
            break;
        }
    }

    let start = start_index?;
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(index, line)| {
            parse_heading_line(line)
                .filter(|(level, _heading)| *level <= start_level)
                .map(|_| index)
        })
        .unwrap_or(lines.len());

    let content = lines[start..end].join("\n");
    let from_line = u32::try_from(start + 1).unwrap_or(u32::MAX);
    let to_line = u32::try_from(end).unwrap_or(u32::MAX);
    let section = ReadSection {
        obsidian_ref: format!("[[{vault_path}#{actual_heading}]]"),
        heading: actual_heading,
        from_line,
        to_line,
    };

    Some(SectionSlice { content, section })
}

struct ParsedReadReference {
    target: String,
    heading: Option<String>,
}

fn parse_read_reference(raw: &str) -> ParsedReadReference {
    let trimmed = raw.trim();
    let without_embed = trimmed.strip_prefix('!').unwrap_or(trimmed);
    let inner = without_embed
        .strip_prefix("[[")
        .and_then(|value| value.strip_suffix("]]"))
        .unwrap_or(without_embed);
    let parsed = parse_wikilink(inner);
    ParsedReadReference {
        target: parsed.target,
        heading: parsed.heading,
    }
}

fn resolve_note_path(conn: &Connection, target: &str) -> Option<String> {
    resolve_exact_path(conn, target)
        .or_else(|| resolve_exact_path(conn, &with_markdown_extension(target)))
        .or_else(|| resolve_against_note_references(conn, target))
}

fn resolve_exact_path(conn: &Connection, target: &str) -> Option<String> {
    conn.query_row(
        "SELECT vault_path FROM notes WHERE active = 1 AND vault_path = ?1",
        params![target],
        |row| row.get(0),
    )
    .optional()
    .ok()
    .flatten()
}

fn resolve_against_note_references(conn: &Connection, target: &str) -> Option<String> {
    let notes = query_note_references(conn);
    resolve_wiki_link_target(target, &notes)
}

fn query_note_references(conn: &Connection) -> Vec<NoteReference> {
    let Ok(mut stmt) =
        conn.prepare("SELECT vault_path, title, aliases FROM notes WHERE active = 1 ORDER BY id")
    else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| {
        let aliases_raw: Option<String> = row.get(2)?;
        let aliases = aliases_raw
            .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
            .unwrap_or_default();
        Ok(NoteReference {
            vault_path: row.get(0)?,
            title: row.get(1)?,
            aliases,
        })
    }) else {
        return Vec::new();
    };
    rows.flatten().collect()
}

fn with_markdown_extension(value: &str) -> String {
    if std::path::Path::new(value)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
    {
        value.to_string()
    } else {
        format!("{value}.md")
    }
}

fn parse_heading_line(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = trimmed.get(level..)?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let heading = strip_heading_text(trimmed);
    if heading.is_empty() {
        return None;
    }
    Some((level, heading))
}
