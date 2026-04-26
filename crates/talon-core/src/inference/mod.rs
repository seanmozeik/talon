//! Blocking HTTP client for the TEI-compatible sidecar (`/embed`,
//! `/embed-chunked`, `/rerank`).
//!
//! Ports `clients/sidecar-llm/sidecar.ts`. The wire shapes are dictated by the
//! Python sidecar in `ultra/sidecar/routers/{embed,rerank}.py`, not by the TS
//! client — see [`types`] for details.

pub mod client;
pub mod error;
pub mod types;

pub use client::{DEFAULT_INFERENCE_TIMEOUT, InferenceClient};
pub use error::{InferenceError, MAX_DIAGNOSTIC_CHARS, redact};
pub use types::{
    EmbedChunkedDataItem, EmbedChunkedRequest, EmbedChunkedResponse, EmbedRequest, RerankRequest,
    RerankResult,
};
