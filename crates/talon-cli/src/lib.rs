//! CLI process boundary for Talon.

mod banner;
pub mod cli;
pub mod command;
pub mod config;
pub mod exit_codes;
pub mod mcp;
pub mod output;
pub mod platform;
mod spinner;

/// Embedded skill contract printed by `talon --skill`.
pub const SKILL_MD: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../skill/SKILL.md"));

/// Runs the CLI and returns a process exit code.
#[must_use]
pub async fn run() -> u8 {
    let args = cli::parse_or_exit();
    platform::start();

    if args.skill.enabled() {
        return output::write_stdout_bytes(SKILL_MD.as_bytes());
    }

    banner::eprint_fancy_prelude_for_run(&args);

    match command::run(&args).await {
        Ok(()) => exit_codes::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:#}");
            exit_codes::GENERIC_ERROR
        }
    }
}
