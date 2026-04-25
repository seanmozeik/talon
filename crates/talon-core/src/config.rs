//! Configuration model for standalone and federated Talon processes.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Full Talon runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TalonConfig {
    /// Host or standalone vault path.
    pub vault_path: PathBuf,
    /// `SQLite` index path.
    pub db_path: PathBuf,
    /// Whether startup should index before serving.
    pub index_on_start: bool,
    /// Whether `talon --mcp` should run the watcher and embed scheduler.
    pub watch: bool,
    /// Local wall-clock embedding schedule entries such as `03:00`.
    pub embedding_schedule: Vec<String>,
    /// Glob-style include patterns.
    pub include_patterns: Vec<String>,
    /// Glob-style ignore patterns.
    pub ignore_patterns: Vec<String>,
    /// Embedding and rerank endpoint configuration.
    pub inference: InferenceConfig,
    /// Query expansion endpoint configuration.
    pub expansion: ExpansionConfig,
}

impl TalonConfig {
    /// Returns the configured vault path.
    #[must_use]
    pub fn vault_path(&self) -> &Path {
        &self.vault_path
    }

    /// Returns the configured database path.
    #[must_use]
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }
}

/// TEI-compatible inference endpoint configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct InferenceConfig {
    /// Base URL for TEI-compatible routes.
    pub base_url: String,
    /// Model names used by the endpoint.
    pub models: InferenceModels,
}

/// Inference model names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct InferenceModels {
    /// Query embedding model.
    pub query_embedding: String,
    /// Document embedding model.
    pub document_embedding: String,
    /// Chunk embedding model.
    pub chunk_embedding: String,
    /// Reranker model.
    pub reranker: String,
}

/// OpenAI-compatible query expansion configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ExpansionConfig {
    /// Provider label, such as `openai-compatible`.
    pub provider: String,
    /// Chat-completions-compatible base URL.
    pub base_url: String,
    /// Expansion model name.
    pub model: String,
}
