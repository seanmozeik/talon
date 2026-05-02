use std::path::PathBuf;
use std::time::Instant;

use color_eyre::eyre::{Result, WrapErr as _};
use talon_core::{
    IndexerConfig, ResponseMeta, SyncInput, SyncResponse, SyncStatus, TalonConfig, TalonEnvelope,
    TalonResponseData, acquire_sync_lock, embed::EmbedPassOptions, inference::InferenceClient,
    open_database, vec_ext::register_sqlite_vec,
};

use crate::config;
use crate::telemetry::{count_u32, elapsed_ms};

pub(super) fn dispatch_sync(input: &SyncInput) -> Result<TalonEnvelope> {
    let started = Instant::now();
    let config = config::load_config(None)?;
    let vault_path: PathBuf = config.vault_path.clone();
    let db_path: PathBuf = config.db_path.clone();
    let lock_path: PathBuf = db_path.parent().map_or_else(
        || PathBuf::from("sync.lock"),
        |parent| parent.join("sync.lock"),
    );
    let indexer_config = IndexerConfig {
        include_patterns: config.include_patterns.clone(),
        ignore_patterns: config.ignore_patterns.clone(),
        talon_config: Some(config.clone()),
    };

    register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
    let lock = acquire_sync_lock(&lock_path).wrap_err("acquiring sync lock")?;
    if input.rebuild {
        talon_core::remove_index_files(&db_path)
            .wrap_err_with(|| format!("removing index files for {}", db_path.display()))?;
    }
    let mut conn = open_database(&db_path)
        .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;
    let (embed_opts, inference) = embed_options(&config, input)?;
    let (stats, embed_stats) = talon_core::run_sync_with_chunker_locked(
        &mut conn,
        &vault_path,
        &indexer_config,
        embed_opts,
        inference.as_ref(),
        &config.chunker,
        lock,
    )
    .wrap_err("sync failed")?;
    let (embedded, embed_failed, dimension_mismatch, embed_remediation, embed_diagnostics) =
        embed_stats.map_or((0, 0, false, None, Vec::new()), |stats| {
            (
                stats.succeeded,
                stats.failed,
                stats.dimension_mismatch,
                stats.remediation,
                stats.diagnostics,
            )
        });
    let response = SyncResponse {
        completed: true,
        status: SyncStatus::Ok,
        fast: input.fast,
        force: input.force,
        rebuild: input.rebuild,
        path_count: count_u32(input.paths.len()),
        indexed: stats.indexed,
        skipped: stats.skipped,
        deleted: stats.deleted,
        embedded,
        embed_failed,
        dimension_mismatch,
        embed_remediation,
        embed_diagnostics,
        graph: stats.graph,
        duration_ms: elapsed_ms(started),
    };
    let meta = ResponseMeta {
        duration_ms: response.duration_ms,
        result_count: Some(response.indexed),
        warnings: Vec::new(),
        scope_set: None,
        since: None,
    };
    Ok(TalonEnvelope::ok(
        "sync",
        TalonResponseData::Sync(response),
        meta,
    ))
}

fn embed_options(
    config: &TalonConfig,
    input: &SyncInput,
) -> Result<(Option<EmbedPassOptions>, Option<InferenceClient>)> {
    if input.fast {
        return Ok((None, None));
    }
    let opts = EmbedPassOptions {
        force: input.force,
        restrict_paths: input.paths.clone(),
        chunk_embedding_model: config.inference.models.chunk_embedding.clone(),
        document_embedding_model: config.inference.models.document_embedding.clone(),
    };
    let inference =
        InferenceClient::new(&config.inference.base_url).wrap_err("building inference client")?;
    Ok((Some(opts), Some(inference)))
}
