//! Arguments for `talon related`.

use clap::{Args, ValueEnum};

use crate::cli::SharedScopeArgs;

/// Direction variant for clap derive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CliDirection {
    /// Outgoing wikilinks.
    Outgoing,
    /// Backlinks.
    Backlinks,
    /// Outgoing wikilinks and backlinks.
    Both,
}

impl From<CliDirection> for talon_core::Direction {
    fn from(dir: CliDirection) -> Self {
        match dir {
            CliDirection::Outgoing => Self::Outgoing,
            CliDirection::Backlinks => Self::Backlinks,
            CliDirection::Both => Self::Both,
        }
    }
}

/// Arguments for the `related` subcommand.
#[derive(Debug, Clone, Args)]
#[command(about = "Find related notes via wikilink traversal.")]
pub struct RelatedArgs {
    /// Path to the note in the vault.
    pub path: String,

    /// Traversal depth (default 1).
    #[arg(long)]
    pub depth: Option<u8>,

    /// Traversal direction.
    #[arg(long, value_enum, ignore_case = true)]
    pub direction: Option<CliDirection>,

    #[command(flatten)]
    pub scope: SharedScopeArgs,
}
