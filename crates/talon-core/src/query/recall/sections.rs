use std::collections::HashSet;

use rusqlite::{Connection, params};

use crate::config::TalonConfig;
use crate::contracts::VaultPath;
use crate::numeric::count_u32;
use crate::query::related::RelationKind;
use crate::query::related::find_related;
use crate::query::{LinkedNote, NoteExcerpt, RecallInput, RelatedInput};
use crate::search::Direction;
use crate::search::types::RawSearchResult;

fn to_headline(snippet: &str) -> String {
    let first = snippet
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if first.len() <= 120 {
        return first.to_owned();
    }
    first[..120]
        .rfind(['.', '!', '?'])
        .map_or_else(|| format!("{}…", &first[..117]), |i| first[..=i].to_owned())
}

fn mtime_date(conn: &Connection, path: &str) -> String {
    conn.query_row(
        "SELECT strftime('%Y-%m-%d', mtime_ms / 1000, 'unixepoch') \
         FROM notes WHERE vault_path = ?1 AND active = 1",
        rusqlite::params![path],
        |row| row.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
    .unwrap_or_default()
}

/// Returns the affinity multiplier for a given scope name.
///
/// Higher values promote linked notes from high-signal scopes; lower values
/// dampen contributions from noisy scopes (e.g. daily notes).
fn scope_affinity(scope: &str) -> f64 {
    match scope {
        "wiki" => 1.3,
        "projects" => 1.1,
        "raw" => 0.8,
        "daily" => 0.5,
        "archive" => 0.4,
        "private" => 0.3,
        "meta" => 0.2,
        // "artifacts" and unscoped/unknown notes use neutral weight 1.0.
        _ => 1.0,
    }
}

/// Minimum active-note score to contribute to linked context.
/// Same as the suppression confidence gate — marginal active notes
/// don't get to pollute the graph neighbourhood.
const LINKED_CTX_MIN_SCORE: f64 = 0.55;

/// Builds linked context by aggregating graph links from all active notes
/// that score above [`LINKED_CTX_MIN_SCORE`].
///
/// Each linked note accumulates an `aggregated_score` (sum of source note
/// scores) and a `source_notes` list. If two active notes point to the same
/// linked note, that note receives a higher aggregated score and appears
/// earlier in the output.  The MCP suppression layer uses `source_notes` to
/// recompute scores after filtering suppressed active notes, then drops
/// linked notes whose remaining score falls below its own (higher) threshold.
pub(super) fn build_linked_context(
    conn: &Connection,
    pipeline_results: &[RawSearchResult],
    input: &RecallInput,
    excluded_set: &HashSet<String>,
    config: Option<&TalonConfig>,
) -> (Vec<LinkedNote>, u32) {
    use std::collections::HashMap;

    struct Entry {
        vault_path: VaultPath,
        title: String,
        link_text: String,
        relation: RelationKind,
        best_source_score: f64,
        source_notes: Vec<(VaultPath, f64)>,
    }

    let mut by_path: HashMap<String, Entry> = HashMap::new();

    for source in pipeline_results {
        if source.score < LINKED_CTX_MIN_SCORE {
            continue;
        }
        let Ok(source_vpath) = VaultPath::parse(&source.path) else {
            continue;
        };
        let ri = RelatedInput {
            path: source.path.clone(),
            depth: input.depth.clamp(0, 3),
            direction: Direction::Both,
            scope: input.scope.clone(),
            scope_only: input.scope_only.clone(),
            scope_all: input.scope_all,
        };
        for r in find_related(conn, &ri, config).results {
            if excluded_set.contains(r.vault_path.as_str()) {
                continue;
            }
            let key = r.vault_path.as_str().to_owned();
            let entry = by_path.entry(key).or_insert_with(|| Entry {
                vault_path: r.vault_path.clone(),
                title: r.title.clone(),
                link_text: r.link_text.clone(),
                relation: r.relation,
                best_source_score: 0.0,
                source_notes: Vec::new(),
            });
            // Outgoing takes precedence over Backlink when sources disagree.
            if matches!(r.relation, RelationKind::Outgoing) {
                entry.relation = RelationKind::Outgoing;
            }
            // Link text comes from the highest-scoring source.
            if source.score > entry.best_source_score {
                entry.best_source_score = source.score;
                entry.link_text = r.link_text;
            }
            let affinity = scope_affinity(r.scope.as_deref().unwrap_or(""));
            entry
                .source_notes
                .push((source_vpath.clone(), source.score * affinity));
        }
    }

    let raw_count = count_u32(by_path.len());
    let mut notes: Vec<LinkedNote> = by_path
        .into_values()
        .map(|e| LinkedNote {
            vault_path: e.vault_path,
            title: e.title,
            link_text: e.link_text,
            relation: e.relation,
            hops: 1,
            aggregated_score: e.source_notes.iter().map(|(_, s)| s).sum(),
            source_notes: e.source_notes,
        })
        .collect();
    // Sort by aggregated score so budget trimmer drops the weakest first.
    notes.sort_by(|a, b| {
        b.aggregated_score
            .partial_cmp(&a.aggregated_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    (notes, raw_count)
}

pub(super) fn to_note_excerpts(
    conn: &Connection,
    pipeline_results: &[RawSearchResult],
) -> Vec<NoteExcerpt> {
    pipeline_results
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let vault_path = VaultPath::parse(&r.path).ok()?;
            Some(NoteExcerpt {
                vault_path,
                title: r.title.clone(),
                snippet: to_headline(&r.snippet),
                score: r.score,
                rank: u32::try_from(i + 1).unwrap_or(u32::MAX),
                mtime: mtime_date(conn, &r.path),
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::indexing::migrations::run_migrations;
    use rusqlite::Connection;

    #[test]
    fn scope_affinity_wiki_is_highest() {
        assert!(scope_affinity("wiki") > scope_affinity("projects"));
        assert!(scope_affinity("projects") > scope_affinity("daily"));
        assert!(scope_affinity("daily") > scope_affinity("archive"));
        assert!((scope_affinity("unknown_scope") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn headline_takes_first_nonempty_line() {
        assert_eq!(to_headline("line one\nline two"), "line one");
    }

    #[test]
    fn headline_skips_blank_lines() {
        assert_eq!(to_headline("\n\n  content  \n"), "content");
    }

    #[test]
    fn headline_truncates_long_line_at_sentence() {
        // >120 chars, ends with a period — should cut at sentence boundary
        let s = "A ".repeat(40) + "end.";
        let result = to_headline(&s);
        assert!(
            result.ends_with('.'),
            "should end at sentence boundary: {result:?}"
        );
        assert!(result.len() <= 120, "too long: {}", result.len());
    }

    #[test]
    fn headline_hard_truncates_with_ellipsis() {
        let s = "x".repeat(200);
        let result = to_headline(&s);
        assert!(
            result.ends_with('…'),
            "should end with ellipsis: {result:?}"
        );
        // 117 ascii bytes + ellipsis (3 UTF-8 bytes) = 120 bytes max
        assert!(result.len() <= 120, "too long: {}", result.len());
    }

    #[test]
    fn mtime_date_returns_empty_for_unknown_path() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        assert_eq!(mtime_date(&conn, "does/not/exist.md"), "");
    }

    #[test]
    fn mtime_date_formats_unix_ms_as_yyyy_mm_dd() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        // 2026-04-15 00:00:00 UTC = 1776211200 seconds = 1776211200000 ms
        conn.execute(
            "INSERT INTO notes (vault_path, title, tags, aliases, content, frontmatter, \
             mtime_ms, size_bytes, hash, docid, active, scope) \
             VALUES ('test.md', 'Test', '[]', '[]', '', '{}', 1776211200000, 0, 'h', 1, 1, '')",
            [],
        )
        .unwrap();
        assert_eq!(mtime_date(&conn, "test.md"), "2026-04-15");
    }
}
