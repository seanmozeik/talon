use super::{output_mode, should_spin};
use crate::cli::CliArgs;
use crate::config;
use crate::output::emit_response;
use crate::spinner;
use eyre::{Result, WrapErr as _};
use std::path::PathBuf;
use std::time::Instant;
use talon_core::{
    PositiveCount, ResponseMeta, TalonEnvelope, TalonResponseData, open_database, query_changes,
};

pub(super) async fn emit(args: &CliArgs) -> Result<()> {
    let since = args
        .since
        .clone()
        .ok_or_else(|| eyre::eyre!("changes requires --since <timestamp>"))?;
    let since_str = since.clone();

    let input = talon_core::ChangesInput {
        since,
        scope: Vec::new(),
        scope_only: Vec::new(),
        limit: PositiveCount::new(
            args.limit.unwrap_or(talon_core::constants::DEFAULT_LIMIT),
            "limit",
        )?,
    };

    let config = config::load_config(args.config_file.as_deref())?;
    let db_path: PathBuf = config.db_path.clone();

    let started = Instant::now();
    let work = async move {
        let conn = open_database(&db_path)
            .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;
        let response = query_changes(&conn, &input);
        let result_count =
            u32::try_from(response.added.len() + response.modified.len() + response.deleted.len())
                .unwrap_or(u32::MAX);
        let meta = ResponseMeta {
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            result_count: Some(result_count),
            warnings: Vec::new(),
            scope_set: None,
            since: Some(since_str),
        };
        let data = TalonResponseData::Changes(response);
        Ok::<TalonEnvelope, eyre::Report>(TalonEnvelope::ok("changes", data, meta))
    };
    let response = if should_spin(args) {
        spinner::with_spinner("Fetching changes...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(args))
}
