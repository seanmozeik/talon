//! Wire types for the TEI-compatible sidecar.
//!
//! Mirrors `clients/sidecar-llm/sidecar.ts` and `sidecar/routers/{embed,rerank}.py`.
//! The Python sidecar is the source of truth: `/embed` returns a bare
//! `[[f32]]` array, `/embed-chunked` returns `{data: [{embeddings, index}], model}`,
//! and `/rerank` returns `[{index, score}]` where the sidecar has already
//! resolved any model-specific label layout into a single normalized relevance score
//! (TEI client sends `return_text` even though the field is inert server-side).

use serde::{Deserialize, Serialize};

/// `POST /embed` request body.
///
/// The sidecar accepts a single string or a list of strings; we always send a
/// list to keep the response shape uniform.
#[derive(Debug, Clone, Serialize)]
pub struct EmbedRequest {
    /// Texts to embed.
    pub inputs: Vec<String>,
}

/// `POST /embed-chunked` request body.
///
/// Outer list is one entry per note; inner list is the chunks for that note.
/// The sidecar enforces `MAX_EMBED_CHUNKED_GROUPS` and `MAX_EMBED_CHUNKS_PER_GROUP`
/// (HTTP 413 on overflow).
#[derive(Debug, Clone, Serialize)]
pub struct EmbedChunkedRequest {
    /// Groups of chunks (one group per note).
    pub input: Vec<Vec<String>>,
}

/// One row of `/embed-chunked` response data.
#[derive(Debug, Clone, Deserialize)]
pub struct EmbedChunkedDataItem {
    /// Per-chunk embedding vectors for this group.
    pub embeddings: Vec<Vec<f32>>,
    /// Group index (matches request order).
    pub index: u32,
}

/// `/embed-chunked` response envelope.
#[derive(Debug, Clone, Deserialize)]
pub struct EmbedChunkedResponse {
    /// Per-group embeddings.
    pub data: Vec<EmbedChunkedDataItem>,
    /// Model name as reported by the sidecar.
    pub model: String,
}

/// `POST /rerank` request body.
///
/// `return_text` is accepted by common reranker endpoints. TEI-style fields are
/// optional so the client can target either minimal sidecars or full TEI routes.
#[derive(Debug, Clone, Serialize)]
pub struct RerankRequest {
    /// Query text.
    pub query: String,
    /// Candidate texts to rerank.
    pub texts: Vec<String>,
    /// TEI-compatible score-scale flag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_scores: Option<bool>,
    /// TEI-compatible truncation flag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncate: Option<bool>,
    /// TEI compatibility flag (server returns index+score either way).
    pub return_text: bool,
}

/// One reranker score.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct RerankResult {
    /// Index into the request's `texts` array.
    pub index: u32,
    /// Cross-encoder relevance score emitted by the sidecar after label extraction.
    pub score: f32,
}

/// OpenAI-compatible `POST /embeddings` request body.
#[derive(Debug, Clone, Serialize)]
pub struct OpenAiEmbeddingRequest {
    pub model: String,
    pub input: Vec<String>,
}

/// One row of an OpenAI-compatible embeddings response.
#[derive(Debug, Clone, Deserialize)]
pub struct OpenAiEmbeddingDataItem {
    pub embedding: Vec<f32>,
    pub index: u32,
}

/// OpenAI-compatible embeddings response envelope.
#[derive(Debug, Clone, Deserialize)]
pub struct OpenAiEmbeddingResponse {
    pub data: Vec<OpenAiEmbeddingDataItem>,
    pub model: String,
}

/// Cohere-style `POST /rerank` request body.
#[derive(Debug, Clone, Serialize)]
pub struct CohereRerankRequest {
    pub model: String,
    pub query: String,
    pub documents: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_n: Option<u32>,
}

/// One row of a Cohere-style rerank response.
#[derive(Debug, Clone, Deserialize)]
pub struct CohereRerankResult {
    pub index: u32,
    pub relevance_score: f32,
}

/// Cohere-style rerank response envelope.
#[derive(Debug, Clone, Deserialize)]
pub struct CohereRerankResponse {
    pub results: Vec<CohereRerankResult>,
}
