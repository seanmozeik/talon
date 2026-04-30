//! Arguments for `talon sync`.

use clap::Args;

/// Arguments for the `sync` subcommand.
#[derive(Debug, Clone, Args)]
#[command(about = "Sync your vault with the search index.")]
pub struct SyncArgs {
    /// Paths to sync (defaults to entire vault).
    #[arg(value_hint = clap::ValueHint::FilePath)]
    pub paths: Vec<String>,

    /// Force vector rebuild during sync.
    #[arg(long)]
    pub force: bool,

    /// Delete and recreate the index before syncing.
    #[arg(long)]
    pub rebuild: bool,
}
