use super::{output_mode, should_spin};
use crate::cli::CliArgs;
use crate::config;
use crate::output::emit_response;
use crate::spinner;
use crate::telemetry::{count_u32, elapsed_ms};
use eyre::{Result, WrapErr as _};
use std::path::PathBuf;
use std::time::Instant;
use talon_core::{
    IndexerConfig, ResponseMeta, SyncInput, SyncResponse, SyncStatus, TalonEnvelope,
    TalonResponseData, embed::EmbedPassOptions, inference::InferenceClient, open_database,
    vec_ext::register_sqlite_vec,
};

pub(super) async fn emit(args: &CliArgs, rest: &[String]) -> Result<()> {
    let input = SyncInput {
        paths: rest.to_vec(),
        fast: args.fast.enabled(),
        force: args.force.enabled(),
        no_wait: false,
    };
    let config = config::load_config(args.config_file.as_deref())?;
    let vault_path: PathBuf = config.vault_path.clone();
    let db_path: PathBuf = config.db_path.clone();
    let lock_path: PathBuf = db_path
        .parent()
        .map_or_else(|| PathBuf::from("sync.lock"), |p| p.join("sync.lock"));
    let indexer_config = IndexerConfig {
        include_patterns: config.include_patterns.clone(),
        ignore_patterns: config.ignore_patterns.clone(),
    };

    let work = async move {
        let started = Instant::now();
        let path_count = count_u32(input.paths.len());
        let result = tokio::task::spawn_blocking(move || -> Result<SyncResponse> {
            register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
            let mut conn = open_database(&db_path)
                .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;

            let (embed_opts, inference) = if input.fast {
                (None, None::<InferenceClient>)
            } else {
                let opts = EmbedPassOptions {
                    force: input.force,
                    restrict_paths: input.paths.clone(),
                    chunk_embedding_model: config.inference.models.chunk_embedding.clone(),
                    document_embedding_model: config.inference.models.document_embedding.clone(),
                };
                let client = InferenceClient::new(&config.inference.base_url)
                    .wrap_err("building inference client")?;
                (Some(opts), Some(client))
            };

            let (stats, embed_stats) = talon_core::run_sync_with_chunker(
                &mut conn,
                &vault_path,
                &lock_path,
                &indexer_config,
                embed_opts,
                inference.as_ref(),
                &config.chunker,
            )
            .wrap_err("sync failed")?;

            let (embedded, embed_failed, dimension_mismatch, embed_remediation, embed_diagnostics) =
                embed_stats.map_or((0, 0, false, None, Vec::new()), |s| {
                    (
                        s.succeeded,
                        s.failed,
                        s.dimension_mismatch,
                        s.remediation,
                        s.diagnostics,
                    )
                });

            Ok(SyncResponse {
                completed: true,
                status: SyncStatus::Ok,
                fast: input.fast,
                force: input.force,
                path_count,
                indexed: stats.indexed,
                skipped: stats.skipped,
                deleted: stats.deleted,
                embedded,
                embed_failed,
                dimension_mismatch,
                embed_remediation,
                embed_diagnostics,
                duration_ms: elapsed_ms(started),
            })
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
    let response = if should_spin(args) {
        spinner::with_spinner("Syncing...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(args))
}
