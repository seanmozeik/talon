//! Arguments for `talon changes`.

use clap::Args;

use crate::cli::SharedScopeArgs;

/// Arguments for the `changes` subcommand.
#[derive(Debug, Clone, Args)]
#[command(about = "Show vault changes since a timestamp.")]
pub struct ChangesArgs {
    /// Filter results indexed since this timestamp (ISO 8601, epoch ms, or relative like 7d/3h).
    #[arg(long)]
    pub since: String,

    /// Search result limit.
    #[arg(short = 'n', long)]
    pub limit: Option<u16>,

    #[command(flatten)]
    pub scope: SharedScopeArgs,
}
