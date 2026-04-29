//! Arguments for `talon ask`.

use clap::Args;

use super::SharedSearchArgs;

/// Arguments for the `ask` subcommand.
#[derive(Debug, Clone, Args)]
#[command(
    about = "Ask a vault-grounded question.",
    long_about = r#"Ask a broad question and synthesize an answer from ranked vault snippets.

The question is planned into search queries with the configured ask model,
then Talon's existing search pipeline retrieves the supporting snippets."#
)]
pub struct AskArgs {
    /// Question to answer from vault knowledge.
    pub question: Vec<String>,

    #[command(flatten)]
    pub shared: SharedSearchArgs,
}
