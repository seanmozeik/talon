use super::{output_mode, should_spin};
use crate::cli::CliArgs;
use crate::config;
use crate::output::emit_response;
use crate::spinner;
use crate::telemetry::{count_u32, elapsed_ms};
use eyre::{Result, WrapErr as _, bail};
use std::path::PathBuf;
use std::time::Instant;
use talon_core::{
    RelatedInput, ResponseMeta, TalonEnvelope, TalonResponseData, find_related, open_database,
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
        scope: vec![],
        scope_only: vec![],
    };

    let config = config::load_config(args.config_file.as_deref())?;
    let db_path: PathBuf = config.db_path.clone();

    let started = Instant::now();
    let work = async move {
        let conn = open_database(&db_path)
            .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;

        let response = find_related(&conn, &input);
        let result_count = response.results.len();

        let meta = ResponseMeta {
            duration_ms: elapsed_ms(started),
            result_count: Some(count_u32(result_count)),
            warnings: Vec::new(),
            scope_set: None,
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
