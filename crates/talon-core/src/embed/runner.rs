//! Top-level embed pass: select pending chunks → call sidecar → persist.
//!
//! Ports `embed/chunks-run.ts` + `embed/scheduler.ts::runEmbedPass`. The
//! runner picks the `/embed` (single-chunk) or `/embed-chunked` (multi-
//! chunk) endpoint per note, pins dimensionality on the first success,
//! and writes vectors transactionally so a partial failure does not leave
//! `vec_chunks` and `chunks.embedding_status` out of sync.

use rusqlite::Connection;
use time::OffsetDateTime;

use crate::TalonError;
use crate::inference::{EmbedChunkedDataItem, InferenceClient, InferenceError};

use super::diagnostics::{
    EmbedDiagnostics, EmbedRunContext, align_embedding_dimensions, mark_note_chunks_failed,
};
use super::pending::{NoteWithChunks, get_pending_chunks};
use super::persist::{first_non_empty_batch, persist_chunk_vector};

/// Options for [`run_embed_pass`].
#[derive(Debug, Clone, Default)]
pub struct EmbedPassOptions {
    /// Re-embed every chunk even if its `embedding_status` is `ok`.
    pub force: bool,
    /// Restrict the pass to these vault-relative paths (empty = whole vault).
    pub restrict_paths: Vec<String>,
    /// Sidecar model name written to `vector_metadata.model` for single-
    /// chunk notes. Defaults to `"embed"` (the sidecar's `embed` `model_id`).
    pub chunk_embedding_model: String,
    /// Sidecar model name written for multi-chunk notes. Defaults to
    /// `"embed_chunked"`.
    pub document_embedding_model: String,
}

impl EmbedPassOptions {
    /// Builds defaults that match the Talon sidecar's wrapper IDs.
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            force: false,
            restrict_paths: Vec::new(),
            chunk_embedding_model: "embed".to_string(),
            document_embedding_model: "embed_chunked".to_string(),
        }
    }
}

/// Stats returned by [`run_embed_pass`].
#[derive(Debug, Clone, Default)]
pub struct EmbedPassStats {
    /// Notes encountered during the pass.
    pub processed: u32,
    /// Notes successfully embedded.
    pub succeeded: u32,
    /// Notes that failed.
    pub failed: u32,
    /// True if any note's vector dimensionality differed from the
    /// established pass dimensionality (semantic search disabled).
    pub dimension_mismatch: bool,
    /// Actionable remediation hint, populated when the pass detected a
    /// recoverable problem (e.g. dim mismatch needs `talon embed --force`).
    /// `None` when no remediation is needed.
    pub remediation: Option<String>,
    /// Up to [`super::diagnostics::MAX_DIAGNOSTICS`] redacted diagnostic
    /// strings.
    pub diagnostics: Vec<String>,
}

/// Operator-facing remediation message for a dimension mismatch.
///
/// Surfaced verbatim in the CLI human output and the JSON response so the
/// agent can also relay the hint.
pub const DIMENSION_MISMATCH_REMEDIATION: &str = "embedding model changed mid-pass — run `talon embed --force` to drop the existing vec_chunks index and re-embed every chunk at the new dimensionality";

impl From<EmbedDiagnostics> for EmbedPassStats {
    fn from(value: EmbedDiagnostics) -> Self {
        let remediation = value
            .dimension_mismatch
            .then(|| DIMENSION_MISMATCH_REMEDIATION.to_string());
        Self {
            processed: value.processed,
            succeeded: value.succeeded,
            failed: value.failed,
            dimension_mismatch: value.dimension_mismatch,
            remediation,
            diagnostics: value.diagnostics,
        }
    }
}

/// Runs one embed pass: select pending chunks, call the sidecar, persist.
///
/// `client` performs the HTTP work; `conn` is the same `rusqlite`
/// connection the rest of the indexer uses (the runner is sync precisely
/// so it can share a connection with `run_sync`).
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] for any underlying DB failure during
/// the initial select. Per-note HTTP/JSON failures are recorded in
/// `EmbedPassStats.diagnostics` rather than aborting the whole pass.
pub fn run_embed_pass(
    conn: &Connection,
    client: &InferenceClient,
    options: &EmbedPassOptions,
) -> Result<EmbedPassStats, TalonError> {
    let pending = get_pending_chunks(conn, options.force, &options.restrict_paths)?;
    let mut ctx = EmbedRunContext::default();

    for note in &pending {
        if note.chunks.len() == 1 {
            embed_single_chunk(conn, client, options, note, &mut ctx);
        } else {
            embed_multi_chunk(conn, client, options, note, &mut ctx);
        }
    }

    Ok(ctx.snapshot().into())
}

fn now_ms() -> i64 {
    let nanos = OffsetDateTime::now_utc().unix_timestamp_nanos();
    i64::try_from(nanos / 1_000_000).unwrap_or(i64::MAX)
}

fn fail_note(conn: &Connection, note: &NoteWithChunks, ctx: &mut EmbedRunContext, detail: &str) {
    ctx.failed = ctx.failed.saturating_add(1);
    ctx.record_diagnostic(&note.vault_path, detail);
    if let Err(err) = mark_note_chunks_failed(conn, note) {
        tracing::error!(
            target: "talon::embed",
            vault_path = note.vault_path,
            error = %err,
            "could not mark chunks failed"
        );
    }
}

fn format_inference_failure(err: &InferenceError) -> String {
    err.to_string()
}

fn embed_single_chunk(
    conn: &Connection,
    client: &InferenceClient,
    options: &EmbedPassOptions,
    note: &NoteWithChunks,
    ctx: &mut EmbedRunContext,
) {
    ctx.processed = ctx.processed.saturating_add(1);
    let Some(chunk) = note.chunks.first() else {
        return;
    };
    let response = match client.embed(std::slice::from_ref(&chunk.embedding_text)) {
        Ok(rows) => rows,
        Err(err) => {
            fail_note(conn, note, ctx, &format_inference_failure(&err));
            return;
        }
    };
    let Some(row) = response.into_iter().next() else {
        fail_note(conn, note, ctx, "sidecar returned no embedding rows");
        return;
    };
    let dims = match u32::try_from(row.len()) {
        Ok(d) if d > 0 => d,
        _ => {
            fail_note(conn, note, ctx, "sidecar returned empty embedding vector");
            return;
        }
    };
    if let Err(err) = align_embedding_dimensions(conn, ctx, dims) {
        fail_note(conn, note, ctx, &err.to_string());
        return;
    }
    if ctx.dimension_mismatch {
        fail_note(
            conn,
            note,
            ctx,
            &format!(
                "embedding dimension mismatch (expected {expected}, got {dims}); semantic search disabled — run `talon embed --force` to rebuild at the new dimensionality",
                expected = ctx.current_dimensions.unwrap_or(0)
            ),
        );
        return;
    }
    if let Err(err) = persist_chunk_vector(
        conn,
        chunk.chunk_id,
        &options.chunk_embedding_model,
        dims,
        now_ms(),
        &row,
    ) {
        fail_note(conn, note, ctx, &err.to_string());
        return;
    }
    ctx.succeeded = ctx.succeeded.saturating_add(1);
}

fn embed_multi_chunk(
    conn: &Connection,
    client: &InferenceClient,
    options: &EmbedPassOptions,
    note: &NoteWithChunks,
    ctx: &mut EmbedRunContext,
) {
    ctx.processed = ctx.processed.saturating_add(1);
    let texts: Vec<String> = note
        .chunks
        .iter()
        .map(|c| c.embedding_text.clone())
        .collect();
    let response = match client.embed_chunked(&[texts]) {
        Ok(r) => r,
        Err(err) => {
            fail_note(conn, note, ctx, &format_inference_failure(&err));
            return;
        }
    };
    let Some((dims, batch)) = first_non_empty_batch(&response) else {
        fail_note(
            conn,
            note,
            ctx,
            "sidecar returned no usable chunked embeddings",
        );
        return;
    };
    if let Err(err) = align_embedding_dimensions(conn, ctx, dims) {
        fail_note(conn, note, ctx, &err.to_string());
        return;
    }
    if ctx.dimension_mismatch {
        fail_note(
            conn,
            note,
            ctx,
            &format!(
                "embedding dimension mismatch (expected {expected}, got {dims}); semantic search disabled — run `talon embed --force` to rebuild at the new dimensionality",
                expected = ctx.current_dimensions.unwrap_or(0)
            ),
        );
        return;
    }
    if let Err(err) = persist_multi_chunk(conn, options, note, batch, dims) {
        fail_note(conn, note, ctx, &err.to_string());
        return;
    }
    ctx.succeeded = ctx.succeeded.saturating_add(1);
}

fn persist_multi_chunk(
    conn: &Connection,
    options: &EmbedPassOptions,
    note: &NoteWithChunks,
    batch: &EmbedChunkedDataItem,
    dims: u32,
) -> Result<(), TalonError> {
    if batch.embeddings.len() != note.chunks.len() {
        return Err(TalonError::Internal {
            message: format!(
                "chunked response length {got} != note chunks {expected}",
                got = batch.embeddings.len(),
                expected = note.chunks.len()
            ),
        });
    }
    let now = now_ms();
    for (chunk, embedding) in note.chunks.iter().zip(batch.embeddings.iter()) {
        persist_chunk_vector(
            conn,
            chunk.chunk_id,
            &options.document_embedding_model,
            dims,
            now,
            embedding,
        )?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod wiremock_tests;
