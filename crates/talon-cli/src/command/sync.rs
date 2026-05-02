use super::{output_mode, should_spin};
use crate::cli::{Cli, SyncArgs};
use crate::config;
use crate::output::emit_response;
use crate::spinner;
use crate::telemetry::{count_u32, elapsed_ms};
use eyre::{Result, WrapErr as _};
use std::path::{Path, PathBuf};
use std::time::Instant;
use talon_core::{
    IndexerConfig, ResponseMeta, SyncInput, SyncResponse, SyncStatus, TalonConfig, TalonEnvelope,
    TalonResponseData, acquire_sync_lock, embed::EmbedPassOptions, inference::InferenceClient,
    open_database, vec_ext::register_sqlite_vec,
};

pub(super) async fn emit(args: &SyncArgs, cli: &Cli) -> Result<()> {
    let input = SyncInput {
        paths: args.paths.clone(),
        fast: cli.fast,
        force: args.force,
        rebuild: args.rebuild,
    };
    let config = config::load_config(cli.config_file.as_deref())?;
    let vault_path: PathBuf = config.vault_path.clone();
    let db_path: PathBuf = config.db_path.clone();
    let lock_path: PathBuf = db_path
        .parent()
        .map_or_else(|| PathBuf::from("sync.lock"), |p| p.join("sync.lock"));
    let indexer_config = IndexerConfig {
        include_patterns: config.include_patterns.clone(),
        ignore_patterns: config.ignore_patterns.clone(),
        graph_suggester: if input.fast {
            None
        } else {
            talon_core::GraphSuggestionClient::from_config(&config)
                .wrap_err("building graph suggestion client")?
        },
        talon_config: Some(config.clone()),
    };

    let work = async move {
        let started = Instant::now();
        let result = tokio::task::spawn_blocking(move || {
            run_sync_blocking(
                &input,
                &config,
                &vault_path,
                &db_path,
                &lock_path,
                &indexer_config,
                started,
            )
        })
        .await
        .wrap_err("sync task join failed")?;
        let sync_resp = result?;
        let meta = ResponseMeta {
            duration_ms: sync_resp.duration_ms,
            result_count: Some(sync_resp.indexed),
            warnings: Vec::new(),
            scope_set: None,
            since: None,
        };
        let data = TalonResponseData::Sync(sync_resp);
        Ok::<TalonEnvelope, eyre::Report>(TalonEnvelope::ok("sync", data, meta))
    };
    let response = if should_spin(cli) {
        spinner::with_spinner("Syncing...".to_string(), work).await?
    } else {
        work.await?
    };
    if crate::banner::should_clear_fancy_prelude(cli) {
        crate::banner::clear_fancy_prelude();
    }
    emit_response(&response, output_mode(cli))
}

fn run_sync_blocking(
    input: &SyncInput,
    config: &TalonConfig,
    vault_path: &Path,
    db_path: &Path,
    lock_path: &Path,
    indexer_config: &IndexerConfig,
    started: Instant,
) -> Result<SyncResponse> {
    register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
    let lock = acquire_sync_lock(lock_path).wrap_err("acquiring sync lock")?;
    if input.rebuild {
        talon_core::remove_index_files(db_path)
            .wrap_err_with(|| format!("removing index files for {}", db_path.display()))?;
    }
    let mut conn = open_database(db_path)
        .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;
    let (embed_opts, inference) = embed_options(input, config)?;
    let (stats, embed_stats) = talon_core::run_sync_with_chunker_locked(
        &mut conn,
        vault_path,
        indexer_config,
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

    Ok(SyncResponse {
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
    })
}

fn embed_options(
    input: &SyncInput,
    config: &TalonConfig,
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
