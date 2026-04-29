//! Arguments for `talon recall`.

use clap::Args;

use crate::cli::SharedScopeArgs;

/// Arguments for the `recall` subcommand.
#[derive(Debug, Clone, Args)]
#[command(
    about = "Recall relevant vault context for a message.",
    long_about = r#"Recall relevant vault context for a message.

Uses semantic search to find notes relevant to the query message,
then assembles them into a compact context block suitable for
agent tool calls."#
)]
pub struct RecallArgs {
    /// Message to recall context for.
    pub message: Vec<String>,

    /// Output format: json (default) or prompt-xml.
    #[arg(long)]
    pub format: Option<String>,

    /// Token budget for the recall context block (default 500).
    #[arg(long)]
    pub budget_tokens: Option<u32>,

    /// Minimum evidence score threshold 0.0-1.0 (default 0.4).
    #[arg(long)]
    pub min_confidence: Option<f64>,

    /// Prior turn message to widen the query (repeatable).
    #[arg(long)]
    pub prior_messages: Vec<String>,

    /// Vault path to exclude from recall candidates (repeatable).
    #[arg(long)]
    pub exclude: Vec<String>,

    /// Traversal depth for context expansion (default 1).
    #[arg(long)]
    pub depth: Option<u8>,

    #[command(flatten)]
    pub scope: SharedScopeArgs,
}
