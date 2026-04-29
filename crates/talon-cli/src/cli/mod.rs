//! CLI argument parsing via `clap` derive.

mod changes_args;
mod init_args;
mod lint_args;
mod meta_args;
mod read_args;
mod recall_args;
mod related_args;
pub mod scope;
mod search_args;
mod status_args;
mod sync_args;
mod where_clause;

pub use where_clause::parse_where_clause;

pub use changes_args::ChangesArgs;
use clap::{Parser, Subcommand};
pub use init_args::InitArgs;
pub use lint_args::LintArgs;
pub use lint_args::LintCheck;
pub use meta_args::MetaArgs;
pub use read_args::ReadArgs;
pub use recall_args::RecallArgs;
pub use related_args::RelatedArgs;
pub use scope::SharedScopeArgs;
pub use search_args::{CliSearchMode, SearchArgs};
pub use status_args::StatusArgs;
pub use sync_args::SyncArgs;

/// Talon — Obsidian vault search, indexing, and MCP server.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Parser)]
#[command(
    name = "talon",
    version = env!("CARGO_PKG_VERSION"),
    subcommand_required = false,
    about = "Obsidian vault search, indexing, and MCP server.",
    before_help = banner::help_banner(),
    after_help = r#"Examples:
  talon search "project setup" --mode hybrid
  talon read src/main.rs --from-line 10 --max-lines 20
  talon related src/lib.rs --depth 2 --direction both
  talon sync --force

Use 'talon <command> --help' for per-command help."#
)]
pub struct Cli {
    /// Print embedded SKILL.md.
    #[arg(long, global = true)]
    pub skill: bool,

    /// Token-efficient JSON for agents. Disables human banner and spinner.
    #[arg(long, global = true, conflicts_with = "verbose")]
    pub agent: bool,

    /// Emit JSON output.
    #[arg(long, global = true)]
    pub json: bool,

    /// Use fast mode for search or sync.
    #[arg(long, global = true)]
    pub fast: bool,

    /// Include diagnostic details in output.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Read config from PATH.
    #[arg(short = 'c', long = "config", global = true, value_hint = clap::ValueHint::FilePath)]
    pub config_file: Option<std::path::PathBuf>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Commands {
    #[command(about = "Initialize a new talon configuration file.")]
    Init(InitArgs),

    #[command(about = "Sync your vault with the search index.")]
    Sync(SyncArgs),

    #[command(about = "Show vault index status.")]
    Status(StatusArgs),

    #[command(about = "Search your Obsidian vault using hybrid ranking.")]
    Search(SearchArgs),

    #[command(about = "Read a note from your vault.")]
    Read(ReadArgs),

    #[command(about = "Find related notes via wikilink traversal.")]
    Related(RelatedArgs),

    #[command(about = "Query frontmatter metadata from your vault.")]
    Meta(MetaArgs),

    #[command(about = "Show vault changes since a timestamp.")]
    Changes(ChangesArgs),

    #[command(about = "Lint your vault for common issues.")]
    Lint(LintArgs),

    #[command(about = "Recall relevant vault context for a message.")]
    Recall(RecallArgs),

    #[command(about = "Run MCP-over-stdio mode.")]
    Mcp,
}

mod banner {
    use std::fmt::Write as _;

    const BANNER: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/talon.txt"));

    pub fn help_banner() -> String {
        let mut out = String::new();
        for line in BANNER.lines().filter(|l| !l.is_empty()) {
            let _ = writeln!(out, "  {line}");
        }
        out
    }
}

/// Parses CLI args or exits on error.
#[must_use]
pub fn parse_or_exit() -> Cli {
    Cli::parse()
}
