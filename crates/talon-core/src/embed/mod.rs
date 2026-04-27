//! Embedding pipeline: pulls pending chunks from `chunks`, calls the
//! TEI sidecar, and writes vectors back to `vec_chunks` + `vector_metadata`.
//!
//! Ports `services/talon/embed/*.ts`. The pipeline preserves two paths from
//! the TS reference because they call different sidecar endpoints:
//!
//! - **Single-chunk** notes go to `/embed` (a flat `Vec<Vec<f32>>` response).
//! - **Multi-chunk** notes go to `/embed-chunked` so the sidecar can keep all
//!   chunks of one note in the same forward pass — important for context
//!   models where neighbour chunks influence each other's representation.
//!
//! Dimensionality is discovered on the first successful row and pinned for
//! the rest of the pass; mismatches mark the offending note's chunks as
//! `failed` and disable semantic search until the operator runs `talon embed`
//! again with a consistent model.

pub mod diagnostics;
pub mod pending;
pub mod persist;
pub mod quantize;
pub mod runner;

pub use diagnostics::{EmbedDiagnostics, EmbedRunContext, MAX_DIAGNOSTICS};
pub use pending::{ChunkInfo, MAX_PENDING_CHUNKS_PER_PASS, NoteWithChunks, get_pending_chunks};
pub use persist::{first_non_empty_batch, persist_chunk_vector};
pub use runner::{
    DIMENSION_MISMATCH_REMEDIATION, EmbedPassOptions, EmbedPassStats, run_embed_pass,
};
