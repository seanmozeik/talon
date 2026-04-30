//! Arguments for `talon search`.

use clap::{Args, ValueEnum};

use crate::cli::SharedScopeArgs;
use talon_core::SearchMode;

/// Search mode variant for clap derive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CliSearchMode {
    /// Hybrid lexical plus semantic search.
    Hybrid,
    /// Semantic-only search.
    Semantic,
    /// Full-text search.
    Fulltext,
    /// Title and alias search.
    Title,
}

impl From<CliSearchMode> for SearchMode {
    fn from(mode: CliSearchMode) -> Self {
        match mode {
            CliSearchMode::Hybrid => Self::Hybrid,
            CliSearchMode::Semantic => Self::Semantic,
            CliSearchMode::Fulltext => Self::Fulltext,
            CliSearchMode::Title => Self::Title,
        }
    }
}

/// Arguments for the `search` subcommand.
#[derive(Debug, Clone, Args)]
#[command(
    about = "Search your Obsidian vault using hybrid ranking.",
    long_about = r#"Search your Obsidian vault using hybrid ranking.

Combines BM25 fulltext scoring with semantic vector similarity.
The query is expanded using LLM-based context before ranking."#
)]
pub struct SearchArgs {
    /// Search query (space-separated words).
    pub query: Vec<String>,

    #[command(flatten)]
    pub shared: SharedSearchArgs,
}

/// Search-related flags shared by `search` and `ask`.
#[derive(Debug, Clone, Args)]
pub struct SharedSearchArgs {
    #[arg(long, value_enum, ignore_case = true)]
    pub mode: Option<CliSearchMode>,

    #[arg(short = 'n', long)]
    pub limit: Option<u16>,

    #[arg(long)]
    pub candidate_limit: Option<u16>,

    #[arg(long)]
    pub intent: Option<String>,

    /// Frontmatter filter: KEY OP VALUE (repeatable). Ops: =, !=, <, <=, >, >=, contains, exists.
    #[arg(long)]
    pub where_: Vec<String>,

    /// Filter results indexed since this timestamp (ISO 8601, epoch ms, or relative like 7d/3h).
    #[arg(long)]
    pub since: Option<String>,

    /// Include per-result match anchors (BM25 + semantic) in the response.
    #[arg(long)]
    pub anchors: bool,

    /// Show only title, path, and score — no snippets or extra metadata.
    #[arg(long)]
    pub compact: bool,

    #[command(flatten)]
    pub scope: SharedScopeArgs,
}
