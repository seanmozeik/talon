//! Command dispatch for the Talon CLI scaffold.

mod changes;
mod init;
mod lint;
mod meta;
mod read;
mod recall;
mod related;
mod search;
mod status;
mod sync;

use crate::cli::CliArgs;
use crate::config;
use crate::mcp::transport::{TransportOutcome, run_jsonrpc_loop};
use crate::output::OutputMode;
use eyre::{Result, bail};
use std::io::{self, BufReader};

/// Runs the selected command.
///
/// # Errors
///
/// Returns an error for invalid command input or not-yet-implemented behavior.
pub async fn run(args: &CliArgs) -> Result<()> {
    if args.version.enabled() {
        use std::io::Write as _;
        writeln!(io::stdout().lock(), "{}", env!("CARGO_PKG_VERSION"))?;
        return Ok(());
    }

    if args.mcp.enabled() {
        let stdin = io::stdin();
        let stdout = io::stdout();
        let outcome = run_jsonrpc_loop(BufReader::new(stdin.lock()), stdout.lock())?;
        if outcome == TransportOutcome::Shutdown {
            return Ok(());
        }
        return Ok(());
    }

    if let Some(path) = args.config_file.as_deref() {
        let _config = config::load_config_file(path)?;
    }

    let Some((command, rest)) = args.positionals.split_first() else {
        bail!("missing command; try `talon --help`");
    };

    match command.as_str() {
        "init" => init::emit(),
        "search" => search::emit(args, rest).await,
        "read" => read::emit(args, rest).await,
        "sync" => sync::emit(args, rest).await,
        "related" => related::emit(args, rest).await,
        "status" => status::emit(args),
        "meta" => meta::emit(args).await,
        "changes" => changes::emit(args).await,
        "lint" => lint::emit(args, rest).await,
        "recall" => recall::emit(args, rest).await,
        "help" => bail!("use `talon --help` for command help"),
        other => bail!("unknown command `{other}`"),
    }
}

pub(super) const fn output_mode(args: &CliArgs) -> OutputMode {
    if args.agent.enabled() {
        OutputMode::Agent
    } else if args.json.enabled() {
        OutputMode::JsonPretty
    } else {
        OutputMode::Human
    }
}

pub(super) fn should_spin(args: &CliArgs) -> bool {
    !args.agent.enabled() && !args.json.enabled() && crate::platform::stderr_is_tty()
}
