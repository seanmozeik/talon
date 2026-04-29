use super::{output_mode, should_spin};
use crate::cli::{ChangesArgs, Cli};
use crate::config::{self, vault_container_path};
use crate::output::emit_response;
use crate::spinner;
use crate::telemetry::{count_u32, elapsed_ms};
use eyre::{Result, WrapErr as _};
use std::path::PathBuf;
use std::time::Instant;
use talon_core::{
    PositiveCount, ResponseMeta, ScopeFilter, TalonEnvelope, TalonResponseData, open_database,
    query_changes,
};

pub(super) async fn emit(args: &ChangesArgs, cli: &Cli) -> Result<()> {
    let since_str = args.since.clone();

    let input = talon_core::ChangesInput {
        since: args.since.clone(),
        scope: args.scope.scope.clone(),
        scope_only: args.scope.scope_only.clone(),
        scope_all: args.scope.scope_all,
        limit: PositiveCount::new(
            args.limit.unwrap_or(talon_core::constants::DEFAULT_LIMIT),
            "limit",
        )?,
    };

    let config = config::load_config(cli.config_file.as_deref())?;
    let db_path: PathBuf = config.db_path.clone();
    let vault = vault_container_path(Some(&config));
    let scope_set = Some(
        ScopeFilter::from_args(&config, &input.scope, &input.scope_only, input.scope_all)
            .map_err(|e| eyre::eyre!("{e}"))?
            .resolved_set(),
    );

    let fast = cli.fast;
    let started = Instant::now();
    let work = async move {
        let mut conn = open_database(&db_path)
            .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;
        crate::config::refresh_index_if_needed(&config, &mut conn, fast)?;
        let mut response = query_changes(&conn, &input, Some(&config));
        response.vault = vault;
        let result_count =
            count_u32(response.added.len() + response.modified.len() + response.deleted.len());
        let meta = ResponseMeta {
            duration_ms: elapsed_ms(started),
            result_count: Some(result_count),
            warnings: Vec::new(),
            scope_set,
            since: Some(since_str),
        };
        let data = TalonResponseData::Changes(response);
        Ok::<TalonEnvelope, eyre::Report>(TalonEnvelope::ok("changes", data, meta))
    };
    let response = if should_spin(cli) {
        spinner::with_spinner("Fetching changes...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(cli))
}
