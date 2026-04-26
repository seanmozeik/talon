//! Embed-pass run state: dimension tracking, diagnostics buffer, and
//! the `mark chunks failed` write.
//!
//! Ports `embed/chunks-diagnostics.ts`. The diagnostics buffer is capped
//! so a sidecar that's failing every request does not flood the agent-facing
//! response.

use rusqlite::{Connection, params};

use crate::TalonError;
use crate::inference::redact;

use super::pending::NoteWithChunks;

/// Hard cap on diagnostic strings retained per pass (older entries are
/// dropped).
pub const MAX_DIAGNOSTICS: usize = 20;

/// Read-only view of an embed pass's diagnostic outcome.
#[derive(Debug, Clone, Default)]
pub struct EmbedDiagnostics {
    /// Notes seen in the pass (single-chunk or multi-chunk).
    pub processed: u32,
    /// Notes that landed an embedding successfully.
    pub succeeded: u32,
    /// Notes that failed (HTTP error, dim mismatch, empty response, ...).
    pub failed: u32,
    /// True if any successful row arrived at a different dimensionality
    /// than the first; semantic search must be considered offline until
    /// resolved.
    pub dimension_mismatch: bool,
    /// Up to [`MAX_DIAGNOSTICS`] redacted detail strings.
    pub diagnostics: Vec<String>,
}

/// Mutable run state passed through the embed pipeline.
#[derive(Debug, Default)]
pub struct EmbedRunContext {
    /// First successful row's dimensionality; pinned for the rest of the
    /// pass so a model swap mid-pass is caught.
    pub current_dimensions: Option<u32>,
    /// Set true if any later row's dimensionality differs from
    /// `current_dimensions`.
    pub dimension_mismatch: bool,
    /// Stats accumulators.
    pub processed: u32,
    pub succeeded: u32,
    pub failed: u32,
    /// Diagnostic ring buffer.
    pub diagnostics: Vec<String>,
}

impl EmbedRunContext {
    /// Snapshots the context as a [`EmbedDiagnostics`] for return to the
    /// caller.
    #[must_use]
    pub fn snapshot(&self) -> EmbedDiagnostics {
        EmbedDiagnostics {
            processed: self.processed,
            succeeded: self.succeeded,
            failed: self.failed,
            dimension_mismatch: self.dimension_mismatch,
            diagnostics: self.diagnostics.clone(),
        }
    }

    /// Records a redacted `"<vault_path>: <detail>"` line. No-op once
    /// the cap is reached.
    pub fn record_diagnostic(&mut self, vault_path: &str, detail: &str) {
        if self.diagnostics.len() >= MAX_DIAGNOSTICS {
            return;
        }
        let line = redact(&format!("{vault_path}: {detail}"));
        tracing::warn!(target: "talon::embed", "{line}");
        self.diagnostics.push(line);
    }
}

/// Marks every chunk on `note` as `failed` so the next non-forced embed
/// pass picks them up again. Failures here are surfaced because the next
/// pass would silently skip a note with stale `pending` state.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] for any underlying update failure.
pub fn mark_note_chunks_failed(conn: &Connection, note: &NoteWithChunks) -> Result<(), TalonError> {
    for chunk in &note.chunks {
        conn.execute(
            "UPDATE chunks SET embedding_status = 'failed' WHERE id = ?",
            params![chunk.chunk_id],
        )
        .map_err(|source| TalonError::Sqlite {
            context: "mark chunk failed",
            source,
        })?;
    }
    Ok(())
}

/// Pins or validates the run-wide embedding dimensionality.
///
/// On the first successful row, sets `current_dimensions` and ensures
/// `vec_chunks` exists at that size (creating or resizing as needed).
/// Subsequent rows must match; mismatches set `dimension_mismatch = true`
/// so the runner can mark the note failed and surface the warning.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] for the underlying `ensure_vec_chunks`
/// DDL/DML failure.
pub fn align_embedding_dimensions(
    conn: &Connection,
    ctx: &mut EmbedRunContext,
    dims: u32,
) -> Result<(), TalonError> {
    match ctx.current_dimensions {
        None => {
            ctx.current_dimensions = Some(dims);
            crate::vec_ext::ensure_vec_chunks(conn, dims)?;
            Ok(())
        }
        Some(existing) if existing == dims => Ok(()),
        Some(existing) => {
            ctx.dimension_mismatch = true;
            tracing::error!(
                target: "talon::embed",
                expected = existing,
                got = dims,
                "embedding dimension mismatch — semantic search disabled until consistent"
            );
            Ok(())
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::store::open_database;
    use crate::vec_ext::register_sqlite_vec;
    use std::env::temp_dir;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_path() -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        temp_dir().join(format!("talon-embed-diag-test-{pid}-{n}.sqlite"))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = fs_err::remove_file(path);
        let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
    }

    #[test]
    fn record_diagnostic_caps_at_max() {
        let mut ctx = EmbedRunContext::default();
        for i in 0..(MAX_DIAGNOSTICS + 5) {
            ctx.record_diagnostic("a.md", &format!("detail {i}"));
        }
        assert_eq!(ctx.diagnostics.len(), MAX_DIAGNOSTICS);
    }

    #[test]
    fn record_diagnostic_redacts_paths() {
        let mut ctx = EmbedRunContext::default();
        ctx.record_diagnostic("note.md", "POST https://localhost:8080/embed timed out");
        assert!(ctx.diagnostics[0].contains("[sidecar]"));
    }

    #[test]
    fn align_dimensions_pins_then_detects_mismatch() {
        register_sqlite_vec().unwrap();
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        let mut ctx = EmbedRunContext::default();
        align_embedding_dimensions(&conn, &mut ctx, 768).unwrap();
        assert_eq!(ctx.current_dimensions, Some(768));
        assert!(!ctx.dimension_mismatch);
        align_embedding_dimensions(&conn, &mut ctx, 768).unwrap();
        assert!(!ctx.dimension_mismatch);
        align_embedding_dimensions(&conn, &mut ctx, 384).unwrap();
        assert!(ctx.dimension_mismatch);
        drop(conn);
        cleanup(&path);
    }
}
