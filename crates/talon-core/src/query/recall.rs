//! Vault-native context recall for agent lifecycle hooks.
//!
//! Implements `talon recall`: a composite pipeline that fans out to five
//! existing query modules (hybrid search, link graph, meta frontmatter,
//! change feed, fuzzy title search) and packs results into a token-budgeted
//! payload with a calibrated evidence score.
//!
//! Spec: `docs/recall.md`.  Scoring formulas: `recall_scoring.rs`.

use std::collections::HashSet;

use rusqlite::{Connection, params};
use tokenx_rs::estimate_token_count;

use crate::change_tracking;
use crate::config::TalonConfig;
use crate::contracts::VaultPath;
use crate::expansion::client::ExpansionClient;
use crate::inference::InferenceClient;
use crate::query::{
    EditedNote, FrontmatterFact, FuzzyAnchor, LinkedNote, NoteExcerpt, RecallInput, RecallResponse,
    RelatedInput, VaultRecall,
};
use crate::search::Direction;
use crate::search::bm25::search_bm25;
use crate::search::constants::DEFAULT_SNIPPET_LENGTH;
use crate::search::fuse::fuse_hybrid_result_lists;
use crate::search::fuzzy_title::search_title_parts;
use crate::search::hybrid_pipeline::{HybridPipelineOptions, run_hybrid_pipeline};

use super::recall_scoring::{EvidenceInputs, compute_evidence_score};
use super::related::find_related;

// ── section priority order for budget trimming ────────────────────────────────
// active_notes > linked_context > frontmatter > recent_edits > fuzzy_anchors

/// Runs the full recall pipeline and returns a `RecallResponse`.
///
/// When `inference` is `None` or `fast == true`, expansion and reranking are
/// skipped (the pipeline falls back to BM25+title lexical search).
///
/// # Panics
///
/// Does not panic under normal operation.  The internal `VaultPath::parse("_")`
/// fallback only fires when a path retrieved from the DB is empty, which should
/// not occur in a well-formed index.
#[must_use]
pub fn run_recall(
    conn: &Connection,
    inference: Option<&InferenceClient>,
    expansion: Option<&ExpansionClient>,
    input: &RecallInput,
    config: Option<&TalonConfig>,
) -> RecallResponse {
    if input.message.trim().is_empty() {
        return make_skipped(0.0);
    }

    let excluded_set: HashSet<String> = input.exclude.iter().cloned().collect();
    let query = build_query(input);
    let limit: u32 = 20;

    let raw = retrieve_pipeline_results(conn, inference, expansion, &query, limit, input.fast);
    let raw = apply_scope_priority(raw, config);

    let (pipeline_results, excluded_raw): (Vec<_>, Vec<_>) = raw
        .into_iter()
        .partition(|r| !excluded_set.contains(&r.path));
    let excluded_paths: Vec<String> = excluded_raw.into_iter().map(|r| r.path).collect();

    let top_rerank_score = pipeline_results
        .first()
        .map_or(0.0, |r| r.score.clamp(0.0, 1.0));
    let top_lexical_indicator =
        f64::from(u8::from(pipeline_results.iter().any(|r| {
            r.scores.bm25.is_some() || r.scores.fuzzy_title.is_some()
        })));

    let (linked_notes, top_link_count) =
        build_linked_context(conn, &pipeline_results, input, &excluded_set);

    let frontmatter_facts = collect_frontmatter(conn, &pipeline_results, &excluded_set);
    let frontmatter_match_indicator = if frontmatter_facts.is_empty() {
        0.0
    } else {
        1.0
    };

    let since_str = input.since.clone().unwrap_or_else(default_since_7d);
    let active_paths: Vec<String> = pipeline_results.iter().map(|r| r.path.clone()).collect();
    let recent_edits = collect_recent_edits(
        conn,
        &since_str,
        &active_paths,
        &excluded_set,
        input.recency_half_life_days,
    );

    let fuzzy_anchors = collect_fuzzy_anchors(conn, &query, top_rerank_score, &excluded_set);

    let top_days = pipeline_results
        .first()
        .map_or(9999.0, |r| days_since_mtime(conn, &r.path));

    let evidence_score = compute_evidence_score(&EvidenceInputs {
        top_rerank_score,
        top_lexical_indicator,
        top_result_link_count: top_link_count,
        days_since_top_result_modified: top_days,
        frontmatter_match_indicator,
    });

    if evidence_score < input.min_confidence || pipeline_results.is_empty() {
        return RecallResponse {
            vault_recall: None,
            evidence_score,
            tokens_used: 0,
            excluded: excluded_paths,
            excluded_by_budget: Vec::new(),
            skipped: true,
        };
    }

    let mut active_notes = to_note_excerpts(&pipeline_results);
    let mut linked_notes_mut = linked_notes;
    let mut frontmatter_facts_mut = frontmatter_facts;
    let mut recent_edits_mut = recent_edits;
    let mut fuzzy_anchors_mut = fuzzy_anchors;
    let mut excluded_by_budget: Vec<String> = Vec::new();

    trim_to_budget(
        input.budget_tokens as usize,
        &mut active_notes,
        &mut linked_notes_mut,
        &mut frontmatter_facts_mut,
        &mut recent_edits_mut,
        &mut fuzzy_anchors_mut,
        &mut excluded_by_budget,
    );

    let tokens_used = estimate_payload_tokens(
        &active_notes,
        &linked_notes_mut,
        &frontmatter_facts_mut,
        &recent_edits_mut,
        &fuzzy_anchors_mut,
    );

    RecallResponse {
        vault_recall: Some(VaultRecall {
            active_notes,
            linked_context: linked_notes_mut,
            frontmatter: frontmatter_facts_mut,
            recent_edits: recent_edits_mut,
            fuzzy_anchors: fuzzy_anchors_mut,
        }),
        evidence_score,
        tokens_used: u32::try_from(tokens_used).unwrap_or(u32::MAX),
        excluded: excluded_paths,
        excluded_by_budget,
        skipped: false,
    }
}

// ── private helpers ───────────────────────────────────────────────────────────

const fn make_skipped(evidence_score: f64) -> RecallResponse {
    RecallResponse {
        vault_recall: None,
        evidence_score,
        tokens_used: 0,
        excluded: Vec::new(),
        excluded_by_budget: Vec::new(),
        skipped: true,
    }
}

fn build_query(input: &RecallInput) -> String {
    if input.fast || input.prior_messages.is_empty() {
        return input.message.clone();
    }
    let mut combined = input.prior_messages.join("\n");
    combined.push('\n');
    combined.push_str(&input.message);
    combined
}

fn retrieve_pipeline_results(
    conn: &Connection,
    inference: Option<&InferenceClient>,
    expansion: Option<&ExpansionClient>,
    query: &str,
    limit: u32,
    fast: bool,
) -> Vec<crate::search::types::RawSearchResult> {
    let opts = HybridPipelineOptions {
        limit,
        fast,
        queries: Vec::new(),
    };
    inference.map_or_else(
        || run_fast_bm25_title(conn, query, limit),
        |inf| run_hybrid_pipeline(conn, inf, expansion, query, &opts),
    )
}

fn apply_scope_priority(
    results: Vec<crate::search::types::RawSearchResult>,
    config: Option<&TalonConfig>,
) -> Vec<crate::search::types::RawSearchResult> {
    let Some(cfg) = config else {
        return results;
    };
    results
        .into_iter()
        .map(|mut r| {
            let resolution = cfg.resolve_scope(std::path::Path::new(&r.path));
            r.score *= resolution.priority.multiplier();
            r
        })
        .collect()
}

fn build_linked_context(
    conn: &Connection,
    pipeline_results: &[crate::search::types::RawSearchResult],
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
            title: r.title,
            link_text: r.link_text,
            relation: r.relation,
            hops: 1,
        })
        .collect();
    (notes, link_count)
}

fn collect_frontmatter(
    conn: &Connection,
    pipeline_results: &[crate::search::types::RawSearchResult],
    excluded_set: &HashSet<String>,
) -> Vec<FrontmatterFact> {
    pipeline_results
        .iter()
        .filter(|r| !excluded_set.contains(&r.path))
        .flat_map(|r| extract_frontmatter_facts(conn, &r.path))
        .collect()
}

fn to_note_excerpts(
    pipeline_results: &[crate::search::types::RawSearchResult],
) -> Vec<NoteExcerpt> {
    pipeline_results
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let vp = VaultPath::parse(&r.path).ok()?;
            Some(NoteExcerpt {
                vault_path: vp,
                title: r.title.clone(),
                snippet: r.snippet.clone(),
                score: r.score,
                rank: u32::try_from(i + 1).unwrap_or(u32::MAX),
            })
        })
        .collect()
}

/// Fast BM25 + title search used when no `InferenceClient` is available.
fn run_fast_bm25_title(
    conn: &Connection,
    query: &str,
    limit: u32,
) -> Vec<crate::search::types::RawSearchResult> {
    let bm25 = search_bm25(conn, query, limit, DEFAULT_SNIPPET_LENGTH);
    let title_parts = search_title_parts(conn, query, limit);
    let mut all_title = title_parts.exact_alias;
    all_title.extend(title_parts.fuzzy);
    fuse_hybrid_result_lists(&[bm25.as_slice(), all_title.as_slice()], limit as usize)
}

/// Extracts all frontmatter key-value pairs for a note as [`FrontmatterFact`]s.
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
fn collect_recent_edits(
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
fn collect_fuzzy_anchors(
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
                title: r.title,
                snippet: r.snippet,
                match_score: r.score,
            })
        })
        .collect()
}

/// Returns fractional days since a note's `mtime_ms`, or a large sentinel value.
fn days_since_mtime(conn: &Connection, vault_path: &str) -> f64 {
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
fn default_since_7d() -> String {
    let now = time::OffsetDateTime::now_utc();
    let week_ago = now - time::Duration::days(7);
    week_ago
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "2000-01-01T00:00:00Z".to_string())
}

/// Estimates the total token cost of a recall payload.
fn estimate_payload_tokens(
    active_notes: &[NoteExcerpt],
    linked_context: &[LinkedNote],
    frontmatter: &[FrontmatterFact],
    recent_edits: &[EditedNote],
    fuzzy_anchors: &[FuzzyAnchor],
) -> usize {
    let mut total = 0usize;
    for n in active_notes {
        total += estimate_token_count(&n.title) + estimate_token_count(&n.snippet) + 10;
    }
    for l in linked_context {
        total += estimate_token_count(&l.title) + estimate_token_count(&l.link_text) + 8;
    }
    for f in frontmatter {
        total += estimate_token_count(&f.key) + estimate_token_count(&f.value.to_string()) + 6;
    }
    for e in recent_edits {
        total += estimate_token_count(&e.title) + 8;
    }
    for a in fuzzy_anchors {
        total += estimate_token_count(&a.title) + estimate_token_count(&a.snippet) + 8;
    }
    total
}

/// Greedy budget trimmer.
///
/// Drops lowest-ranked items from the lowest-priority non-empty section until
/// the token estimate fits within `budget` (with 2% slack per AC).
///
/// Section priority (highest to lowest):
/// `active_notes` > `linked_context` > `frontmatter` > `recent_edits` > `fuzzy_anchors`.
fn trim_to_budget(
    budget: usize,
    active_notes: &mut Vec<NoteExcerpt>,
    linked_context: &mut Vec<LinkedNote>,
    frontmatter: &mut Vec<FrontmatterFact>,
    recent_edits: &mut Vec<EditedNote>,
    fuzzy_anchors: &mut Vec<FuzzyAnchor>,
    excluded_by_budget: &mut Vec<String>,
) {
    loop {
        let used = estimate_payload_tokens(
            active_notes,
            linked_context,
            frontmatter,
            recent_edits,
            fuzzy_anchors,
        );
        // Allow 2% slack per AC.  Compute using integer arithmetic to avoid casts.
        let budget_with_slack = budget.saturating_add(budget / 50);
        if used <= budget_with_slack {
            break;
        }
        if let Some(item) = fuzzy_anchors.pop() {
            excluded_by_budget.push(item.vault_path.as_str().to_string());
        } else if let Some(item) = recent_edits.pop() {
            excluded_by_budget.push(item.vault_path.as_str().to_string());
        } else if let Some(item) = frontmatter.pop() {
            excluded_by_budget.push(item.vault_path.as_str().to_string());
        } else if let Some(item) = linked_context.pop() {
            excluded_by_budget.push(item.vault_path.as_str().to_string());
        } else if let Some(item) = active_notes.pop() {
            excluded_by_budget.push(item.vault_path.as_str().to_string());
        } else {
            break;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use rusqlite::Connection;

    use super::*;
    use crate::migrations::run_migrations;

    fn fresh_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn insert_note(conn: &Connection, vault_path: &str, title: &str) {
        conn.execute(
            "INSERT INTO notes \
             (vault_path, title, tags, aliases, content, frontmatter, mtime_ms, size_bytes, hash, docid, active) \
             VALUES (?, ?, '[]', '[]', '', '{}', 1_000_000, 0, 'h', ?, 1)",
            params![vault_path, title, vault_path],
        )
        .unwrap();
    }

    fn insert_link(conn: &Connection, from: &str, to: &str) {
        conn.execute(
            "INSERT OR IGNORE INTO links (from_path, to_path, raw_target) VALUES (?, ?, ?)",
            params![from, to, to],
        )
        .unwrap();
    }

    fn recall_input(message: &str) -> RecallInput {
        RecallInput {
            message: message.to_string(),
            budget_tokens: 10_000,
            ..RecallInput::default()
        }
    }

    #[test]
    fn empty_message_returns_skipped() {
        let conn = fresh_db();
        let result = run_recall(&conn, None, None, &recall_input("   "), None);
        assert!(result.skipped);
        assert_eq!(result.evidence_score, 0.0);
    }

    #[test]
    fn no_results_returns_skipped() {
        let conn = fresh_db();
        let result = run_recall(&conn, None, None, &recall_input("nothing here"), None);
        assert!(result.skipped);
    }

    #[test]
    fn exclude_does_not_panic() {
        let conn = fresh_db();
        insert_note(&conn, "Atlas/Note.md", "Note");

        let input = RecallInput {
            message: "Note".to_string(),
            exclude: vec!["Atlas/Note.md".to_string()],
            budget_tokens: 10_000,
            ..RecallInput::default()
        };
        let result = run_recall(&conn, None, None, &input, None);
        // excluded path must not appear in active_notes
        if let Some(vr) = &result.vault_recall {
            for note in &vr.active_notes {
                assert_ne!(note.vault_path.as_str(), "Atlas/Note.md");
            }
        }
    }

    #[test]
    fn linked_context_does_not_panic() {
        let conn = fresh_db();
        insert_note(&conn, "Hub.md", "Hub");
        insert_note(&conn, "Child.md", "Child");
        insert_link(&conn, "Hub.md", "Child.md");

        let result = run_recall(&conn, None, None, &recall_input("Hub"), None);
        assert!(result.excluded_by_budget.is_empty());
    }

    #[test]
    fn budget_enforcement_populates_excluded_by_budget() {
        let active = vec![
            NoteExcerpt {
                vault_path: VaultPath::parse("A.md").unwrap(),
                title: "A".to_string(),
                snippet: "a".repeat(50),
                score: 1.0,
                rank: 1,
            },
            NoteExcerpt {
                vault_path: VaultPath::parse("B.md").unwrap(),
                title: "B".to_string(),
                snippet: "b".repeat(50),
                score: 0.5,
                rank: 2,
            },
        ];
        let mut active_mut = active;
        let mut linked: Vec<LinkedNote> = Vec::new();
        let mut fm: Vec<FrontmatterFact> = Vec::new();
        let mut edits: Vec<EditedNote> = Vec::new();
        let mut anchors: Vec<FuzzyAnchor> = Vec::new();
        let mut dropped: Vec<String> = Vec::new();

        trim_to_budget(
            1,
            &mut active_mut,
            &mut linked,
            &mut fm,
            &mut edits,
            &mut anchors,
            &mut dropped,
        );

        assert!(
            !dropped.is_empty(),
            "budget trimmer must populate excluded_by_budget"
        );
    }
}
