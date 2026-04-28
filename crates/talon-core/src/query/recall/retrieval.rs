use rusqlite::Connection;

use crate::config::TalonConfig;
use crate::expansion::client::ExpansionClient;
use crate::inference::InferenceClient;
use crate::query::RecallInput;
use crate::search::bm25::search_bm25;
use crate::search::constants::{CANDIDATE_FLOOR, DEFAULT_SNIPPET_LENGTH};
use crate::search::fuse::fuse_hybrid_result_lists;
use crate::search::fuzzy_title::search_title_parts;
use crate::search::hybrid_pipeline::{HybridPipelineOptions, run_hybrid_pipeline};
use crate::search::pre_filter::PreFilter;
use crate::search::types::RawSearchResult;

pub(super) fn build_query(input: &RecallInput) -> String {
    if input.fast || input.prior_messages.is_empty() {
        return input.message.clone();
    }
    let mut combined = input.prior_messages.join("\n");
    combined.push('\n');
    combined.push_str(&input.message);
    combined
}

pub(super) fn retrieve_pipeline_results(
    conn: &Connection,
    inference: Option<&InferenceClient>,
    expansion: Option<&ExpansionClient>,
    query: &str,
    limit: u32,
    fast: bool,
) -> Vec<RawSearchResult> {
    let opts = HybridPipelineOptions {
        limit,
        candidate_limit: CANDIDATE_FLOOR,
        fast,
        queries: Vec::new(),
        intent: None,
        hooks: crate::search::SearchHooks::default(),
        pre_filter: PreFilter::none(),
    };
    inference.map_or_else(
        || run_fast_bm25_title(conn, query, limit),
        |inf| run_hybrid_pipeline(conn, inf, expansion, query, &opts),
    )
}

pub(super) fn apply_scope_priority(
    results: Vec<RawSearchResult>,
    config: Option<&TalonConfig>,
) -> Vec<RawSearchResult> {
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
fn run_fast_bm25_title(conn: &Connection, query: &str, limit: u32) -> Vec<RawSearchResult> {
    let bm25 = search_bm25(
        conn,
        query,
        limit,
        DEFAULT_SNIPPET_LENGTH,
        &PreFilter::none(),
    );
    let title_parts = search_title_parts(conn, query, limit, &PreFilter::none());
    let mut all_title = title_parts.exact_alias;
    all_title.extend(title_parts.fuzzy);
    fuse_hybrid_result_lists(
        &[bm25.as_slice(), all_title.as_slice()],
        &[1.0, 1.0],
        limit as usize,
    )
}
