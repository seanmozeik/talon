//! Arguments for `talon status`.

use clap::Args;

/// Arguments for the `status` subcommand.
#[derive(Debug, Clone, Args)]
#[command(about = "Show vault index status.")]
pub struct StatusArgs {}
