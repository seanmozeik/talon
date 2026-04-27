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
    LintCheck, LintInput, ResponseMeta, ScopeFilter, TalonEnvelope, TalonResponseData,
    open_database, query_lint,
};

pub(super) async fn emit(args: &CliArgs, rest: &[String]) -> Result<()> {
    let check = if let Some(c) = rest.first() {
        match c.as_str() {
            "all" => LintCheck::All,
            "orphans" => LintCheck::Orphans,
            "broken-links" => LintCheck::BrokenLinks,
            "dangling-refs" => LintCheck::DanglingRefs,
            "unreferenced" => LintCheck::Unreferenced,
            other => bail!(
                "unknown lint check: {other}; try all, orphans, broken-links, dangling-refs, unreferenced"
            ),
        }
    } else {
        LintCheck::All
    };

    let input = LintInput {
        check,
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

    let started = Instant::now();
    let work = async move {
        let conn = open_database(&db_path)
            .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;
        let mut response = query_lint(&conn, &input, Some(&config));
        response.vault = vault;
        let result_count = count_u32(response.findings.len());
        let meta = ResponseMeta {
            duration_ms: elapsed_ms(started),
            result_count: Some(result_count),
            warnings: Vec::new(),
            scope_set,
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
