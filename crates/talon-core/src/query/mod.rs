//! Query handlers for the Talon CLI.
//!
//! This module contains the real implementations of search, read, related,
//! meta, changes, and lint handlers — replacing the CLI stubs.

pub mod changes;
pub mod meta;
pub mod read;
pub mod related;
pub mod search;
pub(crate) mod where_filter;

pub use changes::query_changes;
pub use meta::query_meta;
pub use read::run_read;
pub use related::find_related;
pub use search::run_search;
