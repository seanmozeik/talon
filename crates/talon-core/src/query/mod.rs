//! Query handlers for the Talon CLI.
//!
//! This module contains the real implementations of search, read, related,
//! meta, changes, and lint handlers — replacing the CLI stubs.

pub mod search;

pub use search::run_search;
