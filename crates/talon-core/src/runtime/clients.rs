//! HTTP clients built from [`TalonConfig`].

use crate::config::TalonConfig;
use crate::error::TalonError;
use crate::expansion::ExpansionClient;
use crate::inference::{EmbeddingClient, InferenceError, RerankClient};
use crate::llm::ChatClient;

/// Runtime HTTP clients for search, sync, and recall.
#[derive(Debug, Clone)]
pub struct TalonClients {
    /// Embedding endpoint client.
    pub embedding: EmbeddingClient,
    /// Rerank endpoint client.
    pub rerank: RerankClient,
    /// Query expansion chat client.
    pub expansion: ExpansionClient,
}

impl TalonClients {
    /// Builds all runtime clients from config.
    ///
    /// # Errors
    ///
    /// Returns [`TalonError::Config`] when auth resolution fails.
    /// Returns [`TalonError::Internal`] when an HTTP client cannot be built.
    pub fn from_config(config: &TalonConfig) -> Result<Self, TalonError> {
        let embedding = EmbeddingClient::from_config(&config.embedding, &config.credentials)
            .map_err(map_inference_build)?;
        let rerank = RerankClient::from_config(
            &config.rerank,
            &config.credentials,
            config.search.rerank_batch_size,
        )
        .map_err(map_inference_build)?;
        let expansion = build_expansion_client(config).map_err(map_inference_build)?;
        Ok(Self {
            embedding,
            rerank,
            expansion,
        })
    }
}

/// Builds the expansion chat client from `[chat.expansion]`.
///
/// # Errors
///
/// Returns [`InferenceError`] when auth resolution or client construction fails.
pub fn build_expansion_client(config: &TalonConfig) -> Result<ExpansionClient, InferenceError> {
    let chat = build_chat_client(
        &config.chat.expansion.base_url,
        &config.chat.expansion.auth,
        &config.credentials,
        &config.chat.expansion.model,
        config.chat.expansion.max_output_tokens,
    )?;
    Ok(ExpansionClient::from_chat(chat))
}

/// Builds an ask-stage chat client, inheriting transport defaults from expansion.
///
/// # Errors
///
/// Returns [`InferenceError`] when auth resolution or client construction fails.
pub fn build_ask_chat_client(
    config: &TalonConfig,
    model: &str,
    max_tokens: Option<u32>,
) -> Result<ChatClient, InferenceError> {
    let ask = &config.chat.ask;
    let expansion = &config.chat.expansion;
    build_chat_client(
        ask.resolved_base_url(expansion),
        &ask.resolved_auth(expansion),
        &config.credentials,
        model,
        max_tokens,
    )
}

fn build_chat_client(
    base_url: &str,
    auth: &crate::config::EndpointAuthConfig,
    credentials: &crate::config::CredentialsConfig,
    model: &str,
    max_tokens: Option<u32>,
) -> Result<ChatClient, InferenceError> {
    let resolved = auth
        .resolve(credentials)
        .map_err(|err| InferenceError::Config {
            message: err.to_string(),
        })?;
    ChatClient::with_timeout_max_tokens_and_auth(
        base_url,
        model,
        crate::expansion::client::DEFAULT_EXPANSION_TIMEOUT,
        max_tokens,
        resolved,
    )
    .map_err(|err| InferenceError::Build {
        message: err.to_string(),
    })
}

fn map_inference_build(err: InferenceError) -> TalonError {
    match err {
        InferenceError::Config { message } => TalonError::Config { message },
        other => TalonError::Internal {
            message: other.to_string(),
        },
    }
}
