use super::{output_mode, should_spin};
use crate::cli::{Cli, RelatedArgs};
use crate::config::{self, RefreshLockPolicy, vault_container_path};
use crate::output::emit_response;
use crate::spinner;
use crate::telemetry::{count_u32, elapsed_ms};
use eyre::{Result, WrapErr as _};
use std::path::PathBuf;
use std::time::Instant;
use talon_core::{
    Direction, RelatedInput, ResponseMeta, ScopeFilter, TalonEnvelope, TalonResponseData,
    find_related, open_database, open_database_read_only,
};

pub(super) async fn emit(args: &RelatedArgs, cli: &Cli) -> Result<()> {
    let direction = args
        .direction
        .map_or_else(Direction::default, std::convert::Into::into);
    let depth = args
        .depth
        .unwrap_or(talon_core::constants::RELATED_DEFAULT_DEPTH);

    let input = RelatedInput {
        path: args.path.clone(),
        depth,
        direction,
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

    let fast = cli.fast;
    let started = Instant::now();
    let work = async move {
        let mut conn = if fast {
            open_database_read_only(&db_path)
        } else {
            open_database(&db_path)
        }
        .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;
        crate::config::refresh_index_if_needed(
            &config,
            &mut conn,
            fast,
            RefreshLockPolicy::ErrorIfBusy,
        )?;

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
    let response = if should_spin(cli) {
        spinner::with_spinner("Finding related...".to_string(), work).await?
    } else {
        work.await?
    };
    if crate::banner::should_clear_fancy_prelude(cli) {
        crate::banner::clear_fancy_prelude();
    }
    emit_response(&response, output_mode(cli))
}
