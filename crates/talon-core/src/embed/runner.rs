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
mod tests {
    use super::*;
    use crate::embed::EmbedDiagnostics;

    #[test]
    fn embed_pass_stats_from_diagnostics_passthrough() {
        let stats: EmbedPassStats = EmbedDiagnostics {
            processed: 3,
            succeeded: 2,
            failed: 1,
            dimension_mismatch: false,
            diagnostics: vec!["a".into(), "b".into()],
        }
        .into();
        assert_eq!(stats.processed, 3);
        assert_eq!(stats.succeeded, 2);
        assert_eq!(stats.failed, 1);
        assert!(stats.remediation.is_none());
        assert_eq!(stats.diagnostics, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn dimension_mismatch_populates_remediation() {
        let stats: EmbedPassStats = EmbedDiagnostics {
            processed: 2,
            succeeded: 1,
            failed: 1,
            dimension_mismatch: true,
            diagnostics: vec!["dim mismatch".into()],
        }
        .into();
        assert!(stats.dimension_mismatch);
        let remediation = stats.remediation.as_deref().unwrap_or("");
        assert!(remediation.contains("--force"));
        assert!(remediation.contains("vec_chunks") || remediation.contains("dimensionality"));
    }

    #[test]
    fn embed_pass_options_defaults_use_sidecar_model_ids() {
        let opts = EmbedPassOptions::defaults();
        assert_eq!(opts.chunk_embedding_model, "embed");
        assert_eq!(opts.document_embedding_model, "embed_chunked");
        assert!(!opts.force);
        assert!(opts.restrict_paths.is_empty());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod wiremock_tests {
    //! End-to-end runner tests against an in-process mock sidecar. These
    //! prove the single-chunk and multi-chunk paths persist correctly,
    //! including dimension pinning and the JSON wire shapes from the
    //! Python sidecar.
    //!
    //! The tests block on `tokio::runtime::Runtime` directly because the
    //! runner itself is sync — the runtime only exists to host wiremock.

    use super::*;
    use crate::store::open_database;
    use crate::vec_ext::register_sqlite_vec;
    use rusqlite::params;
    use serde_json::json;
    use std::env::temp_dir;
    use std::sync::atomic::{AtomicU64, Ordering};
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn unique_path(label: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        temp_dir().join(format!("talon-runner-test-{label}-{pid}-{n}.sqlite"))
    }

    fn cleanup(p: &std::path::Path) {
        let _ = fs_err::remove_file(p);
        let _ = fs_err::remove_file(p.with_extension("sqlite-wal"));
        let _ = fs_err::remove_file(p.with_extension("sqlite-shm"));
    }

    fn seed_note(conn: &Connection, vault_path: &str, chunks: &[&str]) {
        conn.execute(
            "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
             VALUES (?, ?, '[]', '[]', '', 0, 0, 'h', 'd', 1)",
            params![vault_path, vault_path],
        ).unwrap();
        let note_id = conn.last_insert_rowid();
        for (i, text) in chunks.iter().enumerate() {
            conn.execute(
                "INSERT INTO chunks (note_id, chunk_index, text, embedding_text, heading_path, char_start, char_end, chunk_hash, token_estimate, embedding_status)
                 VALUES (?, ?, ?, ?, '', 0, 0, ?, 1, 'pending')",
                params![note_id, i64::try_from(i).unwrap(), text, text, format!("h{i}")],
            ).unwrap();
        }
    }

    #[test]
    fn single_chunk_path_persists_vector_and_marks_ok() {
        register_sqlite_vec().unwrap();
        let db = unique_path("single");
        let conn = open_database(&db).unwrap();
        seed_note(&conn, "single.md", &["hello world"]);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(
            Mock::given(method("POST"))
                .and(path("/embed"))
                .and(body_partial_json(json!({"inputs": ["hello world"]})))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!([[0.1, 0.2, 0.3, 0.4]])),
                )
                .mount(&server),
        );

        let client = InferenceClient::new(server.uri()).unwrap();
        let stats = run_embed_pass(&conn, &client, &EmbedPassOptions::defaults()).unwrap();
        assert_eq!(stats.processed, 1);
        assert_eq!(stats.succeeded, 1);
        assert_eq!(stats.failed, 0);
        assert!(!stats.dimension_mismatch);

        let dims: i64 = conn
            .query_row("SELECT dimensions FROM vector_metadata LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(dims, 4);
        let chunk_status: String = conn
            .query_row(
                "SELECT embedding_status FROM chunks WHERE chunk_index = 0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(chunk_status, "ok");

        drop(conn);
        cleanup(&db);
    }

    #[test]
    fn multi_chunk_path_persists_each_chunk() {
        register_sqlite_vec().unwrap();
        let db = unique_path("multi");
        let conn = open_database(&db).unwrap();
        seed_note(&conn, "multi.md", &["alpha", "beta", "gamma"]);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(
            Mock::given(method("POST"))
                .and(path("/embed-chunked"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [{
                        "embeddings": [
                            [0.1, 0.2, 0.3],
                            [0.4, 0.5, 0.6],
                            [0.7, 0.8, 0.9],
                        ],
                        "index": 0,
                    }],
                    "model": "embed_chunked",
                })))
                .mount(&server),
        );

        let client = InferenceClient::new(server.uri()).unwrap();
        let stats = run_embed_pass(&conn, &client, &EmbedPassOptions::defaults()).unwrap();
        assert_eq!(stats.succeeded, 1);
        assert_eq!(stats.failed, 0);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM vector_metadata", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3);
        drop(conn);
        cleanup(&db);
    }

    #[test]
    fn http_error_marks_note_failed_and_records_diagnostic() {
        register_sqlite_vec().unwrap();
        let db = unique_path("err");
        let conn = open_database(&db).unwrap();
        seed_note(&conn, "bad.md", &["one"]);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(
            Mock::given(method("POST"))
                .and(path("/embed"))
                .respond_with(ResponseTemplate::new(500).set_body_string("upstream model OOM"))
                .mount(&server),
        );

        let client = InferenceClient::new(server.uri()).unwrap();
        let stats = run_embed_pass(&conn, &client, &EmbedPassOptions::defaults()).unwrap();
        assert_eq!(stats.processed, 1);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.succeeded, 0);
        assert_eq!(stats.diagnostics.len(), 1);
        let chunk_status: String = conn
            .query_row(
                "SELECT embedding_status FROM chunks WHERE chunk_index = 0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(chunk_status, "failed");

        drop(conn);
        cleanup(&db);
    }

    #[test]
    fn dimension_mismatch_is_reported() {
        register_sqlite_vec().unwrap();
        let db = unique_path("dim");
        let conn = open_database(&db).unwrap();
        seed_note(&conn, "first.md", &["a"]);
        seed_note(&conn, "second.md", &["b"]);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = runtime.block_on(MockServer::start());
        // Two requests, two different shapes — second one trips the mismatch.
        runtime.block_on(
            Mock::given(method("POST"))
                .and(path("/embed"))
                .and(body_partial_json(json!({"inputs": ["a"]})))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!([[0.1, 0.2, 0.3, 0.4]])),
                )
                .mount(&server),
        );
        runtime.block_on(
            Mock::given(method("POST"))
                .and(path("/embed"))
                .and(body_partial_json(json!({"inputs": ["b"]})))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([[0.1, 0.2]])))
                .mount(&server),
        );

        let client = InferenceClient::new(server.uri()).unwrap();
        let stats = run_embed_pass(&conn, &client, &EmbedPassOptions::defaults()).unwrap();
        assert!(stats.dimension_mismatch);
        assert_eq!(stats.succeeded, 1);
        assert_eq!(stats.failed, 1);

        drop(conn);
        cleanup(&db);
    }
}
