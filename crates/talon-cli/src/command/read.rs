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
    PositiveCount, ReadInput, ResponseMeta, TalonEnvelope, TalonResponseData, open_database,
    run_read,
};

pub(super) async fn emit(args: &CliArgs, rest: &[String]) -> Result<()> {
    if rest.is_empty() {
        bail!("read requires a path");
    }

    let path = rest[0].clone();
    let from_line = args
        .from_line
        .map(|n| PositiveCount::new(n, "from-line"))
        .transpose()?;
    let max_lines = args
        .max_lines
        .map(|n| PositiveCount::new(n, "max-lines"))
        .transpose()?;
    let raw = args.raw.enabled();

    let input = ReadInput {
        path: Some(path),
        raw,
        from_line,
        max_lines,
    };

    let config = config::load_config(args.config_file.as_deref())?;
    let db_path: PathBuf = config.db_path.clone();
    let vault_root: PathBuf = config.vault_path.clone();

    let started = Instant::now();
    let work = async move {
        let conn = open_database(&db_path)
            .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;

        let response = run_read(&conn, &vault_root, &input);
        let result_count = response.results.iter().filter(|r| r.found).count();

        let meta = ResponseMeta {
            duration_ms: elapsed_ms(started),
            result_count: Some(count_u32(result_count)),
            warnings: Vec::new(),
            scope_set: None,
            since: None,
        };
        let data = TalonResponseData::Read(response);
        Ok::<TalonEnvelope, eyre::Report>(TalonEnvelope::ok("read", data, meta))
    };
    let response = if should_spin(args) {
        spinner::with_spinner("Reading...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(args))
}
