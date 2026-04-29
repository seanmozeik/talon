use super::{CONFIG_TEMPLATE, RefreshLockPolicy, load_config_file};
use eyre::Result;
use std::path::PathBuf;

fn temp_config_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("talon-{label}-{}.toml", std::process::id()))
}

fn temp_dir_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("talon-{label}-{}", std::process::id()))
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
    assert_eq!(
        config.inference.rerank.request_shape,
        talon_core::RerankRequestShape::Minimal
    );
    assert_eq!(
        config.inference.rerank.score_scale,
        talon_core::RerankScoreScale::Normalized
    );
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

[inference.rerank]
request_shape = "tei"
score_scale = "logits"
truncate = false

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
    assert_eq!(
        config.inference.rerank.request_shape,
        talon_core::RerankRequestShape::Tei
    );
    assert_eq!(
        config.inference.rerank.score_scale,
        talon_core::RerankScoreScale::Logits
    );
    assert!(!config.inference.rerank.truncate);
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

#[test]
fn refresh_index_if_needed_skips_when_lock_is_busy_and_policy_allows_it() {
    let root = temp_dir_path("skip-busy-refresh");
    let vault = root.join("vault");
    fs_err::create_dir_all(&vault).unwrap_or_else(|err| panic!("create vault: {err}"));
    let db = root.join("index.sqlite");
    let mut config = super::default_config_for_vault(vault);
    config.db_path = db.clone();
    let lock_path = super::sync_lock_path(&config);
    let _lock =
        talon_core::acquire_sync_lock(&lock_path).unwrap_or_else(|err| panic!("lock: {err}"));
    let mut conn =
        talon_core::open_database(&db).unwrap_or_else(|err| panic!("open database: {err}"));

    super::refresh_index_if_needed(&config, &mut conn, false, RefreshLockPolicy::SkipIfBusy)
        .unwrap_or_else(|err| panic!("busy refresh should be skipped: {err}"));

    drop(conn);
    let _ = fs_err::remove_dir_all(root);
}

#[test]
fn refresh_index_if_needed_errors_when_lock_is_busy_and_policy_requires_it() {
    let root = temp_dir_path("error-busy-refresh");
    let vault = root.join("vault");
    fs_err::create_dir_all(&vault).unwrap_or_else(|err| panic!("create vault: {err}"));
    let db = root.join("index.sqlite");
    let mut config = super::default_config_for_vault(vault);
    config.db_path = db.clone();
    let lock_path = super::sync_lock_path(&config);
    let _lock =
        talon_core::acquire_sync_lock(&lock_path).unwrap_or_else(|err| panic!("lock: {err}"));
    let mut conn =
        talon_core::open_database(&db).unwrap_or_else(|err| panic!("open database: {err}"));

    let Err(err) =
        super::refresh_index_if_needed(&config, &mut conn, false, RefreshLockPolicy::ErrorIfBusy)
    else {
        panic!("busy refresh should fail when policy requires it");
    };

    assert!(
        err.to_string().contains("auto-refresh failed"),
        "unexpected error: {err}"
    );
    drop(conn);
    let _ = fs_err::remove_dir_all(root);
}
