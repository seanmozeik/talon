//! Command dispatch for the Talon CLI scaffold.

use crate::cli::CliArgs;
use crate::output::{OutputMode, emit_response};
use crate::spinner;
use eyre::{Result, bail};
use talon_core::{SearchInput, SearchResponse, StatusResponse, TalonResponse};

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
        let _config = crate::config::load_config_file(path)?;
    }

    let Some((command, rest)) = args.positionals.split_first() else {
        bail!("missing command; try `talon --help`");
    };

    match command.as_str() {
        "init" => init_config(),
        "search" => emit_search_stub(args, rest).await,
        "read" => bail!("read is scaffolded but not implemented yet"),
        "sync" => bail!("sync is scaffolded but not implemented yet"),
        "related" => bail!("related is scaffolded but not implemented yet"),
        "status" => emit_status_stub(),
        "help" => bail!("use `talon --help` for command help"),
        other => bail!("unknown command `{other}`"),
    }
}

fn init_config() -> Result<()> {
    let result = crate::config::init_default_config()?;
    if result.created {
        eprintln!("Created {}", result.path.display());
    } else {
        eprintln!("Exists {}", result.path.display());
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

fn emit_status_stub() -> Result<()> {
    let response = TalonResponse::Status(StatusResponse::scaffold()?);
    emit_response(&response, output_mode_for_human_json())
}

const fn output_mode(args: &CliArgs) -> OutputMode {
    if args.agent.enabled() {
        OutputMode::Agent
    } else {
        output_mode_for_human_json()
    }
}

const fn output_mode_for_human_json() -> OutputMode {
    OutputMode::JsonPretty
}

fn should_spin(args: &CliArgs) -> bool {
    !args.agent.enabled() && crate::platform::stderr_is_tty()
}
