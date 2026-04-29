use super::{output_mode, should_spin};
use crate::cli::{Cli, LintArgs, LintCheck};
use crate::config::{self, RefreshLockPolicy, refresh_index_if_needed, vault_container_path};
use crate::output::emit_response;
use crate::spinner;
use crate::telemetry::{count_u32, elapsed_ms};
use eyre::{Result, WrapErr as _};
use std::path::PathBuf;
use std::time::Instant;
use talon_core::{
    LintInput, ResponseMeta, ScopeFilter, TalonEnvelope, TalonResponseData, open_database,
    query_lint, vec_ext::register_sqlite_vec,
};

pub(super) async fn emit(args: &LintArgs, cli: &Cli) -> Result<()> {
    let check = args.check.unwrap_or(LintCheck::All).into();

    let input = LintInput {
        check,
        scope: args.scope.scope.clone(),
        scope_only: args.scope.scope_only.clone(),
        scope_all: args.scope.scope_all,
    };

    let config = config::load_config(cli.config_file.as_deref())?;
    let db_path: PathBuf = config.db_path.clone();
    let vault = vault_container_path(Some(&config));
    let scope_set = Some(
        ScopeFilter::from_args(&config, &input.scope, &input.scope_only, input.scope_all)
            .map_err(|e| eyre::eyre!("{e}"))?
            .resolved_set(),
    );

    let started = Instant::now();
    let work = async move {
        register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
        let mut conn = open_database(&db_path)
            .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;
        // Lint always refreshes — findings must reflect current vault state.
        // `false` for `fast`: lint never opts out of refresh.
        refresh_index_if_needed(&config, &mut conn, false, RefreshLockPolicy::ErrorIfBusy)?;

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
    let response = if should_spin(cli) {
        spinner::with_spinner("Running lint...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(cli))
}
