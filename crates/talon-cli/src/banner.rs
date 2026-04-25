//! Colored figlet banner for human-oriented TTY runs.

use crate::cli::CliArgs;
use anstyle::{Color, Effects, RgbColor, Style};
use std::io::Write;

const FIGLET_TALON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/talon-figlet-speed.txt"
));

const LINE_INDENT: &str = "  ";
const TOP_RGB: (u8, u8, u8) = (0, 240, 255);
const BOTTOM_RGB: (u8, u8, u8) = (88, 62, 210);

/// Prints the banner to stderr when stdout/stderr are TTYs and agent mode is off.
pub fn eprint_fancy_prelude_for_run(args: &CliArgs) {
    if args.agent.enabled() || !human_tty_for_cli_arts() {
        return;
    }
    eprint_figlet_indented(LINE_INDENT);
}

fn human_tty_for_cli_arts() -> bool {
    crate::platform::stdout_is_tty()
        && crate::platform::stderr_is_tty()
        && crate::platform::user_accepts_ansi_color()
}

fn eprint_figlet_indented(indent: &str) {
    let lines: Vec<&str> = FIGLET_TALON
        .lines()
        .filter(|line| !line.is_empty())
        .collect();
    let denominator = lines.len().saturating_sub(1).max(1);

    for (index, line) in lines.iter().enumerate() {
        let (r, g, b) = gradient_rgb(index, denominator);
        let style = Style::new()
            .fg_color(Some(Color::Rgb(RgbColor(r, g, b))))
            .effects(Effects::BOLD);
        eprintln!("{indent}{style}{line}{style:#}");
    }
    let _ = std::io::stderr().flush();
    eprintln!();
}

fn gradient_rgb(index: usize, denominator: usize) -> (u8, u8, u8) {
    let blend = |top: u8, bottom: u8| -> u8 {
        let fallback = bottom;
        let top = usize::from(top);
        let bottom = usize::from(bottom);
        let value = ((top * (denominator - index)) + (bottom * index)) / denominator;
        u8::try_from(value).unwrap_or(fallback)
    };

    (
        blend(TOP_RGB.0, BOTTOM_RGB.0),
        blend(TOP_RGB.1, BOTTOM_RGB.1),
        blend(TOP_RGB.2, BOTTOM_RGB.2),
    )
}
