//! Command dispatch for the Talon CLI scaffold.

use crate::cli::CliArgs;
use crate::config;
use crate::output::{OutputMode, emit_response};
use crate::spinner;
use eyre::{Result, WrapErr as _, bail};
use std::path::PathBuf;
use std::time::Instant;
use talon_core::{
    IndexerConfig, LintCheck, LintResponse, MetaInput, MetaResponse, ReadResponse, RelatedInput,
    RelatedResponse, SearchInput, SearchResponse, StatusResponse, SyncInput, SyncResponse,
    SyncStatus, TalonResponse, embed::EmbedPassOptions, inference::InferenceClient,
    open_database, run_sync, vec_ext::register_sqlite_vec,
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
        "search" => emit_search_stub(args, rest).await,
        "read" => emit_read_stub(args, rest).await,
        "sync" => emit_sync_stub(args, rest).await,
        "related" => emit_related_stub(args, rest).await,
        "status" => emit_status_stub(args),
        "meta" => emit_meta_stub(args, rest).await,
        "changes" => bail!("changes is scaffolded but not implemented yet"),
        "lint" => emit_lint_stub(args, rest).await,
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

async fn emit_search_stub(args: &CliArgs, rest: &[String]) -> Result<()> {
    if rest.is_empty() {
        bail!("search requires a query");
    }

    let input = SearchInput::from_cli_query(
        rest.join(" "),
        args.mode.unwrap_or_default(),
        args.fast.enabled(),
        args.limit,
    )?;
    let work = async move {
        Ok::<TalonResponse, eyre::Report>(TalonResponse::Search(SearchResponse::empty_scaffold(
            input,
        )))
    };
    let response = if should_spin(args) {
        spinner::with_spinner("Searching...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(args))
}

async fn emit_read_stub(args: &CliArgs, rest: &[String]) -> Result<()> {
    if rest.is_empty() {
        bail!("read requires a path");
    }

    let work =
        async move { Ok::<TalonResponse, eyre::Report>(TalonResponse::Read(ReadResponse::stub())) };
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
                embed_stats.map_or(
                    (0, 0, false, None, Vec::new()),
                    |s| {
                        (
                            s.succeeded,
                            s.failed,
                            s.dimension_mismatch,
                            s.remediation,
                            s.diagnostics,
                        )
                    },
                );

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
        let response = TalonResponse::Sync(result?);
        Ok::<TalonResponse, eyre::Report>(response)
    };
    let response = if should_spin(args) {
        spinner::with_spinner("Syncing...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(args))
}

async fn emit_related_stub(args: &CliArgs, rest: &[String]) -> Result<()> {
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
    let work = async move {
        Ok::<TalonResponse, eyre::Report>(TalonResponse::Related(RelatedResponse {
            path: talon_core::VaultPath::parse(&input.path)?,
            direction: input.direction,
            results: Vec::new(),
        }))
    };
    let response = if should_spin(args) {
        spinner::with_spinner("Finding related...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(args))
}

fn emit_status_stub(args: &CliArgs) -> Result<()> {
    let response = TalonResponse::Status(StatusResponse::scaffold()?);
    emit_response(&response, output_mode(args))
}

async fn emit_meta_stub(args: &CliArgs, _rest: &[String]) -> Result<()> {
    let _input = MetaInput {
        where_: Vec::new(),
        since: None,
        scope: vec![],
        scope_only: vec![],
        select: vec![],
        tag_counts: false,
        sources: None,
        limit: talon_core::PositiveCount::new(
            args.limit.unwrap_or(talon_core::constants::DEFAULT_LIMIT),
            "limit",
        )?,
    };
    let work = async move {
        Ok::<TalonResponse, eyre::Report>(TalonResponse::Meta(MetaResponse {
            entries: Vec::new(),
            tag_counts: None,
        }))
    };
    let response = if should_spin(args) {
        spinner::with_spinner("Querying frontmatter...".to_string(), work).await?
    } else {
        work.await?
    };
    emit_response(&response, output_mode(args))
}

async fn emit_lint_stub(args: &CliArgs, rest: &[String]) -> Result<()> {
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
        bail!(
            "lint requires --check <type>; try orphans, broken-links, dangling-refs, unreferenced"
        );
    };

    let work = async move {
        Ok::<TalonResponse, eyre::Report>(TalonResponse::Lint(LintResponse {
            check,
            findings: Vec::new(),
        }))
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
