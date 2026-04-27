//! Indexing tool input and output types.

pub mod input;
pub mod output;

pub use input::{LintCheck, LintInput, StatusInput, SyncInput};
pub use output::{
    IndexStats, LintFinding, LintResponse, ScopeReport, StatusResponse, StatusState, SyncResponse,
    SyncStatus,
};
