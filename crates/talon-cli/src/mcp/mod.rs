//! MCP-over-stdio transport for Talon.
//!
//! This module hand-rolls the small JSON-RPC 2.0 subset Talon needs instead of
//! pulling in `rmcp`: this story only needs line-delimited framing and a few
//! lifecycle methods, while Talon's tool schema and dispatch are added in the
//! next story. Keeping the transport explicit makes the wire contract easy to
//! test and avoids committing the CLI to a larger server abstraction too early.

pub mod background;
pub mod diagnostics;
pub mod protocol;
pub mod session;
pub mod state;
pub mod tool;
pub mod transport;

pub use transport::{TransportOutcome, run_jsonrpc_loop};
