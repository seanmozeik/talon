//! Blocking HTTP clients for embedding and reranking endpoints.

pub mod embedding;
pub mod error;
pub mod http;
pub mod rerank;
pub mod types;

pub use embedding::EmbeddingClient;
pub use error::{InferenceError, MAX_DIAGNOSTIC_CHARS, redact};
pub use http::DEFAULT_INFERENCE_TIMEOUT;
pub use rerank::RerankClient;
pub use types::{
    CohereRerankRequest, CohereRerankResponse, EmbedChunkedDataItem, EmbedChunkedRequest,
    EmbedChunkedResponse, EmbedRequest, OpenAiEmbeddingRequest, OpenAiEmbeddingResponse,
    RerankRequest, RerankResult,
};
