use super::{output_mode, should_spin};
use crate::cli::{CliArgs, parse_where_clause};
use crate::config::{self, vault_container_path};
use crate::output::emit_response;
use crate::spinner;
use crate::telemetry::elapsed_ms;
use eyre::{Result, WrapErr as _, bail};
use std::path::PathBuf;
use std::time::Instant;
use talon_core::{
    ExpansionClient, PositiveCount, ResponseMeta, SearchInput, SearchMode, TalonEnvelope,
    TalonResponseData, inference::InferenceClient, open_database, run_search,
    vec_ext::register_sqlite_vec,
};

pub(super) async fn emit(args: &CliArgs, rest: &[String]) -> Result<()> {
    if rest.is_empty() {
        bail!("search requires a query");
    }

    let query = rest.join(" ");
    let mode = args.mode.unwrap_or_default();
    let fast = args.fast.enabled();
    let limit = args.limit;
    // TODO(US-025): consult talon.toml [search].candidate_limit before falling back to CANDIDATE_FLOOR
    let candidate_limit = args.candidate_limit;

    let where_clauses: Vec<talon_core::WhereClause> = args
        .where_clauses
        .iter()
        .map(|s| parse_where_clause(s).map_err(|e| eyre::eyre!("invalid --where: {s}: {e}")))
        .collect::<Result<Vec<_>>>()?;

    let mut input = SearchInput {
        query: Some(query),
        mode,
        fast,
        where_: where_clauses,
        since: args.since.clone(),
        anchors: if args.anchors.enabled() {
            Some(true)
        } else {
            None
        },
        ..SearchInput::default()
    };
    if let Some(n) = limit {
        input.limit = PositiveCount::new(n, "limit")?;
    }
    if let Some(n) = candidate_limit {
        input.candidate_limit = PositiveCount::new(n, "candidate_limit")?;
    }

    let started = Instant::now();
    let config = config::load_config(args.config_file.as_deref()).ok();

    let work = async move {
        tokio::task::spawn_blocking(move || {
            execute_search(input, config.as_ref(), started, fast, mode)
        })
        .await
        .wrap_err("search task join failed")?
    };

    let response = if should_spin(args) {
        spinner::with_spinner("Searching...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(args))
}

fn execute_search(
    input: SearchInput,
    config: Option<&talon_core::TalonConfig>,
    started: Instant,
    fast: bool,
    mode: SearchMode,
) -> Result<TalonEnvelope> {
    let db_path: PathBuf = config
        .as_ref()
        .map_or_else(crate::config::default_db_path, |c| c.db_path.clone());

    register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
    let conn = open_database(&db_path)
        .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;

    let (inference, expansion) =
        if fast || mode == SearchMode::Fulltext || mode == SearchMode::Title {
            (None, None)
        } else {
            let inference_url = config.as_ref().map_or_else(
                || "http://localhost:8080".to_string(),
                |c| c.inference.base_url.clone(),
            );
            let inference = InferenceClient::new(inference_url)
                .wrap_err("building inference client")
                .ok();
            let expansion = config
                .as_ref()
                .map(|c| {
                    ExpansionClient::with_max_tokens(
                        c.expansion.base_url.clone(),
                        &c.expansion.model,
                        c.expansion.max_tokens,
                    )
                })
                .transpose()
                .ok()
                .flatten();
            (inference, expansion)
        };

    let mut response = run_search(
        &conn,
        &input,
        inference.as_ref(),
        expansion.as_ref(),
        config,
    );
    response.vault = vault_container_path(config);

    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: Some(response.total),
        warnings: Vec::new(),
        scope_set: config
            .as_ref()
            .map(|c| c.default_scope_names().into_iter().cloned().collect()),
        since: input.since,
    };
    Ok(TalonEnvelope::ok(
        "search",
        TalonResponseData::Search(response),
        meta,
    ))
}
