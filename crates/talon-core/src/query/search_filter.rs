use rusqlite::Connection;

use crate::search::pre_filter::PreFilter;
use crate::search::types::RawSearchResult;

pub(super) fn apply_glob_post_filter(
    conn: &Connection,
    raw_results: Vec<RawSearchResult>,
    pre_filter: &PreFilter,
) -> Vec<RawSearchResult> {
    if crate::search::pre_filter::has_glob_where_clauses(&pre_filter.where_clauses) {
        crate::search::pre_filter::filter_results_by_glob(
            conn,
            &raw_results,
            &pre_filter.where_clauses,
        )
    } else {
        raw_results
    }
}
