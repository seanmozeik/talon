use super::{output_mode, should_spin};
use crate::cli::{CliArgs, parse_where_clause};
use crate::config::{self, vault_container_path};
use crate::output::emit_response;
use crate::spinner;
use crate::telemetry::{count_u32, elapsed_ms};
use eyre::{Result, WrapErr as _};
use std::path::PathBuf;
use std::time::Instant;
use talon_core::{
    MetaInput, PositiveCount, ResponseMeta, ScopeFilter, TalonEnvelope, TalonResponseData,
    open_database, query_meta,
};

pub(super) async fn emit(args: &CliArgs) -> Result<()> {
    let where_clauses: Vec<talon_core::WhereClause> = args
        .where_clauses
        .iter()
        .map(|s| parse_where_clause(s).map_err(|e| eyre::eyre!("invalid --where: {s}: {e}")))
        .collect::<Result<Vec<_>>>()?;

    // When --tag-counts is set, the per-path entries list is incidental to the
    // global tag dictionary. Default to a much higher cap so the agent isn't
    // silently handed a 10-of-N slice; explicit --limit still wins.
    let limit_default = if args.meta.tag_counts {
        u16::MAX
    } else {
        talon_core::constants::DEFAULT_LIMIT
    };
    let input = MetaInput {
        where_: where_clauses,
        since: args.since.clone(),
        scope: args.scope.scope.clone(),
        scope_only: args.scope.scope_only.clone(),
        scope_all: args.scope.scope_all,
        select: args.meta.select.clone(),
        tag_counts: args.meta.tag_counts,
        sources: args.meta.sources.clone(),
        limit: PositiveCount::new(args.limit.unwrap_or(limit_default), "limit")?,
    };

    let config = config::load_config(args.config_file.as_deref())?;
    let db_path: PathBuf = config.db_path.clone();
    let vault = vault_container_path(Some(&config));
    let since_str = input.since.clone();
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

        let mut response = query_meta(&conn, &input, Some(&config));
        response.vault = vault;
        let result_count = count_u32(response.entries.len());

        let meta = ResponseMeta {
            duration_ms: elapsed_ms(started),
            result_count: Some(result_count),
            warnings: Vec::new(),
            scope_set,
            since: since_str,
        };
        let data = TalonResponseData::Meta(response);
        Ok::<TalonEnvelope, eyre::Report>(TalonEnvelope::ok("meta", data, meta))
    };
    let response = if should_spin(args) {
        spinner::with_spinner("Querying frontmatter...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(args))
}
