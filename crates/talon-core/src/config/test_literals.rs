//! Shared [`TalonConfig`] literals for unit tests.

use std::path::{Path, PathBuf};

use super::{
    ChatAdapter, ChatAskConfig, ChatExpansionConfig, ChatSection, ChunkerConfig, CredentialsConfig,
    EmbeddingAdapter, EmbeddingConfig, EndpointAuthConfig, InspectConfig, McpConfig, RerankAdapter,
    RerankConfig, RerankScoreScale, ScopesConfig, SearchConfig, TalonConfig,
};

/// Builds a minimal config with default scopes and the given HTTP base URL.
#[must_use]
pub fn minimal(base_url: impl Into<String>) -> TalonConfig {
    minimal_for_paths(
        PathBuf::from("/vault"),
        PathBuf::from("/vault/.talon/index.db"),
        base_url,
        ScopesConfig::default(),
    )
}

/// Builds a minimal config for explicit vault/db paths.
#[must_use]
pub fn minimal_for_paths(
    vault_path: PathBuf,
    db_path: PathBuf,
    base_url: impl Into<String>,
    scopes: ScopesConfig,
) -> TalonConfig {
    let base_url = base_url.into();
    let chat_base = if base_url.ends_with("/v1") {
        base_url.clone()
    } else {
        format!("{base_url}/v1")
    };
    TalonConfig {
        vault_path,
        db_path,
        config_file_path: None,
        include_patterns: Vec::new(),
        ignore_patterns: Vec::new(),
        credentials: CredentialsConfig::default(),
        embedding: EmbeddingConfig {
            base_url: base_url.clone(),
            auth: EndpointAuthConfig::default(),
            adapter: EmbeddingAdapter::Tei,
            model: "embed".to_string(),
            document_model: Some("embed_chunked".to_string()),
            context_tokens: 512,
        },
        rerank: RerankConfig {
            base_url,
            auth: EndpointAuthConfig::default(),
            adapter: RerankAdapter::Minimal,
            model: "rerank".to_string(),
            score_scale: RerankScoreScale::default(),
            truncate: true,
        },
        chat: ChatSection {
            expansion: ChatExpansionConfig {
                base_url: chat_base,
                auth: EndpointAuthConfig::default(),
                adapter: ChatAdapter::default(),
                model: "test".to_string(),
                context_tokens: 32_768,
                max_output_tokens: None,
            },
            ask: ChatAskConfig::default(),
        },
        mcp: McpConfig::default(),
        scopes,
        search: SearchConfig::default(),
        inspect: InspectConfig::default(),
        chunker: ChunkerConfig::default(),
    }
}

/// Builds a minimal config rooted at `vault` with the default sidecar URL.
#[must_use]
pub fn minimal_for_vault(vault: &Path) -> TalonConfig {
    minimal_for_paths(
        vault.to_path_buf(),
        vault.join("idx.sqlite"),
        "http://localhost:1",
        ScopesConfig::default(),
    )
}
