//! Command dispatch for the Talon CLI scaffold.

use crate::cli::{CliArgs, parse_where_clause};
use crate::config;
use crate::output::{OutputMode, emit_response};
use crate::spinner;
use eyre::{Result, WrapErr as _, bail};
use std::path::PathBuf;
use std::time::Instant;
use talon_core::{
    ExpansionClient, IndexerConfig, LintCheck, LintInput, MetaInput, PositiveCount, ReadInput,
    RelatedInput, ResponseMeta, SearchInput, SearchMode, StatusResponse, SyncInput, SyncResponse,
    SyncStatus, TalonEnvelope, TalonResponseData, embed::EmbedPassOptions, find_related,
    inference::InferenceClient, open_database, query_changes, query_lint, query_meta, run_read,
    run_search, run_sync, vec_ext::register_sqlite_vec,
};

/// Runs the selected command.
///
/// # Errors
///
/// Returns an error for invalid command input or not-yet-implemented behavior.
pub async fn run(args: &CliArgs) -> Result<()> {
    if args.mcp.enabled() {
        bail!("mcp mode is scaffolded but not implemented yet");
    }

    if let Some(path) = args.config_file.as_deref() {
        let _config = config::load_config_file(path)?;
    }

    let Some((command, rest)) = args.positionals.split_first() else {
        bail!("missing command; try `talon --help`");
    };

    match command.as_str() {
        "init" => init_config(),
        "search" => emit_search(args, rest).await,
        "read" => emit_read(args, rest).await,
        "sync" => emit_sync_stub(args, rest).await,
        "related" => emit_related(args, rest).await,
        "status" => emit_status_stub(args),
        "meta" => emit_meta(args, rest).await,
        "changes" => emit_changes(args, rest).await,
        "lint" => emit_lint(args, rest).await,
        "help" => bail!("use `talon --help` for command help"),
        other => bail!("unknown command `{other}`"),
    }
}

fn init_config() -> Result<()> {
    let result = config::init_config()?;
    if result {
        eprintln!("Created {}", config::default_config_path().display());
    } else {
        eprintln!("Exists {}", config::default_config_path().display());
    }
    Ok(())
}

async fn emit_search(args: &CliArgs, rest: &[String]) -> Result<()> {
    if rest.is_empty() {
        bail!("search requires a query");
    }

    let query = rest.join(" ");
    let mode = args.mode.unwrap_or_default();
    let fast = args.fast.enabled();
    let limit = args.limit;

    // Parse --where clauses.
    let where_clauses: Vec<talon_core::WhereClause> = args
        .where_clauses
        .iter()
        .map(|s| parse_where_clause(s).map_err(|e| eyre::eyre!("invalid --where: {s}: {e}")))
        .collect::<Result<Vec<_>>>()?;

    let input = SearchInput {
        query: Some(query),
        queries: Vec::new(),
        mode,
        fast,
        limit: PositiveCount::new(
            limit.unwrap_or(talon_core::constants::DEFAULT_LIMIT),
            "limit",
        )?,
        path: None,
        tag: Vec::new(),
        frontmatter: None,
        related: false,
        depth: talon_core::constants::RELATED_DEFAULT_DEPTH,
        direction: talon_core::Direction::Both,
        scope: Vec::new(),
        scope_only: Vec::new(),
        where_: where_clauses,
        since: args.since.clone(),
    };

    let started = Instant::now();

    // Load config for scope priority resolution.
    let config = config::load_config(args.config_file.as_deref()).ok();

    let work = async move {
        let db_path: PathBuf = config.as_ref().map_or_else(
            || PathBuf::from("~/.local/share/talon/index.sqlite"),
            |c| c.db_path.clone(),
        );

        // Open DB and register sqlite-vec.
        let conn = open_database(&db_path)
            .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;
        register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;

        // Build inference client (needed for hybrid/semantic modes).
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
                    .map(|c| ExpansionClient::new(c.expansion.base_url.clone(), &c.expansion.model))
                    .transpose()
                    .ok()
                    .flatten();
                (inference, expansion)
            };

        let response = run_search(
            &conn,
            &input,
            inference.as_ref(),
            expansion.as_ref(),
            config.as_ref().map(|c| c as &talon_core::TalonConfig),
        );

        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let meta = ResponseMeta {
            duration_ms,
            result_count: Some(response.total),
            warnings: Vec::new(),
            scope_set: config
                .as_ref()
                .map(|c| c.default_scope_names().into_iter().cloned().collect()),
            since: input.since.clone(),
        };
        let data = TalonResponseData::Search(response);
        Ok::<TalonEnvelope, eyre::Report>(TalonEnvelope::ok("search", data, meta))
    };

    let response = if should_spin(args) {
        spinner::with_spinner("Searching...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(args))
}

async fn emit_read(args: &CliArgs, rest: &[String]) -> Result<()> {
    if rest.is_empty() {
        bail!("read requires a path");
    }

    let path = rest[0].clone();
    let from_line = args
        .from_line
        .map(|n| PositiveCount::new(n, "from-line"))
        .transpose()?;
    let max_lines = args
        .max_lines
        .map(|n| PositiveCount::new(n, "max-lines"))
        .transpose()?;
    let raw = args.raw.enabled();

    let input = ReadInput {
        path: Some(path),
        raw,
        from_line,
        max_lines,
    };

    let config = config::load_config(args.config_file.as_deref())?;
    let db_path: PathBuf = config.db_path.clone();
    let vault_root: PathBuf = config.vault_path.clone();

    let started = Instant::now();
    let work = async move {
        let conn = open_database(&db_path)
            .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;

        let response = run_read(&conn, &vault_root, &input);
        let result_count = response.results.iter().filter(|r| r.found).count();

        let meta = ResponseMeta {
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            result_count: Some(u32::try_from(result_count).unwrap_or(u32::MAX)),
            warnings: Vec::new(),
            scope_set: None,
            since: None,
        };
        let data = TalonResponseData::Read(response);
        Ok::<TalonEnvelope, eyre::Report>(TalonEnvelope::ok("read", data, meta))
    };
    let response = if should_spin(args) {
        spinner::with_spinner("Reading...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(args))
}

async fn emit_sync_stub(args: &CliArgs, rest: &[String]) -> Result<()> {
    let input = SyncInput {
        paths: rest.to_vec(),
        fast: args.fast.enabled(),
        force: args.force.enabled(),
        no_wait: false,
    };
    let config = config::load_config(args.config_file.as_deref())?;
    let vault_path: PathBuf = config.vault_path.clone();
    let db_path: PathBuf = config.db_path.clone();
    let lock_path: PathBuf = db_path
        .parent()
        .map_or_else(|| PathBuf::from("sync.lock"), |p| p.join("sync.lock"));
    let indexer_config = IndexerConfig {
        include_patterns: config.include_patterns.clone(),
        ignore_patterns: config.ignore_patterns.clone(),
    };

    let work = async move {
        // Sync is CPU-and-disk-bound and uses sync rusqlite — push it to a
        // blocking thread so the tokio runtime stays responsive.
        let started = Instant::now();
        let path_count = u32::try_from(input.paths.len()).unwrap_or(u32::MAX);
        let result = tokio::task::spawn_blocking(move || -> Result<SyncResponse> {
            register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
            let mut conn = open_database(&db_path)
                .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;

            // Build embed config and inference client when not in fast mode.
            let (embed_opts, inference) = if input.fast {
                (None, None::<InferenceClient>)
            } else {
                let opts = EmbedPassOptions {
                    force: input.force,
                    restrict_paths: input.paths.clone(),
                    chunk_embedding_model: config.inference.models.chunk_embedding.clone(),
                    document_embedding_model: config.inference.models.document_embedding.clone(),
                };
                let client = InferenceClient::new(&config.inference.base_url)
                    .wrap_err("building inference client")?;
                (Some(opts), Some(client))
            };

            let (stats, embed_stats) = run_sync(
                &mut conn,
                &vault_path,
                &lock_path,
                &indexer_config,
                embed_opts,
                inference.as_ref(),
            )
            .wrap_err("sync failed")?;

            let (embedded, embed_failed, dimension_mismatch, embed_remediation, embed_diagnostics) =
                embed_stats.map_or((0, 0, false, None, Vec::new()), |s| {
                    (
                        s.succeeded,
                        s.failed,
                        s.dimension_mismatch,
                        s.remediation,
                        s.diagnostics,
                    )
                });

            Ok(SyncResponse {
                completed: true,
                status: SyncStatus::Ok,
                fast: input.fast,
                force: input.force,
                path_count,
                indexed: stats.indexed,
                skipped: stats.skipped,
                deleted: stats.deleted,
                embedded,
                embed_failed,
                dimension_mismatch,
                embed_remediation,
                embed_diagnostics,
                duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            })
        })
        .await
        .wrap_err("sync task join failed")?;
        let sync_resp = result?;
        let meta = ResponseMeta {
            duration_ms: sync_resp.duration_ms,
            result_count: Some(sync_resp.indexed),
            warnings: Vec::new(),
            scope_set: None,
            since: None,
        };
        let data = TalonResponseData::Sync(sync_resp);
        Ok::<TalonEnvelope, eyre::Report>(TalonEnvelope::ok("sync", data, meta))
    };
    let response = if should_spin(args) {
        spinner::with_spinner("Syncing...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(args))
}

async fn emit_related(args: &CliArgs, rest: &[String]) -> Result<()> {
    if rest.is_empty() {
        bail!("related requires a path");
    }

    let input = RelatedInput {
        path: rest[0].clone(),
        depth: args
            .depth
            .unwrap_or(talon_core::constants::RELATED_DEFAULT_DEPTH),
        direction: args.direction.unwrap_or_default(),
        scope: vec![],
        scope_only: vec![],
    };

    let config = config::load_config(args.config_file.as_deref())?;
    let db_path: PathBuf = config.db_path.clone();

    let started = Instant::now();
    let work = async move {
        let conn = open_database(&db_path)
            .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;

        let response = find_related(&conn, &input);
        let result_count = response.results.len();

        let meta = ResponseMeta {
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            result_count: Some(u32::try_from(result_count).unwrap_or(u32::MAX)),
            warnings: Vec::new(),
            scope_set: None,
            since: None,
        };
        let data = TalonResponseData::Related(response);
        Ok::<TalonEnvelope, eyre::Report>(TalonEnvelope::ok("related", data, meta))
    };
    let response = if should_spin(args) {
        spinner::with_spinner("Finding related...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(args))
}

fn emit_status_stub(args: &CliArgs) -> Result<()> {
    let started = Instant::now();
    let meta = ResponseMeta {
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        result_count: None,
        warnings: Vec::new(),
        scope_set: None,
        since: None,
    };
    let data = TalonResponseData::Status(StatusResponse::scaffold()?);
    let response = TalonEnvelope::ok("status", data, meta);
    emit_response(&response, output_mode(args))
}

async fn emit_meta(args: &CliArgs, _rest: &[String]) -> Result<()> {
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
        let result_count = u32::try_from(response.entries.len()).unwrap_or(u32::MAX);

        let meta = ResponseMeta {
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
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

async fn emit_changes(args: &CliArgs, _rest: &[String]) -> Result<()> {
    let since = args
        .since
        .clone()
        .ok_or_else(|| eyre::eyre!("changes requires --since <timestamp>"))?;
    let since_str = since.clone();

    let input = talon_core::ChangesInput {
        since,
        scope: Vec::new(),
        scope_only: Vec::new(),
        limit: PositiveCount::new(
            args.limit.unwrap_or(talon_core::constants::DEFAULT_LIMIT),
            "limit",
        )?,
    };

    let config = config::load_config(args.config_file.as_deref())?;
    let db_path: PathBuf = config.db_path.clone();

    let started = Instant::now();
    let work = async move {
        let conn = open_database(&db_path)
            .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;
        let response = query_changes(&conn, &input);
        let result_count =
            u32::try_from(response.added.len() + response.modified.len() + response.deleted.len())
                .unwrap_or(u32::MAX);
        let meta = ResponseMeta {
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            result_count: Some(result_count),
            warnings: Vec::new(),
            scope_set: None,
            since: Some(since_str),
        };
        let data = TalonResponseData::Changes(response);
        Ok::<TalonEnvelope, eyre::Report>(TalonEnvelope::ok("changes", data, meta))
    };
    let response = if should_spin(args) {
        spinner::with_spinner("Fetching changes...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(args))
}

async fn emit_lint(args: &CliArgs, rest: &[String]) -> Result<()> {
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

const fn output_mode(args: &CliArgs) -> OutputMode {
    if args.agent.enabled() {
        OutputMode::Agent
    } else if args.json.enabled() {
        OutputMode::JsonPretty
    } else {
        OutputMode::Human
    }
}

fn should_spin(args: &CliArgs) -> bool {
    !args.agent.enabled() && !args.json.enabled() && crate::platform::stderr_is_tty()
}
