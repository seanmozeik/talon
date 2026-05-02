use rusqlite::Connection;

use super::suggest::LinkSuggestion;
use super::{GraphSnapshot, build_missing_link_suggestions};
use crate::TalonError;

/// Builds deterministic missing-link suggestions.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] when note content cannot be read.
pub fn build_link_suggestions(
    conn: &Connection,
    snapshot: &GraphSnapshot,
) -> Result<Vec<LinkSuggestion>, TalonError> {
    build_missing_link_suggestions(conn, snapshot)
}
