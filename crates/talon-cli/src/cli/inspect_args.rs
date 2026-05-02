//! Arguments for `talon inspect`.

use clap::{Args, ValueEnum};

use crate::cli::SharedScopeArgs;

/// Inspect check type (clap derive wrapper).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InspectCheck {
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
    /// Find graph health signals and missing-link suggestions.
    Graph,
}

impl From<InspectCheck> for talon_core::InspectCheck {
    fn from(check: InspectCheck) -> Self {
        match check {
            InspectCheck::All => Self::All,
            InspectCheck::Orphans => Self::Orphans,
            InspectCheck::BrokenLinks => Self::BrokenLinks,
            InspectCheck::DanglingRefs => Self::DanglingRefs,
            InspectCheck::Unreferenced => Self::Unreferenced,
            InspectCheck::Graph => Self::Graph,
        }
    }
}

/// Arguments for the `inspect` subcommand.
#[derive(Debug, Clone, Args)]
#[command(about = "Inspect your vault for structural signals and patterns.")]
pub struct InspectArgs {
    /// Which inspect check to run (default: all).
    #[arg(value_enum, ignore_case = true)]
    pub check: Option<InspectCheck>,

    #[command(flatten)]
    pub scope: SharedScopeArgs,

    /// Skip LLM-assisted suggestions (faster, deterministic only).
    #[arg(long, short = 'F', global = true)]
    pub fast: bool,
}
