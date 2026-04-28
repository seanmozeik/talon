use super::{output_mode, should_spin};
use crate::cli::CliArgs;
use crate::config::{self, vault_container_path};
use crate::output::emit_response;
use crate::spinner;
use crate::telemetry::{count_u32, elapsed_ms};
use eyre::{Result, WrapErr as _, bail};
use std::path::PathBuf;
use std::time::Instant;
use talon_core::{
    RelatedInput, ResponseMeta, ScopeFilter, TalonEnvelope, TalonResponseData, find_related,
    open_database,
};

pub(super) async fn emit(args: &CliArgs, rest: &[String]) -> Result<()> {
    if rest.is_empty() {
        bail!("related requires a path");
    }

    let input = RelatedInput {
        path: rest[0].clone(),
        depth: args
            .depth
            .unwrap_or(talon_core::constants::RELATED_DEFAULT_DEPTH),
        direction: args.direction.unwrap_or_default(),
        scope: args.scope.scope.clone(),
        scope_only: args.scope.scope_only.clone(),
        scope_all: args.scope.scope_all,
    };

    let config = config::load_config(args.config_file.as_deref())?;
    let db_path: PathBuf = config.db_path.clone();
    let vault = vault_container_path(Some(&config));
    let scope_set = Some(
        ScopeFilter::from_args(&config, &input.scope, &input.scope_only, input.scope_all)
            .map_err(|e| eyre::eyre!("{e}"))?
            .resolved_set(),
    );

    let fast = args.fast.enabled();
    let started = Instant::now();
    let work = async move {
        let mut conn = open_database(&db_path)
            .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;
        crate::config::refresh_index_if_needed(&config, &mut conn, fast)?;

        let mut response = find_related(&conn, &input, Some(&config));
        response.vault = vault;
        let result_count = response.results.len();

        let meta = ResponseMeta {
            duration_ms: elapsed_ms(started),
            result_count: Some(count_u32(result_count)),
            warnings: Vec::new(),
            scope_set,
            since: None,
        };
        let data = TalonResponseData::Related(response);
        Ok::<TalonEnvelope, eyre::Report>(TalonEnvelope::ok("related", data, meta))
    };
    let response = if should_spin(args) {
        spinner::with_spinner("Finding related...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(args))
}
