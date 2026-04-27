//! Colored figlet banner for human-oriented TTY runs.

use crate::cli::CliArgs;
use anstyle::{Color, Effects, RgbColor, Style};
use std::io::Write;

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
pub fn eprint_fancy_prelude_for_run(args: &CliArgs) {
    if args.agent.enabled()
        || args.json.enabled()
        || args.mcp.enabled()
        || !human_tty_for_cli_arts()
    {
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
    let lines: Vec<&str> = BANNER_TALON
        .lines()
        .filter(|line| !line.is_empty())
        .collect();
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
