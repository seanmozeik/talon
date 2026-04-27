use std::collections::HashSet;

use rusqlite::{Connection, params};

use crate::contracts::VaultPath;
use crate::indexing::change_tracking;
use crate::query::related::find_related;
use crate::query::{
    EditedNote, FrontmatterFact, FuzzyAnchor, LinkedNote, NoteExcerpt, RecallInput, RelatedInput,
};
use crate::search::Direction;
use crate::search::fuzzy_title::search_title_parts;
use crate::search::types::RawSearchResult;

pub(super) fn build_linked_context(
    conn: &Connection,
    pipeline_results: &[RawSearchResult],
    input: &RecallInput,
    excluded_set: &HashSet<String>,
) -> (Vec<LinkedNote>, u32) {
    let Some(top_path) = pipeline_results.first().map(|r| r.path.clone()) else {
        return (Vec::new(), 0);
    };
    let ri = RelatedInput {
        path: top_path,
        depth: input.depth.clamp(0, 3),
        direction: Direction::Both,
        scope: input.scope.clone(),
        scope_only: input.scope_only.clone(),
    };
    let rel = find_related(conn, &ri);
    let link_count = u32::try_from(rel.results.len()).unwrap_or(u32::MAX);
    let notes: Vec<LinkedNote> = rel
        .results
        .into_iter()
        .filter(|r| !excluded_set.contains(r.vault_path.as_str()))
        .map(|r| LinkedNote {
            vault_path: r.vault_path,
            path: None,
            title: r.title,
            link_text: r.link_text,
            relation: r.relation,
            hops: 1,
        })
        .collect();
    (notes, link_count)
}

pub(super) fn collect_frontmatter(
    conn: &Connection,
    pipeline_results: &[RawSearchResult],
    excluded_set: &HashSet<String>,
) -> Vec<FrontmatterFact> {
    pipeline_results
        .iter()
        .filter(|r| !excluded_set.contains(&r.path))
        .flat_map(|r| extract_frontmatter_facts(conn, &r.path))
        .collect()
}

pub(super) fn to_note_excerpts(pipeline_results: &[RawSearchResult]) -> Vec<NoteExcerpt> {
    pipeline_results
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let vp = VaultPath::parse(&r.path).ok()?;
            Some(NoteExcerpt {
                vault_path: vp,
                path: None,
                title: r.title.clone(),
                snippet: r.snippet.clone(),
                score: r.score,
                rank: u32::try_from(i + 1).unwrap_or(u32::MAX),
            })
        })
        .collect()
}
fn extract_frontmatter_facts(conn: &Connection, vault_path: &str) -> Vec<FrontmatterFact> {
    let Ok(mut stmt) = conn.prepare_cached(
        "SELECT key, value FROM note_frontmatter_fields \
         WHERE note_id = (SELECT id FROM notes WHERE vault_path = ? AND active = 1) \
         ORDER BY key",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map(params![vault_path], |row| {
        let key: String = row.get(0)?;
        let val_str: String = row.get(1)?;
        Ok((key, val_str))
    }) else {
        return Vec::new();
    };
    let Ok(vp) = VaultPath::parse(vault_path) else {
        return Vec::new();
    };
    rows.flatten()
        .map(|(key, val_str)| {
            let value: serde_json::Value =
                serde_json::from_str(&val_str).unwrap_or(serde_json::Value::String(val_str));
            FrontmatterFact {
                vault_path: vp.clone(),
                path: None,
                key,
                value,
            }
        })
        .collect()
}

/// Collects recently edited notes within the `since` window.
///
/// Orders by composite recency+relevance score:
/// `0.6 * topic_relevance + 0.4 * exp(-days / half_life)`.
pub(super) fn collect_recent_edits(
    conn: &Connection,
    since: &str,
    active_paths: &[String],
    excluded: &HashSet<String>,
    half_life_days: u8,
) -> Vec<EditedNote> {
    let Ok(since_ms) = change_tracking::parse_since(since) else {
        return Vec::new();
    };
    let Ok(mut stmt) = conn.prepare_cached(
        "SELECT vault_path, title, mtime_ms FROM notes \
         WHERE active = 1 AND mtime_ms >= ? \
         ORDER BY mtime_ms DESC LIMIT 50",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map(params![since_ms], |row| {
        let path: String = row.get(0)?;
        let title: String = row.get::<_, Option<String>>(1)?.unwrap_or_default();
        let mtime_ms: u64 = row.get(2)?;
        Ok((path, title, mtime_ms))
    }) else {
        return Vec::new();
    };

    let now_ms = now_millis();
    let active_set: HashSet<&str> = active_paths.iter().map(String::as_str).collect();
    let half_life = f64::from(half_life_days);

    let mut edits: Vec<EditedNote> = rows
        .flatten()
        .filter(|(path, _, _)| !excluded.contains(path))
        .filter_map(|(path, title, mtime_ms)| {
            let diff_days =
                u32::try_from(now_ms.saturating_sub(mtime_ms) / 86_400_000).unwrap_or(u32::MAX);
            let days = f64::from(diff_days);
            let recency = (-days / half_life).exp();
            let topic_relevance = if active_set.contains(path.as_str()) {
                1.0
            } else {
                0.3
            };
            let score = 0.6_f64.mul_add(topic_relevance, 0.4 * recency);
            let Ok(vp) = VaultPath::parse(&path) else {
                return None;
            };
            Some(EditedNote {
                vault_path: vp,
                path: None,
                title,
                indexed_at: mtime_ms,
                days_since_modified: days,
                score,
            })
        })
        .collect();

    edits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    edits
}

/// Returns fuzzy title/alias matches that score below `main_threshold`.
pub(super) fn collect_fuzzy_anchors(
    conn: &Connection,
    query: &str,
    main_threshold: f64,
    excluded: &HashSet<String>,
) -> Vec<FuzzyAnchor> {
    let parts = search_title_parts(conn, query, 10);
    let threshold = main_threshold.max(0.01);
    parts
        .exact_alias
        .into_iter()
        .chain(parts.fuzzy)
        .filter(|r| r.score < threshold && !excluded.contains(&r.path))
        .filter_map(|r| {
            let Ok(vp) = VaultPath::parse(&r.path) else {
                return None;
            };
            Some(FuzzyAnchor {
                vault_path: vp,
                path: None,
                title: r.title,
                snippet: r.snippet,
                match_score: r.score,
            })
        })
        .collect()
}

/// Returns fractional days since a note's `mtime_ms`, or a large sentinel value.
pub(super) fn days_since_mtime(conn: &Connection, vault_path: &str) -> f64 {
    let mtime: Option<u64> = conn
        .query_row(
            "SELECT mtime_ms FROM notes WHERE vault_path = ? AND active = 1",
            params![vault_path],
            |row| row.get(0),
        )
        .ok();
    mtime.map_or(9999.0, |ms| {
        let diff_days =
            u32::try_from(now_millis().saturating_sub(ms) / 86_400_000).unwrap_or(u32::MAX);
        f64::from(diff_days)
    })
}

fn now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Returns an RFC-3339 string for 7 days ago.
pub(super) fn default_since_7d() -> String {
    let now = time::OffsetDateTime::now_utc();
    let week_ago = now - time::Duration::days(7);
    week_ago
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "2000-01-01T00:00:00Z".to_string())
}
