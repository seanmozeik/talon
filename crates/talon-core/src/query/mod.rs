//! Query handlers for the Talon CLI.
//!
//! This module contains the real implementations of search, read, related,
//! meta, changes, and lint handlers — replacing the CLI stubs.

pub mod changes;
pub mod input;
pub mod lint;
pub mod meta;
pub mod output;
pub mod read;
pub mod recall;
pub mod recall_scoring;
pub mod related;
pub mod search;
pub mod status;
pub(crate) mod where_filter;

pub use changes::query_changes;
pub use input::{ChangesInput, MetaInput, ReadInput, RecallFormat, RecallInput};
pub use lint::query_lint;
pub use meta::query_meta;
pub use output::{
    ChangeEntry, ChangesResponse, EditedNote, FrontmatterFact, FuzzyAnchor, LinkedNote, MetaEntry,
    MetaResponse, NoteExcerpt, ReadResponse, ReadResult, RecallResponse, TombstoneEntry,
    VaultRecall,
};
pub use read::run_read;
pub use recall::run_recall;
pub use related::{RelatedInput, RelatedResponse, RelatedResult, RelationKind, find_related};
pub use search::run_search;
pub use status::query_status;
