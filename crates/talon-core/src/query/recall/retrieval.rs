use std::time::Instant;

use rusqlite::Connection;

use crate::config::TalonConfig;
use crate::expansion::client::ExpansionClient;
use crate::inference::InferenceClient;
use crate::query::RecallInput;
use crate::search::bm25::search_bm25;
use crate::search::constants::{CANDIDATE_FLOOR, DEFAULT_SNIPPET_LENGTH};
use crate::search::fuse::fuse_hybrid_result_lists;
use crate::search::fuzzy_title::search_title_parts;
use crate::search::hybrid_pipeline::{HybridPipelineOptions, run_hybrid_pipeline_with_metadata};
use crate::search::pre_filter::PreFilter;
use crate::search::types::RawSearchResult;

pub(super) struct RetrievePipelineArgs<'a> {
    pub(super) conn: &'a Connection,
    pub(super) inference: Option<&'a InferenceClient>,
    pub(super) expansion: Option<&'a ExpansionClient>,
    pub(super) query: &'a str,
    pub(super) queries: &'a [String],
    pub(super) limit: u32,
    pub(super) fast: bool,
    pub(super) pre_filter: &'a PreFilter,
    pub(super) deadline_at: Option<Instant>,
}

pub(super) fn build_query(input: &RecallInput) -> String {
    if input.fast || input.prior_messages.is_empty() {
        return input.message.clone();
    }
    let mut combined = input.prior_messages.join("\n");
    combined.push('\n');
    combined.push_str(&input.message);
    combined
}

#[derive(Debug, Clone, Default)]
pub(super) struct RecallRetrievalOutput {
    pub(super) results: Vec<RawSearchResult>,
    pub(super) rerank_ms: Option<u64>,
}

pub(super) fn retrieve_pipeline_results(args: &RetrievePipelineArgs<'_>) -> RecallRetrievalOutput {
    let opts = HybridPipelineOptions {
        limit: args.limit,
        candidate_limit: CANDIDATE_FLOOR,
        fast: args.fast,
        queries: args.queries.to_vec(),
        intent: None,
        hooks: crate::search::SearchHooks::default(),
        pre_filter: args.pre_filter.clone(),
        deadline_at: args.deadline_at,
    };
    args.inference.map_or_else(
        || RecallRetrievalOutput {
            results: run_fast_bm25_title(args.conn, args.query, args.limit, args.pre_filter),
            rerank_ms: None,
        },
        |inf| {
            let output = run_hybrid_pipeline_with_metadata(
                args.conn,
                inf,
                args.expansion,
                args.query,
                &opts,
            );
            RecallRetrievalOutput {
                results: output.results,
                rerank_ms: output.rerank_ms,
            }
        },
    )
}

pub(super) fn apply_scope_priority(
    results: Vec<RawSearchResult>,
    config: Option<&TalonConfig>,
    requested_scopes: &[String],
) -> Vec<RawSearchResult> {
    let Some(cfg) = config else {
        return results;
    };
    results
        .into_iter()
        .map(|mut r| {
            let resolution = cfg.resolve_scope(std::path::Path::new(&r.path));
            let mut score = resolution.priority.apply_to_score(r.score);
            if cfg
                .resolve_scope_name(std::path::Path::new(&r.path))
                .is_some_and(|name| requested_scopes.iter().any(|requested| requested == name))
            {
                score = score.max(r.score);
            }
            r.score = score;
            r
        })
        .collect()
}
fn run_fast_bm25_title(
    conn: &Connection,
    query: &str,
    limit: u32,
    pre_filter: &PreFilter,
) -> Vec<RawSearchResult> {
    let bm25 = search_bm25(conn, query, limit, DEFAULT_SNIPPET_LENGTH, pre_filter);
    let title_parts = search_title_parts(conn, query, limit, pre_filter);
    let mut all_title = title_parts.exact_alias;
    all_title.extend(title_parts.fuzzy);
    fuse_hybrid_result_lists(
        &[bm25.as_slice(), all_title.as_slice()],
        &[1.0, 1.0],
        limit as usize,
    )
}
