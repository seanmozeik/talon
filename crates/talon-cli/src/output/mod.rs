//! Stdout emission for CLI responses.

mod ask;
mod human;
pub(crate) mod json;
mod obsidian;
mod recall;
mod search;
mod style;

use crate::exit_codes;
use eyre::Result;
use std::io::{self, Write};
use talon_core::TalonEnvelope;

pub use ask::format_ask_human;
pub use human::{format_lint_human, format_status_human, format_sync_human};
pub use recall::{format_recall_human, format_recall_prompt_xml};
pub use search::format_search_human;

/// CLI output mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Human-readable formatted output (colored headings, result cards).
    Human,
    /// Full pretty JSON for debugging.
    JsonPretty,
    /// Compact token-efficient JSON for agents.
    Agent,
}

/// Options controlling human-readable rendering.
#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    /// Terminal column width used for wrapping.
    pub width: u16,
    /// Whether ANSI color codes should be emitted.
    pub colors: bool,
    /// Show compact one-liner cards (title + path + score only, no snippet).
    pub compact: bool,
}

impl RenderOptions {
    /// Detects the current terminal width and color support.
    #[must_use]
    pub fn for_terminal() -> Self {
        use terminal_size::{Width, terminal_size};
        let width = terminal_size().map_or(80, |(Width(w), _)| w);
        Self {
            width,
            colors: crate::platform::stdout_is_tty() && crate::platform::user_accepts_ansi_color(),
            compact: false,
        }
    }
}

/// Writes bytes to stdout.
#[must_use]
pub fn write_stdout_bytes(bytes: &[u8]) -> u8 {
    match io::stdout().lock().write_all(bytes) {
        Ok(()) => exit_codes::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            exit_codes::GENERIC_ERROR
        }
    }
}

/// Emits a Talon envelope.
///
/// # Errors
///
/// Returns an error if serialization or stdout writes fail.
pub fn emit_response(envelope: &TalonEnvelope, mode: OutputMode) -> Result<()> {
    match mode {
        OutputMode::Human => human::emit(envelope),
        OutputMode::JsonPretty => json::emit_pretty(envelope),
        OutputMode::Agent => json::emit_agent(envelope),
    }
}
