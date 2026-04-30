//! Arguments for `talon meta`.

use clap::Args;

use crate::cli::SharedScopeArgs;

/// Arguments for the `meta` subcommand.
#[derive(Debug, Clone, Args)]
#[command(about = "Query frontmatter metadata from your vault.")]
pub struct MetaArgs {
    /// Frontmatter field to project (repeatable).
    #[arg(long)]
    pub select: Vec<String>,

    /// Emit tag counts.
    #[arg(long)]
    pub tag_counts: bool,

    /// Resolve notes referencing this path via their sources: field.
    #[arg(long)]
    pub sources: Option<String>,

    /// Frontmatter filter: KEY OP VALUE (repeatable). Ops: =, !=, <, <=, >, >=, contains, exists.
    #[arg(long)]
    pub where_: Vec<String>,

    /// Filter results indexed since this timestamp.
    #[arg(long)]
    pub since: Option<String>,

    /// Search result limit.
    #[arg(short = 'n', long)]
    pub limit: Option<u16>,

    #[command(flatten)]
    pub scope: SharedScopeArgs,
}
