use rusqlite::Connection;

use super::suggest::LinkSuggestion;
use super::suggest_llm::GraphSuggestionClient;
use super::{GraphSnapshot, build_missing_link_suggestions};
use crate::TalonError;

pub(super) fn build_link_suggestions(
    conn: &Connection,
    snapshot: &GraphSnapshot,
    suggester: Option<&GraphSuggestionClient>,
) -> Result<Vec<LinkSuggestion>, TalonError> {
    let mut suggestions = build_missing_link_suggestions(conn, snapshot)?;
    if let Some(suggester) = suggester {
        suggestions.extend(super::suggest_llm::build_llm_link_suggestions(
            conn, snapshot, suggester,
        )?);
        suggestions.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.target.cmp(&right.target))
                .then_with(|| left.provenance.cmp(&right.provenance))
        });
        suggestions.dedup_by(|left, right| left.path == right.path && left.target == right.target);
    }
    Ok(suggestions)
}
