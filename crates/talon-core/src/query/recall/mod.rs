//! Vault-native context recall for agent lifecycle hooks.
//!
//! Implements `talon recall`: a composite pipeline that fans out to five
//! existing query modules (hybrid search, link graph, meta frontmatter,
//! change feed, fuzzy title search) and packs results into a token-budgeted
//! payload with a calibrated evidence score.
//!
//! Spec: `docs/recall.md`.  Scoring formulas: `recall_scoring.rs`.

use std::collections::HashSet;

use rusqlite::Connection;

use crate::config::TalonConfig;
use crate::expansion::client::ExpansionClient;
use crate::inference::InferenceClient;
use crate::query::{
    EditedNote, FrontmatterFact, FuzzyAnchor, LinkedNote, NoteExcerpt, RecallInput, RecallResponse,
    VaultRecall,
};

use super::recall_scoring::{EvidenceInputs, compute_evidence_score};

mod budget;
mod retrieval;
mod sections;

use budget::{estimate_payload_tokens, trim_to_budget};
use retrieval::{apply_scope_priority, build_query, retrieve_pipeline_results};
use sections::{
    build_linked_context, collect_frontmatter, collect_fuzzy_anchors, collect_recent_edits,
    days_since_mtime, default_since_7d, to_note_excerpts,
};

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
            vault: None,
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

    build_response(
        config,
        RecallResponseParts {
            active_notes,
            linked_notes: linked_notes_mut,
            frontmatter: frontmatter_facts_mut,
            recent_edits: recent_edits_mut,
            fuzzy_anchors: fuzzy_anchors_mut,
            evidence_score,
            tokens_used,
            excluded: excluded_paths,
            excluded_by_budget,
        },
    )
}

// ── private helpers ───────────────────────────────────────────────────────────

struct RecallResponseParts {
    active_notes: Vec<NoteExcerpt>,
    linked_notes: Vec<LinkedNote>,
    frontmatter: Vec<FrontmatterFact>,
    recent_edits: Vec<EditedNote>,
    fuzzy_anchors: Vec<FuzzyAnchor>,
    evidence_score: f64,
    tokens_used: usize,
    excluded: Vec<String>,
    excluded_by_budget: Vec<String>,
}

fn build_response(_config: Option<&TalonConfig>, parts: RecallResponseParts) -> RecallResponse {
    RecallResponse {
        vault: None,
        vault_recall: Some(VaultRecall {
            active_notes: parts.active_notes,
            linked_context: parts.linked_notes,
            frontmatter: parts.frontmatter,
            recent_edits: parts.recent_edits,
            fuzzy_anchors: parts.fuzzy_anchors,
        }),
        evidence_score: parts.evidence_score,
        tokens_used: u32::try_from(parts.tokens_used).unwrap_or(u32::MAX),
        excluded: parts.excluded,
        excluded_by_budget: parts.excluded_by_budget,
        skipped: false,
    }
}

const fn make_skipped(evidence_score: f64) -> RecallResponse {
    RecallResponse {
        vault: None,
        vault_recall: None,
        evidence_score,
        tokens_used: 0,
        excluded: Vec::new(),
        excluded_by_budget: Vec::new(),
        skipped: true,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests;
