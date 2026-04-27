use super::{output_mode, should_spin};
use crate::cli::CliArgs;
use crate::config;
use crate::output::emit_response;
use crate::spinner;
use eyre::{Result, WrapErr as _, bail};
use std::path::PathBuf;
use std::time::Instant;
use talon_core::{
    LintCheck, LintInput, ResponseMeta, TalonEnvelope, TalonResponseData, open_database, query_lint,
};

pub(super) async fn emit(args: &CliArgs, rest: &[String]) -> Result<()> {
    let check = if let Some(c) = rest.first() {
        match c.as_str() {
            "orphans" => LintCheck::Orphans,
            "broken-links" => LintCheck::BrokenLinks,
            "dangling-refs" => LintCheck::DanglingRefs,
            "unreferenced" => LintCheck::Unreferenced,
            other => bail!(
                "unknown lint check: {other}; try orphans, broken-links, dangling-refs, unreferenced"
            ),
        }
    } else {
        bail!("lint requires a check type; try orphans, broken-links, dangling-refs, unreferenced");
    };

    let input = LintInput {
        check,
        scope: Vec::new(),
        scope_only: Vec::new(),
    };

    let config = config::load_config(args.config_file.as_deref())?;
    let db_path: PathBuf = config.db_path.clone();

    let started = Instant::now();
    let work = async move {
        let conn = open_database(&db_path)
            .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;
        let response = query_lint(&conn, &input);
        let result_count = u32::try_from(response.findings.len()).unwrap_or(u32::MAX);
        let meta = ResponseMeta {
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            result_count: Some(result_count),
            warnings: Vec::new(),
            scope_set: None,
            since: None,
        };
        let data = TalonResponseData::Lint(response);
        Ok::<TalonEnvelope, eyre::Report>(TalonEnvelope::ok("lint", data, meta))
    };
    let response = if should_spin(args) {
        spinner::with_spinner("Running lint...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(args))
}
