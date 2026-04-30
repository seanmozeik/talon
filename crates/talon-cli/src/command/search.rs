use super::{output_mode, should_spin};
use crate::cli::{Cli, SearchArgs, SharedSearchArgs, parse_where_clause};
use crate::config::{self, vault_container_path};
use crate::output::{OutputMode, RenderOptions, emit_response, format_search_human};
use crate::spinner;
use crate::telemetry::elapsed_ms;
use eyre::{Result, WrapErr as _, bail};
use std::io;
use std::time::Instant;
use talon_core::{
    ExpansionClient, ResponseMeta, ScopeFilter, SearchInput, SearchMode, SyncLockError,
    TalonEnvelope, TalonResponseData, acquire_sync_lock, inference::InferenceClient, open_database,
    open_database_read_only, run_search, run_search_with_expanded_queries,
    vec_ext::register_sqlite_vec,
};

pub(super) async fn emit(args: &SearchArgs, cli: &Cli) -> Result<()> {
    if args.query.is_empty() {
        bail!("search requires a query");
    }

    let query = args.query.join(" ");
    let mode = args
        .shared
        .mode
        .map_or_else(SearchMode::default, std::convert::Into::into);
    let fast = cli.fast;
    let include_expanded_queries = cli.verbose;
    let config = config::load_config(cli.config_file.as_deref())?;

    let input = build_search_input(query, &args.shared, &config, fast)?;

    let started = Instant::now();

    let work = async move {
        tokio::task::spawn_blocking(move || {
            execute_search(
                input,
                &config,
                started,
                fast,
                mode,
                include_expanded_queries,
            )
        })
        .await
        .wrap_err("search task join failed")?
    };

    let response = if should_spin(cli) {
        spinner::with_spinner("Searching...".to_string(), work).await?
    } else {
        work.await?
    };
    if crate::banner::should_clear_fancy_prelude(cli) {
        crate::banner::clear_fancy_prelude();
    }

    // In human mode, handle search output directly so we can thread the
    // compact flag and per-response warnings (e.g. sync-skipped notice).
    if output_mode(cli) == OutputMode::Human
        && let Some(TalonResponseData::Search(resp)) = response.data.as_ref()
    {
        let mut opts = RenderOptions::for_terminal();
        opts.compact = args.shared.compact;
        let warnings = response
            .meta
            .as_ref()
            .map_or(&[][..], |m| m.warnings.as_slice());
        return format_search_human(&mut io::stdout(), resp, opts, warnings);
    }
    emit_response(&response, output_mode(cli))
}

pub(super) fn build_search_input(
    query: String,
    shared: &SharedSearchArgs,
    config: &talon_core::TalonConfig,
    fast: bool,
) -> Result<SearchInput> {
    let mode = shared
        .mode
        .map_or_else(SearchMode::default, std::convert::Into::into);
    let where_clauses: Vec<talon_core::WhereClause> = shared
        .where_
        .iter()
        .map(|s| parse_where_clause(s).map_err(|e| eyre::eyre!("invalid --where: {s}: {e}")))
        .collect::<Result<Vec<_>>>()?;

    let mut input = SearchInput::from_cli_query(
        query,
        shared.intent.clone(),
        mode,
        fast,
        shared.limit,
        shared.candidate_limit,
        Some(&config.search),
    )?;
    input.where_ = where_clauses;
    input.since.clone_from(&shared.since);
    input.anchors = shared.anchors.then_some(true);
    input.scope.clone_from(&shared.scope.scope);
    input.scope_only.clone_from(&shared.scope.scope_only);
    input.scope_all = shared.scope.scope_all;

    ScopeFilter::from_args(config, &input.scope, &input.scope_only, input.scope_all)
        .map_err(|e| eyre::eyre!("{e}"))?;
    Ok(input)
}

pub(super) fn execute_search(
    input: SearchInput,
    config: &talon_core::TalonConfig,
    started: Instant,
    fast: bool,
    mode: SearchMode,
    include_expanded_queries: bool,
) -> Result<TalonEnvelope> {
    register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
    let (conn, sync_skipped) = open_search_database(config, &config.db_path, fast)?;

    let (inference, expansion) =
        if fast || mode == SearchMode::Fulltext || mode == SearchMode::Title {
            (None, None)
        } else {
            talon_core::cache::rerank::configure_capacity(config.search.rerank_cache_size);
            let inference = InferenceClient::with_rerank_options_and_protocol(
                &config.inference.base_url,
                config.search.rerank_batch_size,
                config.search.rerank_max_tokens,
                config.inference.rerank,
            )
            .wrap_err("building inference client")
            .ok();
            let expansion = ExpansionClient::with_max_tokens(
                config.expansion.base_url.clone(),
                &config.expansion.model,
                config.expansion.max_output_tokens,
            )
            .ok();
            (inference, expansion)
        };

    let mut response = if include_expanded_queries {
        run_search_with_expanded_queries(
            &conn,
            &input,
            inference.as_ref(),
            expansion.as_ref(),
            Some(config),
        )
    } else {
        run_search(
            &conn,
            &input,
            inference.as_ref(),
            expansion.as_ref(),
            Some(config),
        )
    };
    response.vault = vault_container_path(Some(config));

    let scope_set =
        ScopeFilter::from_args(config, &input.scope, &input.scope_only, input.scope_all)
            .map_or_else(
                |_| ScopeFilter::default_for(config).resolved_set(),
                |f| f.resolved_set(),
            );
    let mut warnings = Vec::new();
    if sync_skipped {
        warnings.push("sync skipped (index locked by another process)".to_string());
    }
    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: Some(response.total),
        warnings,
        scope_set: Some(scope_set),
        since: input.since,
    };
    Ok(TalonEnvelope::ok(
        "search",
        TalonResponseData::Search(response),
        meta,
    ))
}

/// Returns `(connection, sync_skipped)` where `sync_skipped` is true when the
/// index lock was held by another process and auto-refresh was bypassed.
fn open_search_database(
    config: &talon_core::TalonConfig,
    db_path: &std::path::Path,
    fast: bool,
) -> Result<(talon_core::Connection, bool)> {
    if fast {
        let conn = open_database_read_only(db_path)
            .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;
        return Ok((conn, false));
    }

    let lock_path = crate::config::sync_lock_path(config);
    match acquire_sync_lock(&lock_path) {
        Ok(lock) => {
            let mut conn = open_database(db_path)
                .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;
            crate::config::refresh_index_with_lock(config, &mut conn, lock)?;
            Ok((conn, false))
        }
        Err(SyncLockError::Busy) => {
            let conn = open_database_read_only(db_path)
                .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;
            Ok((conn, true))
        }
        Err(SyncLockError::Io(err)) => Err(err).wrap_err("acquiring sync lock for search"),
        Err(err) => Err(eyre::eyre!("acquiring sync lock for search: {err}")),
    }
}
