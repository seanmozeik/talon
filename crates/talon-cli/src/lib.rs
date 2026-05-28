//! CLI process boundary for Talon.

pub mod agent_contract;
mod banner;
pub mod cli;
pub mod command;
pub mod config;
pub mod exit_codes;
pub mod mcp;
pub mod output;
pub mod platform;
mod spinner;
mod telemetry;

/// Embedded skill contract printed by `talon --skill`.
pub const SKILL_MD: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/embedded/SKILL.md"));

/// Runs the CLI and returns a process exit code.
#[must_use]
pub async fn run() -> u8 {
    let cli = cli::parse_or_exit();
    platform::start();

    if cli.skill {
        return output::write_stdout_bytes(SKILL_MD.as_bytes());
    }

    banner::eprint_fancy_prelude_for_run(&cli);

    match command::run(&cli).await {
        Ok(()) => exit_codes::SUCCESS,
        Err(error) => {
            // When JSON output was requested, emit a structured error envelope so the caller
            // always receives machine-readable output — even on failure (Decision 8).
            if cli.json || cli.agent {
                let action = cli.command.as_ref().map_or("unknown", |cmd| match cmd {
                    cli::Commands::Mcp => "mcp",
                    cli::Commands::Init(_) => "init",
                    cli::Commands::Sync(_) => "sync",
                    cli::Commands::Status(_) => "status",
                    cli::Commands::Search(_) => "search",
                    cli::Commands::Ask(_) => "ask",
                    cli::Commands::Read(_) => "read",
                    cli::Commands::Related(_) => "related",
                    cli::Commands::Meta(_) => "meta",
                    cli::Commands::Changes(_) => "changes",
                    cli::Commands::Inspect(_) => "inspect",
                    cli::Commands::Recall(_) => "recall",
                    cli::Commands::Secrets(_) => "secrets",
                });
                let envelope = talon_core::TalonEnvelope::err(
                    action,
                    talon_core::ErrorEnvelope {
                        code: talon_core::ErrorCode::Internal,
                        message: format!("{error:#}"),
                        detail: None,
                    },
                );
                let mode = if cli.agent {
                    output::OutputMode::Agent
                } else {
                    output::OutputMode::JsonPretty
                };
                let _ = output::emit_response(&envelope, mode);
            } else {
                if banner::should_clear_fancy_prelude(&cli) {
                    banner::clear_fancy_prelude();
                }
                eprintln!("Error: {error:#}");
            }
            exit_codes::GENERIC_ERROR
        }
    }
}
