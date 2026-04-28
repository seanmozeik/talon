use std::time::Instant;

use color_eyre::eyre::{Result, WrapErr as _};
use talon_core::{
    ChangesInput, ExpansionClient, LintInput, MetaInput, ReadInput, RecallInput, RelatedInput,
    ResponseMeta, SearchInput, SearchMode, TalonEnvelope, TalonInput, TalonResponseData,
    find_related, inference::InferenceClient, open_database, query_changes, query_lint, query_meta,
    run_read, run_recall, run_search, vec_ext::register_sqlite_vec,
};

use crate::config;
use crate::telemetry::{count_u32, elapsed_ms};

pub(super) fn dispatch_input(input: TalonInput) -> Result<TalonEnvelope> {
    match input {
        TalonInput::Search(input) => dispatch_search(&input),
        TalonInput::Read(input) => dispatch_read(&input),
        TalonInput::Sync(input) => super::sync::dispatch_sync(&input),
        TalonInput::Status(input) => Ok(super::status::dispatch_status(input)),
        TalonInput::Related(input) => dispatch_related(&input),
        TalonInput::Meta(input) => dispatch_meta(&input),
        TalonInput::Changes(input) => dispatch_changes(&input),
        TalonInput::Lint(input) => dispatch_lint(&input),
        TalonInput::Recall(input) => dispatch_recall(&input),
    }
}

fn dispatch_search(input: &SearchInput) -> Result<TalonEnvelope> {
    let started = Instant::now();
    let config = config::load_config(None)?;
    register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
    let mut conn = open_database(&config.db_path)
        .wrap_err_with(|| format!("opening index at {}", config.db_path.display()))?;
    let mode = input.mode;
    let fast = input.fast;
    crate::config::refresh_index_if_needed(&config, &mut conn, fast)?;
    let (inference, expansion) =
        if fast || mode == SearchMode::Fulltext || mode == SearchMode::Title {
            (None, None)
        } else {
            talon_core::cache::rerank::configure_capacity(config.search.rerank_cache_size);
            (
                InferenceClient::with_rerank_options(
                    &config.inference.base_url,
                    config.search.rerank_batch_size,
                    config.search.rerank_max_tokens,
                )
                .ok(),
                ExpansionClient::with_max_tokens(
                    config.expansion.base_url.clone(),
                    &config.expansion.model,
                    config.expansion.max_tokens,
                )
                .ok(),
            )
        };
    let response = run_search(
        &conn,
        input,
        inference.as_ref(),
        expansion.as_ref(),
        Some(&config),
    );
    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: Some(response.total),
        warnings: Vec::new(),
        scope_set: Some(config.default_scope_names().into_iter().cloned().collect()),
        since: input.since.clone(),
    };
    Ok(TalonEnvelope::ok(
        "search",
        TalonResponseData::Search(response),
        meta,
    ))
}

fn dispatch_read(input: &ReadInput) -> Result<TalonEnvelope> {
    let started = Instant::now();
    let config = config::load_config(None)?;
    let conn = open_database(&config.db_path)
        .wrap_err_with(|| format!("opening index at {}", config.db_path.display()))?;
    let response = run_read(&conn, &config.vault_path, input);
    let result_count = response
        .results
        .iter()
        .filter(|result| result.found)
        .count();
    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: Some(count_u32(result_count)),
        warnings: Vec::new(),
        scope_set: None,
        since: None,
    };
    Ok(TalonEnvelope::ok(
        "read",
        TalonResponseData::Read(response),
        meta,
    ))
}

fn dispatch_related(input: &RelatedInput) -> Result<TalonEnvelope> {
    let started = Instant::now();
    let config = config::load_config(None)?;
    let mut conn = open_database(&config.db_path)
        .wrap_err_with(|| format!("opening index at {}", config.db_path.display()))?;
    crate::config::refresh_index_if_needed(&config, &mut conn, false)?;
    let response = find_related(&conn, input, Some(&config));
    let result_count = count_u32(response.results.len());
    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: Some(result_count),
        warnings: Vec::new(),
        scope_set: None,
        since: None,
    };
    Ok(TalonEnvelope::ok(
        "related",
        TalonResponseData::Related(response),
        meta,
    ))
}

fn dispatch_meta(input: &MetaInput) -> Result<TalonEnvelope> {
    let started = Instant::now();
    let config = config::load_config(None)?;
    let mut conn = open_database(&config.db_path)
        .wrap_err_with(|| format!("opening index at {}", config.db_path.display()))?;
    crate::config::refresh_index_if_needed(&config, &mut conn, false)?;
    let since = input.since.clone();
    let response = query_meta(&conn, input, Some(&config));
    let result_count = count_u32(response.entries.len());
    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: Some(result_count),
        warnings: Vec::new(),
        scope_set: None,
        since,
    };
    Ok(TalonEnvelope::ok(
        "meta",
        TalonResponseData::Meta(response),
        meta,
    ))
}

fn dispatch_changes(input: &ChangesInput) -> Result<TalonEnvelope> {
    let started = Instant::now();
    let config = config::load_config(None)?;
    let mut conn = open_database(&config.db_path)
        .wrap_err_with(|| format!("opening index at {}", config.db_path.display()))?;
    crate::config::refresh_index_if_needed(&config, &mut conn, false)?;
    let since = input.since.clone();
    let response = query_changes(&conn, input, Some(&config));
    let result_count =
        count_u32(response.added.len() + response.modified.len() + response.deleted.len());
    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: Some(result_count),
        warnings: Vec::new(),
        scope_set: None,
        since: Some(since),
    };
    Ok(TalonEnvelope::ok(
        "changes",
        TalonResponseData::Changes(response),
        meta,
    ))
}

fn dispatch_lint(input: &LintInput) -> Result<TalonEnvelope> {
    let started = Instant::now();
    let config = config::load_config(None)?;
    register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
    let mut conn = open_database(&config.db_path)
        .wrap_err_with(|| format!("opening index at {}", config.db_path.display()))?;
    // Lint always refreshes — findings must reflect current vault state.
    crate::config::refresh_index_if_needed(&config, &mut conn, false)?;

    let response = query_lint(&conn, input, Some(&config));
    let result_count = count_u32(response.findings.len());
    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: Some(result_count),
        warnings: Vec::new(),
        scope_set: None,
        since: None,
    };
    Ok(TalonEnvelope::ok(
        "lint",
        TalonResponseData::Lint(response),
        meta,
    ))
}

fn dispatch_recall(input: &RecallInput) -> Result<TalonEnvelope> {
    let started = Instant::now();
    let config = config::load_config(None)?;
    register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
    let conn = open_database(&config.db_path)
        .wrap_err_with(|| format!("opening index at {}", config.db_path.display()))?;
    let fast = input.fast;
    let (inference, expansion) = if fast {
        (None, None)
    } else {
        talon_core::cache::rerank::configure_capacity(config.search.rerank_cache_size);
        (
            InferenceClient::with_rerank_options(
                &config.inference.base_url,
                config.search.rerank_batch_size,
                config.search.rerank_max_tokens,
            )
            .ok(),
            ExpansionClient::with_max_tokens(
                config.expansion.base_url.clone(),
                &config.expansion.model,
                config.expansion.max_tokens,
            )
            .ok(),
        )
    };
    let response = run_recall(
        &conn,
        inference.as_ref(),
        expansion.as_ref(),
        input,
        Some(&config),
    );
    let result_count = response
        .vault_recall
        .as_ref()
        .map(|r| count_u32(r.active_notes.len()));
    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count,
        warnings: Vec::new(),
        scope_set: Some(config.default_scope_names().into_iter().cloned().collect()),
        since: None,
    };
    Ok(TalonEnvelope::ok(
        "recall",
        TalonResponseData::Recall(response),
        meta,
    ))
}
