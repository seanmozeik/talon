//! Vault-native context recall for agent lifecycle hooks.
//!
//! Implements `talon recall`: a composite pipeline that fans out to
//! hybrid search and the link graph, then packs results into a
//! token-budgeted payload with a calibrated evidence score.
//!
//! Spec: `docs/recall.md`.  Scoring formulas: `recall_scoring.rs`.

use std::collections::HashSet;

use rusqlite::Connection;

use crate::ScopeFilter;
use crate::config::TalonConfig;
use crate::expansion::client::ExpansionClient;
use crate::inference::InferenceClient;
use crate::query::{RecallInput, RecallResponse, VaultRecall};
use crate::search::pre_filter::{PreFilter, scope_to_note_ids};

use super::recall_scoring::{EvidenceInputs, compute_evidence_score};

mod budget;
mod retrieval;
mod sections;

use budget::{estimate_payload_tokens, trim_to_budget};
use retrieval::{apply_scope_priority, build_query, retrieve_pipeline_results};
use sections::{build_linked_context, days_since_mtime, to_note_excerpts};

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

    let pre_filter = config.map_or_else(PreFilter::none, |cfg| {
        let filter = ScopeFilter::from_args(cfg, &input.scope, &input.scope_only, input.scope_all)
            .unwrap_or_else(|_| ScopeFilter::default_for(cfg));
        PreFilter {
            since_ms: None,
            accepted_note_ids: scope_to_note_ids(conn, &filter),
            where_clauses: Vec::new(),
            tags: Vec::new(),
            headings: Vec::new(),
        }
    });
    if pre_filter.is_impossible() {
        return make_skipped(0.0);
    }

    let raw = retrieve_pipeline_results(
        conn,
        inference,
        expansion,
        &query,
        limit,
        input.fast,
        &pre_filter,
    );
    let mut raw = apply_scope_priority(raw, config, &input.scope);
    raw.sort_by(|a, b| b.score.total_cmp(&a.score));

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
        build_linked_context(conn, &pipeline_results, input, &excluded_set, config);

    let top_days = pipeline_results
        .first()
        .map_or(9999.0, |r| days_since_mtime(conn, &r.path));

    let evidence_score = compute_evidence_score(&EvidenceInputs {
        top_rerank_score,
        top_lexical_indicator,
        top_result_link_count: top_link_count,
        days_since_top_result_modified: top_days,
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

    let mut active_notes = to_note_excerpts(conn, &pipeline_results);
    let mut linked_notes_mut = linked_notes;
    let mut excluded_by_budget: Vec<String> = Vec::new();

    trim_to_budget(
        input.budget_tokens as usize,
        &mut active_notes,
        &mut linked_notes_mut,
        &mut excluded_by_budget,
    );

    let tokens_used = estimate_payload_tokens(&active_notes, &linked_notes_mut);

    RecallResponse {
        vault: None,
        vault_recall: Some(VaultRecall {
            active_notes,
            linked_context: linked_notes_mut,
        }),
        evidence_score,
        tokens_used: u32::try_from(tokens_used).unwrap_or(u32::MAX),
        excluded: excluded_paths,
        excluded_by_budget,
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
