//! Query handlers for the Talon CLI.
//!
//! This module contains the real implementations of search, read, related,
//! meta, changes, and lint handlers — replacing the CLI stubs.

pub mod read;
pub mod related;
pub mod search;

pub use read::run_read;
pub use related::find_related;
pub use search::run_search;
