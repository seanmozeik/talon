use super::{output_mode, should_spin};
use crate::cli::{CliArgs, parse_where_clause};
use crate::config;
use crate::output::emit_response;
use crate::spinner;
use crate::telemetry::{count_u32, elapsed_ms};
use eyre::{Result, WrapErr as _};
use std::path::PathBuf;
use std::time::Instant;
use talon_core::{
    MetaInput, PositiveCount, ResponseMeta, TalonEnvelope, TalonResponseData, open_database,
    query_meta,
};

pub(super) async fn emit(args: &CliArgs) -> Result<()> {
    let where_clauses: Vec<talon_core::WhereClause> = args
        .where_clauses
        .iter()
        .map(|s| parse_where_clause(s).map_err(|e| eyre::eyre!("invalid --where: {s}: {e}")))
        .collect::<Result<Vec<_>>>()?;

    let input = MetaInput {
        where_: where_clauses,
        since: args.since.clone(),
        scope: vec![],
        scope_only: vec![],
        select: args.meta.select.clone(),
        tag_counts: args.meta.tag_counts,
        sources: args.meta.sources.clone(),
        limit: PositiveCount::new(
            args.limit.unwrap_or(talon_core::constants::DEFAULT_LIMIT),
            "limit",
        )?,
    };

    let config = config::load_config(args.config_file.as_deref())?;
    let db_path: PathBuf = config.db_path.clone();
    let since_str = input.since.clone();

    let started = Instant::now();
    let work = async move {
        let conn = open_database(&db_path)
            .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;

        let response = query_meta(&conn, &input);
        let result_count = count_u32(response.entries.len());

        let meta = ResponseMeta {
            duration_ms: elapsed_ms(started),
            result_count: Some(result_count),
            warnings: Vec::new(),
            scope_set: None,
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
