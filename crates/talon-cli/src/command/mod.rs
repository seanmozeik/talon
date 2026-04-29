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

use crate::cli::{Cli, Commands};
use crate::mcp::transport::run_jsonrpc_loop;
use crate::output::OutputMode;
use eyre::{Result, bail};
use std::io::{self, BufReader};

/// Runs the selected command.
///
/// # Errors
///
/// Returns an error for invalid command input or not-yet-implemented behavior.
pub async fn run(cli: &Cli) -> Result<()> {
    if cli.skill {
        use std::io::Write as _;
        writeln!(io::stdout().lock(), "{}", crate::SKILL_MD)?;
        return Ok(());
    }

    let Some(cmd) = &cli.command else {
        bail!("missing command; try `talon --help`");
    };

    match cmd {
        Commands::Mcp => {
            let stdin = io::stdin();
            let stdout = io::stdout();
            let outcome = run_jsonrpc_loop(BufReader::new(stdin.lock()), stdout.lock())?;
            let _ = outcome;
            Ok(())
        }
        Commands::Init(args) => init::emit(args),
        Commands::Search(args) => search::emit(args, cli).await,
        Commands::Read(args) => read::emit(args, cli).await,
        Commands::Sync(args) => sync::emit(args, cli).await,
        Commands::Related(args) => related::emit(args, cli).await,
        Commands::Status(args) => status::emit(args, cli),
        Commands::Meta(args) => meta::emit(args, cli).await,
        Commands::Changes(args) => changes::emit(args, cli).await,
        Commands::Lint(args) => lint::emit(args, cli).await,
        Commands::Recall(args) => recall::emit(args, cli).await,
    }
}

pub(super) const fn output_mode(cli: &Cli) -> OutputMode {
    if cli.agent {
        OutputMode::Agent
    } else if cli.json {
        OutputMode::JsonPretty
    } else {
        OutputMode::Human
    }
}

pub(super) fn should_spin(cli: &Cli) -> bool {
    !cli.agent && !cli.json && crate::platform::stderr_is_tty()
}
