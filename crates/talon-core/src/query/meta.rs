//! Meta (frontmatter query) handler for the Talon CLI.
//!
//! Queries active notes by frontmatter `--where` clauses, projects `--select`
//! fields, aggregates `--tag-counts`, and resolves `--sources` reverse
//! references — all against the `SQLite` index.

use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::config::{ScopeFilter, TalonConfig};
use crate::contracts::VaultPath;
use crate::query::{MetaEntry, MetaInput, MetaResponse};
use crate::text::frontmatter::FrontmatterValue;

struct NoteRow {
    note_id: i64,
    vault_path: String,
    frontmatter_json: String,
    mtime_ms: i64,
}

/// Queries active notes by the filters in `input` and returns a [`MetaResponse`].
pub fn query_meta(
    conn: &Connection,
    input: &MetaInput,
    config: Option<&TalonConfig>,
) -> MetaResponse {
    let tag_counts = if input.tag_counts {
        Some(query_tag_counts(conn))
    } else {
        None
    };

    let mut notes = input.sources.as_ref().map_or_else(
        || query_all_active_notes(conn),
        |target| query_notes_by_sources(conn, target),
    );

    if let Some(ref since_str) = input.since
        && let Ok(ts) = crate::indexing::change_tracking::parse_since(since_str)
    {
        notes.retain(|n| n.mtime_ms >= ts.cast_signed());
    }

    if !input.where_.is_empty() {
        notes.retain(|n| super::where_filter::passes_where_clauses(conn, n.note_id, &input.where_));
    }

    let filter = config.map(|cfg| {
        ScopeFilter::from_args(cfg, &input.scope, &input.scope_only, input.scope_all)
            .unwrap_or_else(|_| ScopeFilter::default_for(cfg))
    });
    if let Some(ref f) = filter {
        notes.retain(|n| f.accepts(&n.vault_path));
    }

    let limit = input.limit.get() as usize;
    notes.truncate(limit);

    let entries = notes
        .into_iter()
        .filter_map(|n| {
            build_meta_entry(
                conn,
                &n.vault_path,
                &n.frontmatter_json,
                n.mtime_ms,
                &input.select,
                config,
            )
        })
        .collect();

    MetaResponse {
        vault: None,
        entries,
        tag_counts,
    }
}

fn query_tag_counts(conn: &Connection) -> BTreeMap<String, u32> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT nt.tag, COUNT(*) FROM note_tags nt \
         JOIN notes n ON nt.note_id = n.id \
         WHERE n.active = 1 \
         GROUP BY nt.tag \
         ORDER BY nt.tag",
    ) else {
        return BTreeMap::new();
    };
    stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
    })
    .and_then(Iterator::collect)
    .unwrap_or_default()
}

fn query_all_active_notes(conn: &Connection) -> Vec<NoteRow> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT id, vault_path, frontmatter, mtime_ms \
         FROM notes WHERE active = 1 ORDER BY vault_path",
    ) else {
        return Vec::new();
    };
    stmt.query_map([], |row| {
        Ok(NoteRow {
            note_id: row.get(0)?,
            vault_path: row.get(1)?,
            frontmatter_json: row.get(2)?,
            mtime_ms: row.get(3)?,
        })
    })
    .and_then(Iterator::collect)
    .unwrap_or_default()
}

fn query_notes_by_sources(conn: &Connection, target: &str) -> Vec<NoteRow> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT DISTINCT n.id, n.vault_path, n.frontmatter, n.mtime_ms \
         FROM notes n \
         JOIN note_frontmatter_fields nff ON n.id = nff.note_id \
         WHERE n.active = 1 AND nff.field = 'sources' AND nff.value = ? \
         ORDER BY n.vault_path",
    ) else {
        return Vec::new();
    };
    stmt.query_map(params![target], |row| {
        Ok(NoteRow {
            note_id: row.get(0)?,
            vault_path: row.get(1)?,
            frontmatter_json: row.get(2)?,
            mtime_ms: row.get(3)?,
        })
    })
    .and_then(Iterator::collect)
    .unwrap_or_default()
}

fn build_meta_entry(
    conn: &Connection,
    vault_path: &str,
    frontmatter_json: &str,
    mtime_ms: i64,
    select: &[String],
    config: Option<&TalonConfig>,
) -> Option<MetaEntry> {
    let path = VaultPath::parse(vault_path).ok()?;

    let fm_raw: BTreeMap<String, FrontmatterValue> = if frontmatter_json.is_empty() {
        BTreeMap::new()
    } else {
        serde_json::from_str(frontmatter_json).unwrap_or_default()
    };

    let frontmatter: BTreeMap<String, serde_json::Value> = fm_raw
        .into_iter()
        .filter(|(k, _)| select.is_empty() || select.contains(k))
        .map(|(k, v)| (k, fm_value_to_json(v)))
        .collect();

    let scope = config
        .and_then(|cfg| cfg.resolve_scope_name(std::path::Path::new(vault_path)))
        .map(str::to_string);
    let mtime = super::mtime::format_local_mtime(conn, mtime_ms);

    Some(MetaEntry {
        path,
        frontmatter,
        scope,
        mtime,
    })
}

fn fm_value_to_json(v: FrontmatterValue) -> serde_json::Value {
    use serde_json::Value;
    match v {
        FrontmatterValue::String(s) | FrontmatterValue::Date(s) => Value::String(s),
        FrontmatterValue::Number(n) => {
            serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
        }
        FrontmatterValue::Boolean(b) => Value::Bool(b),
        FrontmatterValue::List(items) => {
            Value::Array(items.into_iter().map(Value::String).collect())
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
