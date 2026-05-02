//! Arguments for `talon inspect`.

use clap::{Args, ValueEnum};

use crate::cli::SharedScopeArgs;

/// Inspect check type (clap derive wrapper).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LintCheck {
    /// Run all checks.
    All,
    /// Find orphan notes with no links to them.
    Orphans,
    /// Find broken wikilinks.
    BrokenLinks,
    /// Find dangling reference markers.
    DanglingRefs,
    /// Find unreferenced notes.
    Unreferenced,
    /// Find graph health signals.
    Graph,
}

impl From<LintCheck> for talon_core::LintCheck {
    fn from(check: LintCheck) -> Self {
        match check {
            LintCheck::All => Self::All,
            LintCheck::Orphans => Self::Orphans,
            LintCheck::BrokenLinks => Self::BrokenLinks,
            LintCheck::DanglingRefs => Self::DanglingRefs,
            LintCheck::Unreferenced => Self::Unreferenced,
            LintCheck::Graph => Self::Graph,
        }
    }
}

/// Arguments for the `inspect` subcommand.
#[derive(Debug, Clone, Args)]
#[command(about = "Inspect your vault for structural signals and patterns.")]
pub struct InspectArgs {
    /// Which inspect check to run (default: all).
    #[arg(value_enum, ignore_case = true)]
    pub check: Option<LintCheck>,

    #[command(flatten)]
    pub scope: SharedScopeArgs,
}
