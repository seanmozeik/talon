//! Arguments for `talon read`.

use clap::Args;

/// Arguments for the `read` subcommand.
#[derive(Debug, Clone, Args)]
#[command(about = "Read a note from your vault.")]
pub struct ReadArgs {
    /// Path to the note in the vault.
    pub path: String,

    #[arg(long)]
    /// First line to read (1-indexed).
    pub from_line: Option<u16>,

    #[arg(long)]
    /// Maximum number of lines to read.
    pub max_lines: Option<u16>,

    /// Read raw note content without formatting.
    #[arg(long)]
    pub raw: bool,
}
