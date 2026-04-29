use super::{output_mode, should_spin};
use crate::cli::{Cli, SearchArgs, parse_where_clause};
use crate::config::{self, RefreshLockPolicy, vault_container_path};
use crate::output::emit_response;
use crate::spinner;
use crate::telemetry::elapsed_ms;
use eyre::{Result, WrapErr as _, bail};
use std::path::PathBuf;
use std::time::Instant;
use talon_core::{
    ExpansionClient, ResponseMeta, ScopeFilter, SearchInput, SearchMode, TalonEnvelope,
    TalonResponseData, inference::InferenceClient, is_sync_lock_held_by_live_process,
    open_database, open_database_read_only, run_search, run_search_with_expanded_queries,
    vec_ext::register_sqlite_vec,
};

pub(super) async fn emit(args: &SearchArgs, cli: &Cli) -> Result<()> {
    if args.query.is_empty() {
        bail!("search requires a query");
    }

    let query = args.query.join(" ");
    let mode = args
        .mode
        .map_or_else(SearchMode::default, std::convert::Into::into);
    let fast = cli.fast;
    let include_expanded_queries = cli.verbose;
    let config = config::load_config(cli.config_file.as_deref()).ok();

    let where_clauses: Vec<talon_core::WhereClause> = args
        .where_
        .iter()
        .map(|s| parse_where_clause(s).map_err(|e| eyre::eyre!("invalid --where: {s}: {e}")))
        .collect::<Result<Vec<_>>>()?;

    let mut input = SearchInput::from_cli_query(
        query,
        args.intent.clone(),
        mode,
        fast,
        args.limit,
        args.candidate_limit,
        config.as_ref().map(|config| &config.search),
    )?;
    input.where_ = where_clauses;
    input.since = args.since.clone();
    input.anchors = args.anchors.then_some(true);
    input.scope.clone_from(&args.scope.scope);
    input.scope_only.clone_from(&args.scope.scope_only);
    input.scope_all = args.scope.scope_all;

    if let Some(cfg) = config.as_ref() {
        ScopeFilter::from_args(cfg, &input.scope, &input.scope_only, input.scope_all)
            .map_err(|e| eyre::eyre!("{e}"))?;
    }

    let started = Instant::now();

    let work = async move {
        tokio::task::spawn_blocking(move || {
            execute_search(
                input,
                config.as_ref(),
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
    emit_response(&response, output_mode(cli))
}

fn execute_search(
    input: SearchInput,
    config: Option<&talon_core::TalonConfig>,
    started: Instant,
    fast: bool,
    mode: SearchMode,
    include_expanded_queries: bool,
) -> Result<TalonEnvelope> {
    let db_path: PathBuf = config
        .as_ref()
        .map_or_else(crate::config::default_db_path, |c| c.db_path.clone());

    register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
    let skip_refresh_for_live_sync = config
        .is_some_and(|cfg| is_sync_lock_held_by_live_process(&crate::config::sync_lock_path(cfg)));
    let mut conn = if fast || skip_refresh_for_live_sync {
        open_database_read_only(&db_path)
    } else {
        open_database(&db_path)
    }
    .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;
    if let Some(cfg) = config
        && !skip_refresh_for_live_sync
    {
        crate::config::refresh_index_if_needed(
            cfg,
            &mut conn,
            fast,
            RefreshLockPolicy::SkipIfBusy,
        )?;
    }

    let (inference, expansion) =
        if fast || mode == SearchMode::Fulltext || mode == SearchMode::Title {
            (None, None)
        } else {
            let inference_url = config.as_ref().map_or_else(
                || "http://localhost:8080".to_string(),
                |c| c.inference.base_url.clone(),
            );
            if let Some(config) = config {
                talon_core::cache::rerank::configure_capacity(config.search.rerank_cache_size);
            }
            let inference = match config {
                Some(config) => InferenceClient::with_rerank_options(
                    inference_url,
                    config.search.rerank_batch_size,
                    config.search.rerank_max_tokens,
                ),
                None => InferenceClient::new(inference_url),
            }
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

    let mut response = if include_expanded_queries {
        run_search_with_expanded_queries(
            &conn,
            &input,
            inference.as_ref(),
            expansion.as_ref(),
            config,
        )
    } else {
        run_search(
            &conn,
            &input,
            inference.as_ref(),
            expansion.as_ref(),
            config,
        )
    };
    response.vault = vault_container_path(config);

    let scope_set = config.as_ref().map(|cfg| {
        ScopeFilter::from_args(cfg, &input.scope, &input.scope_only, input.scope_all).map_or_else(
            |_| ScopeFilter::default_for(cfg).resolved_set(),
            |f| f.resolved_set(),
        )
    });
    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: Some(response.total),
        warnings: Vec::new(),
        scope_set,
        since: input.since,
    };
    Ok(TalonEnvelope::ok(
        "search",
        TalonResponseData::Search(response),
        meta,
    ))
}
