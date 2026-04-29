//! Arguments for `talon init`.

use clap::Args;

/// Arguments for the `init` subcommand.
#[derive(Debug, Clone, Args)]
#[command(about = "Initialize a new talon configuration file.")]
pub struct InitArgs {}
