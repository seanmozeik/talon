use super::{CONFIG_TEMPLATE, load_config_file};
use eyre::Result;
use std::path::PathBuf;

fn temp_config_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("talon-{label}-{}.toml", std::process::id()))
}

#[test]
fn default_config_for_vault_uses_workspace_db_under_home_talon() {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let config = super::default_config_for_vault(PathBuf::from("/vaults/My Notes!"));

    assert_eq!(
        config.db_path,
        home.join(".talon").join("my-notes.db"),
        "workspace db path should be sanitized under ~/.talon"
    );
}

fn load_config_str(label: &str, config: &str) -> Result<talon_core::TalonConfig> {
    let path = temp_config_path(label);
    fs_err::write(&path, config)?;
    let result = load_config_file(&path);
    let _ = fs_err::remove_file(path);
    result
}

#[test]
fn config_template_parses_indexer_chunk_settings() {
    let config = match load_config_str("template-config", CONFIG_TEMPLATE) {
        Ok(config) => config,
        Err(err) => panic!("template config should parse: {err}"),
    };

    assert_eq!(config.chunker.chunk_tokens, 512);
    assert_eq!(config.chunker.chunk_overlap, 64);
    assert_eq!(config.chunker.chunk_min_tokens, 16);
    assert_eq!(config.search.candidate_limit, 60);
    assert_eq!(config.search.limit, 10);
    assert_eq!(config.search.cache_size, 200);
    assert_eq!(config.search.rerank_cache_size, 2000);
    assert_eq!(config.search.rerank_batch_size, 4);
    assert_eq!(config.search.rerank_max_tokens, 128);
    assert!(
        config.db_path.is_absolute(),
        "template db_path should load as an absolute path, got {}",
        config.db_path.display()
    );
}

#[test]
fn load_config_file_parses_search_tunables() {
    let config = r#"
vault_path = "/tmp/vault"
db_path = "/tmp/index.sqlite"

[search]
candidate_limit = 60
limit = 10
cache_size = 200
rerank_cache_size = 2000
rerank_batch_size = 4
rerank_max_tokens = 128

[inference]
base_url = "http://localhost:8080"

[inference.models]
query_embedding = "embed"
document_embedding = "embed"
chunk_embedding = "embed_chunked"
reranker = "rerank"

[expansion]
provider = "openai-compatible"
base_url = "http://localhost:1234/v1"
model = "gemma-smol"
"#;

    let config = load_config_str("search-tunables", config)
        .unwrap_or_else(|err| panic!("config should load: {err}"));

    assert_eq!(config.search.candidate_limit, 60);
    assert_eq!(config.search.limit, 10);
    assert_eq!(config.search.cache_size, 200);
    assert_eq!(config.search.rerank_cache_size, 2000);
    assert_eq!(config.search.rerank_batch_size, 4);
    assert_eq!(config.search.rerank_max_tokens, 128);
}

#[test]
fn load_config_file_resolves_relative_paths_from_config_dir() {
    let config = r#"
vault_path = "vault"
db_path = "state/index.sqlite"

[indexer]
chunk_tokens = 512
chunk_overlap = 64
chunk_min_tokens = 16

[inference]
base_url = "http://localhost:8080"

[inference.models]
query_embedding = "embed"
document_embedding = "embed"
chunk_embedding = "embed_chunked"
reranker = "rerank"

[expansion]
provider = "openai-compatible"
base_url = "http://localhost:1234/v1"
model = "gemma-smol"
"#;
    let path = temp_config_path("relative-paths");
    fs_err::write(&path, config).unwrap_or_else(|err| panic!("write config: {err}"));

    let loaded = load_config_file(&path).unwrap_or_else(|err| panic!("load config: {err}"));

    let base = path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/tmp"));
    assert_eq!(loaded.vault_path, base.join("vault"));
    assert_eq!(loaded.db_path, base.join("state").join("index.sqlite"));
    let _ = fs_err::remove_file(path);
}

#[test]
fn load_config_file_rejects_invalid_chunk_overlap() {
    let config = r#"
vault_path = "/tmp/vault"
db_path = "/tmp/index.sqlite"

[indexer]
chunk_tokens = 64
chunk_overlap = 64
chunk_min_tokens = 16

[inference]
base_url = "http://localhost:8080"

[inference.models]
query_embedding = "embed"
document_embedding = "embed"
chunk_embedding = "embed_chunked"
reranker = "rerank"

[expansion]
provider = "openai-compatible"
base_url = "http://localhost:1234/v1"
model = "gemma-smol"
"#;
    let Err(err) = load_config_str("invalid-chunk-overlap", config) else {
        panic!("invalid chunk overlap should fail");
    };
    assert!(
        err.chain().any(|cause| cause
            .to_string()
            .contains("indexer.chunk_overlap must be less than indexer.chunk_tokens")),
        "unexpected error: {err}"
    );
}

#[test]
fn load_config_file_rejects_non_canonical_names() {
    for (label, config) in [
        (
            "camel-case-compat",
            r#"
vaultPath = "/tmp/vault"
dbPath = "/tmp/index.sqlite"

[chunker]
chunkTokens = 512
chunkOverlap = 64
chunkMinTokens = 16

[inference]
baseUrl = "http://localhost:8080"

[inference.models]
queryEmbedding = "embed"
documentEmbedding = "embed"
chunkEmbedding = "embed_chunked"
reranker = "rerank"

[expansion]
provider = "openai-compatible"
baseUrl = "http://localhost:1234/v1"
model = "gemma-smol"
"#,
        ),
        (
            "chunker-table-alias",
            r#"
vault_path = "/tmp/vault"
db_path = "/tmp/index.sqlite"

[chunker]
chunk_tokens = 512
chunk_overlap = 64
chunk_min_tokens = 16

[inference]
base_url = "http://localhost:8080"

[inference.models]
query_embedding = "embed"
document_embedding = "embed"
chunk_embedding = "embed_chunked"
reranker = "rerank"

[expansion]
provider = "openai-compatible"
base_url = "http://localhost:1234/v1"
model = "gemma-smol"
"#,
        ),
    ] {
        let Err(err) = load_config_str(label, config) else {
            panic!("{label} should fail");
        };
        assert!(err.to_string().contains("failed to parse config file"));
    }
}
