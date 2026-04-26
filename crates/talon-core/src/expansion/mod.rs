//! OpenAI-compatible LLM client for hybrid search query expansion.
//!
//! Ports the `expandSearchQueries` logic from
//! `clients/sidecar-llm/local-llm.ts` in the TS reference.  The sidecar
//! exposes a `/chat/completions` endpoint; this module provides a blocking
//! client that requests reformulated search variants, normalises the
//! response, and degrades gracefully on any LLM-quality failure.

pub mod cache;
pub mod client;
pub mod error;
pub mod types;

pub use cache::LlmCache;
pub use client::{DEFAULT_EXPANSION_TIMEOUT, ExpansionClient};
pub use error::ExpansionError;
