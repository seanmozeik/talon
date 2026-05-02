//! Colored figlet banner for human-oriented TTY runs.

use crate::cli::{Cli, Commands};
use anstyle::{Color, Effects, RgbColor, Style};
use std::fmt::Write as _;
use std::io::{IsTerminal, Write};

const BANNER_TALON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/talon.txt"));

const LINE_INDENT: &str = "  ";
const GRADIENT_STOPS: [(u8, u8, u8); 4] = [
    (118, 221, 255),
    (93, 183, 255),
    (123, 151, 255),
    (180, 140, 255),
];

/// Prints the banner to stderr when stdout/stderr are TTYs and agent mode is off.
pub fn eprint_fancy_prelude_for_run(cli: &Cli) {
    if cli.agent
        || cli.json
        || !human_tty_for_cli_arts()
        || matches!(cli.command.as_ref(), Some(Commands::Mcp))
    {
        return;
    }
    eprint_figlet_indented(LINE_INDENT);
}

/// Clears the banner area if the human TTY prelude was shown.
pub fn clear_fancy_prelude() {
    if !human_tty_for_cli_arts() {
        return;
    }

    let mut stdout = std::io::stdout().lock();
    let _ = write!(stdout, "{}", clear_fancy_prelude_escape());
    let _ = stdout.flush();
}

/// Returns whether the banner should be cleared before emitting command output.
pub fn should_clear_fancy_prelude(cli: &Cli) -> bool {
    if cli.agent || cli.json || !human_tty_for_cli_arts() {
        return false;
    }

    matches!(
        cli.command.as_ref(),
        Some(
            Commands::Search(_)
                | Commands::Ask(_)
                | Commands::Read(_)
                | Commands::Sync(_)
                | Commands::Related(_)
                | Commands::Meta(_)
                | Commands::Changes(_)
                | Commands::Inspect(_)
                | Commands::Recall(_)
        )
    )
}

/// Returns the colored banner string for clap `before_help`.
/// Only emits colors when stdout is a TTY that accepts ANSI codes.
pub fn help_banner_colored() -> String {
    if !std::io::stdout().is_terminal() || !crate::platform::user_accepts_ansi_color() {
        return String::new();
    }

    let lines = banner_lines();
    let width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(1)
        .saturating_sub(1)
        .max(1);

    let mut out = String::new();
    for line in lines {
        let _ = write!(out, "{LINE_INDENT}");
        for (index, ch) in line.chars().enumerate() {
            let (r, g, b) = gradient_rgb(index, width);
            let style = Style::new()
                .fg_color(Some(Color::Rgb(RgbColor(r, g, b))))
                .effects(Effects::BOLD);
            let _ = write!(out, "{style}{ch}{style:#}");
        }
        let _ = writeln!(out);
    }
    out
}

fn human_tty_for_cli_arts() -> bool {
    crate::platform::stdout_is_tty()
        && crate::platform::stderr_is_tty()
        && crate::platform::user_accepts_ansi_color()
}

fn eprint_figlet_indented(indent: &str) {
    let lines = banner_lines();
    let width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(1)
        .saturating_sub(1)
        .max(1);

    for line in lines {
        eprint!("{indent}");
        for (index, ch) in line.chars().enumerate() {
            let (r, g, b) = gradient_rgb(index, width);
            let style = Style::new()
                .fg_color(Some(Color::Rgb(RgbColor(r, g, b))))
                .effects(Effects::BOLD);
            eprint!("{style}{ch}{style:#}");
        }
        eprintln!();
    }
    let _ = std::io::stderr().flush();
    eprintln!();
}

fn banner_lines() -> Vec<&'static str> {
    BANNER_TALON
        .lines()
        .filter(|line| !line.is_empty())
        .collect()
}

fn clear_fancy_prelude_escape() -> String {
    format!("\x1b[{}F\x1b[J", banner_lines().len().saturating_add(1))
}

fn gradient_rgb(index: usize, denominator: usize) -> (u8, u8, u8) {
    let last_stop = GRADIENT_STOPS.len() - 1;
    let scaled = index.saturating_mul(last_stop);
    let segment = (scaled / denominator).min(last_stop - 1);
    let segment_start = segment * denominator;
    let local_index = scaled.saturating_sub(segment_start);
    let local_denominator = denominator.max(1);

    let (left_r, left_g, left_b) = GRADIENT_STOPS[segment];
    let (right_r, right_g, right_b) = GRADIENT_STOPS[segment + 1];
    let blend = |left: u8, right: u8| -> u8 {
        let fallback = right;
        let left = usize::from(left);
        let right = usize::from(right);
        let value = ((left * (local_denominator - local_index)) + (right * local_index))
            / local_denominator;
        u8::try_from(value).unwrap_or(fallback)
    };

    (
        blend(left_r, right_r),
        blend(left_g, right_g),
        blend(left_b, right_b),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_line_count_includes_trailing_blank_line() {
        let figlet_line_count = BANNER_TALON.lines().filter(|line| !line.is_empty()).count();
        assert_eq!(banner_lines().len(), figlet_line_count);
        assert_eq!(
            clear_fancy_prelude_escape(),
            format!("\x1b[{}F\x1b[J", figlet_line_count + 1)
        );
    }
}
