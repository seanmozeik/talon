//! Query handlers for the Talon CLI.
//!
//! This module contains the real implementations of search, read, related,
//! meta, changes, and inspect handlers — replacing the CLI stubs.

pub mod changes;
pub mod input;
pub mod inspect;
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod inspect_tests;
pub mod meta;
pub mod mtime;
pub mod output;
pub mod read;
pub mod recall;
pub mod recall_scoring;
pub mod related;
pub mod search;
mod search_affordances;
mod search_filter;
mod search_graph;
mod search_hybrid;
mod search_retrieval;
pub mod status;
pub(crate) mod where_filter;

pub use changes::query_changes;
pub use input::{ChangesInput, MetaInput, ReadInput, RecallFormat, RecallInput};
pub use inspect::query_inspect;
pub use meta::query_meta;
pub use output::{
    AskDiagnostics, AskLlmStageDiagnostics, AskResponse, AskSearchDiagnostics, AskSource,
    ChangeEntry, ChangesResponse, LinkedNote, MetaEntry, MetaResponse, NoteExcerpt, ReadResponse,
    ReadResult, ReadSection, RecallDiagnostics, RecallResponse, TombstoneEntry, VaultRecall,
};
pub use read::run_read;
pub use recall::run_recall;
pub use related::{RelatedInput, RelatedResponse, RelatedResult, RelationKind, find_related};
pub use search::{run_search, run_search_with_expanded_queries};
pub use status::query_status;
