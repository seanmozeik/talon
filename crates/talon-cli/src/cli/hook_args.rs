use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct HookArgs {
    #[command(subcommand)]
    pub subcommand: HookSubcommand,
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum HookSubcommand {
    #[command(about = "Run vault recall as an agent-host command hook.")]
    Recall(HookRecallArgs),
}

#[derive(Debug, Clone, Args)]
pub struct HookRecallArgs {
    /// Agent host contract to emit, for example claude-code or codex.
    #[arg(long, default_value = "codex")]
    pub host: String,

    /// Token budget for the recall context block.
    #[arg(long, default_value_t = 500)]
    pub budget_tokens: u32,

    /// Add configured scopes to the default search pool.
    #[arg(long = "scope")]
    pub scope: Vec<String>,

    /// Run recall in fast mode for hook latency.
    #[arg(long)]
    pub fast: bool,
}
