# Multi-Provider Inference: Remote Embedding & Reranking

## Problem

Talon's inference layer is locked to a single TEI-compatible sidecar. All embedding and reranking requests go to fixed routes (`/embed`, `/embed-chunked`, `/rerank`) at one `base_url`. This works for self-hosted deployments but prevents:

- Using OpenRouter to access models like `nomic-ai/nomic-embed-text-v2` or `BAAI/bge-m3`
- Using Cohere, Jina AI, Voyage AI, or other hosted embedding/reranking providers
- A/B testing different embedding models without spinning up a new sidecar

The expansion client already supports this pattern (`provider`, `base_url`, `model` in request body). Embedding and reranking need the same treatment.

## Goals

1. Support any OpenAI-compatible embedding endpoint (OpenRouter, local Ollama/LM Studio, etc.)
2. Support major remote reranking providers (Cohere via OpenRouter, Jina AI)
3. Per-provider authentication (API keys, bearer tokens)
4. Graceful degradation: if a provider is unreachable, fall back to hybrid results without semantic/rerank scores
5. Backward compatible: existing `talon.toml` with `[inference]` pointing at a TEI sidecar continues to work unchanged

## Non-Goals

- Local model inference (keep using TEI sidecar for that)
- Provider-specific prompt engineering or model parameter tuning beyond what the API requires
- Caching provider responses differently from existing `rerank_cache` logic

---

## Architecture

### Configuration Changes

#### `talon.toml` — New `[inference]` structure

Current config:
```toml
[inference]
base_url = "http://localhost:8080"

[inference.models]
query_embedding = "embed"
document_embedding = "embed"
chunk_embedding = "embed_chunked"
reranker = "rerank"
```

New config (supports both TEI and remote providers):
```toml
[inference]
# Default: "tei" for backward compatibility.
# Set to "openai-compatible" to use OpenAI-compatible embedding endpoints.
provider = "tei"

base_url = "http://localhost:8080"

# Optional API key (required by some providers, ignored by TEI)
api_key = ""

[inference.models]
query_embedding = "embed"
document_embedding = "embed"
chunk_embedding = "embed_chunked"
reranker = "rerank"
```

For remote providers:
```toml
[inference]
provider = "openai-compatible"
base_url = "https://openrouter.ai/api/v1"
api_key = "sk-or-v1-..."  # or set via TALON_INFERENCE_API_KEY env var

[inference.models]
query_embedding = "nomic-ai/nomic-embed-text-v2"
document_embedding = "nomic-ai/nomic-embed-text-v2"
chunk_embedding = "nomic-ai/nomic-embed-text-v2"
reranker = "colbert-ir/bge-reranker-v2-m3:free"  # OpenRouter model ID
```

#### Rust types — `talon-core/src/config.rs`

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceConfig {
    /// Provider type: "tei" or "openai-compatible"
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Base URL for the inference endpoint.
    pub base_url: String,

    /// API key for authenticated providers.
    /// Environment variable fallback: `TALON_INFERENCE_API_KEY`.
    #[serde(default)]
    pub api_key: Option<String>,

    /// Model names used by the endpoint.
    pub models: InferenceModels,
}

fn default_provider() -> String {
    "tei".to_string()
}
```

### Provider Abstraction Layer

New module: `talon-core/src/inference/provider/`

```
inference/
├── mod.rs              ← re-exports from submodules
├── client.rs           ← existing TEI client (unchanged)
├── error.rs            ← existing errors + new variant
├── types.rs            ← existing wire types
└── provider/           ← NEW
    ├── mod.rs          ← Provider enum + dispatch trait
    ├── tei.rs          ← existing TEI implementation (factored out)
    └── openai_compat.rs← OpenAI-compatible embedding/rerank client
```

#### Provider Enum

```rust
#[derive(Debug, Clone)]
pub enum InferenceProvider {
    Tei(InferenceClient),           // existing TEI client
    OpenAiCompatible(OpenAiCompatClient),
}
```

#### Dispatch Trait

```rust
pub trait EmbeddingProvider: Send + Sync {
    /// Embed a batch of texts. Returns one vector per input.
    async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, InferenceError>;

    /// Embed grouped chunks (one group = one note).
    async fn embed_chunked(
        &self,
        input: &[Vec<String>],
    ) -> Result<EmbedChunkedResponse, InferenceError>;
}

pub trait RerankingProvider: Send + Sync {
    /// Rerank candidates against a query. Returns scored results.
    async fn rerank(
        &self,
        query: &str,
        texts: &[String],
    ) -> Result<Vec<RerankResult>, InferenceError>;
}
```

The existing `InferenceClient` implements both traits for the TEI path. The new `OpenAiCompatClient` implements them for the OpenAI-compatible path.

### OpenAI-Compatible Client — `openai_compat.rs`

#### Embedding Request Shape

OpenAI-compatible embedding endpoints expect:
```json
{
  "model": "nomic-ai/nomic-embed-text-v2",
  "input": ["text one", "text two"]
}
```

Response:
```json
{
  "data": [
    {"embedding": [0.1, 0.2, ...], "index": 0},
    {"embedding": [-0.3, 0.4, ...], "index": 1}
  ],
  "model": "nomic-ai/nomic-embed-text-v2",
  "usage": {"prompt_tokens": 12, "total_tokens": 12}
}
```

#### Reranking Request Shape — Provider-Specific

Different providers use different request shapes for reranking. The client dispatches based on the model name or an explicit `rerank_provider` sub-field:

**Cohere (via OpenRouter):**
```json
{
  "model": "colbert-ir/bge-reranker-v2-m3:free",
  "query": "search query",
  "documents": ["doc one", "doc two"],
  "top_n": 10
}
```
Response: `{"results": [{"document": {"text": "..."}, "index": 0, "relevance_score": 0.95}]}`

**Jina AI:**
```json
{
  "model": "jina-reranker-v2-base-multilingual",
  "query": "search query",
  "documents": ["doc one", "doc two"]
}
```
Response: `{"results": [{"index": 0, "relevance_score": 0.95}]}`

**OpenAI-compatible rerankers (e.g., via OpenRouter):**
Some providers accept the Cohere shape; others need a custom format. The dispatch logic maps model prefixes to request/response shapes.

#### Authentication

API key handling:
- Read from `config.api_key` (TOML) or `TALON_INFERENCE_API_KEY` env var
- Sent as `Authorization: Bearer <key>` header
- TEI provider ignores the header (sidecars typically use their own auth)
- OpenAI-compatible providers require it for most hosted endpoints

```rust
fn auth_header(&self) -> Option<String> {
    self.api_key.as_ref().map(|k| format!("Bearer {}", k))
}
```

### Error Handling

New `InferenceError` variants:
```rust
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum InferenceError {
    // ... existing: Build, Http, Decode ...
    
    /// Provider returned an error (4xx/5xx with body).
    Provider { status: StatusCode, message: String },
    
    /// Model name not found in provider's model list.
    UnknownModel { model: String, provider: String },
    
    /// Authentication failed.
    Unauthorized { provider: String },
}
```

### Integration Points

#### 1. `embed/runner.rs` — Embed Pass

No changes needed at the pipeline level. The runner calls `InferenceClient::embed()` and `InferenceClient::embed_chunked()`. These methods become dispatchers that forward to the provider-specific implementation.

Current call site:
```rust
let embeddings = inference.embed(&chunk_texts)?;
```

New call site (same surface, internal dispatch):
```rust
let embeddings = inference.provider.embed(&chunk_texts).await?;
```

#### 2. `search/rerank_pipeline.rs` — Reranking Pipeline

Same pattern. The pipeline calls `inference.rerank()`. The method dispatches to the provider.

Current call site:
```rust
let Ok(rerank_results) = inference.rerank(query, &missing_texts, false) else {
    return active;  // graceful degradation
};
```

New call site (same surface):
```rust
let Ok(rerank_results) = inference.provider.rerank(query, &missing_texts).await else {
    return active;  // graceful degradation — unchanged
};
```

#### 3. `cli/command/sync.rs` and `cli/command/search.rs`

These construct the `InferenceClient` from config. They need to be updated to dispatch based on `config.inference.provider`:

```rust
let inference = match config.inference.provider.as_str() {
    "tei" => InferenceProvider::Tei(InferenceClient::new(&config.inference.base_url)?),
    "openai-compatible" => InferenceProvider::OpenAiCompatible(
        OpenAiCompatClient::new(
            &config.inference.base_url,
            &config.inference.api_key,
        )?,
    ),
    other => bail!("unknown inference provider: {}", other),
};
```

### Environment Variables

New env var for API key (takes precedence over TOML):
- `TALON_INFERENCE_API_KEY` — API key for the inference provider

Existing vars continue to work:
- `TALON_CONFIG_FILE` — config file path override
- `OBSIDIAN_VAULT_PATH` — vault path

### Backward Compatibility

- Default `provider = "tei"` ensures existing configs work without changes
- TEI request shapes are unchanged (`{inputs}`, `{query, texts}`)
- Model names in `[inference.models]` are ignored by TEI (they're metadata only) for both paths
- If `api_key` is empty and provider is `openai-compatible`, requests proceed without auth (some local OpenAI-compatible servers like Ollama/LM Studio don't require it)

### Testing Strategy

1. **Unit tests** — Mock each provider's HTTP responses. Test request shape correctness per provider.
2. **Integration tests** — Use `wiremock` to mock both TEI and OpenAI-compatible endpoints. Verify correct dispatch based on config.
3. **End-to-end** — Run with Ollama (no auth) and verify embedding works. Run with a local OpenRouter proxy if available.

### Migration Path for Users

1. **TEI sidecar users**: No action needed. Config continues to work as-is.
2. **Ollama/LM Studio users**: Add `provider = "openai-compatible"` and set `base_url`. No API key needed.
3. **OpenRouter users**: Set `provider = "openai-compatible"`, `base_url = "https://openrouter.ai/api/v1"`, and `api_key` (or env var). Use OpenRouter model IDs in `[inference.models]`.
4. **Cohere/Jina users**: Same as OpenRouter — set provider to `openai-compatible` with the respective base URL and API key.

---

## Files That Change

| File | Change Type | Description |
|------|-------------|-------------|
| `crates/talon-core/src/config.rs` | Modify | Add `provider` and `api_key` to `InferenceConfig` |
| `crates/talon-core/src/inference/mod.rs` | Modify | Re-export new provider module |
| `crates/talon-core/src/inference/client.rs` | Refactor | Move TEI logic into `provider/tei.rs`, keep as impl of traits |
| `crates/talon-core/src/inference/error.rs` | Modify | Add `Provider`, `UnknownModel`, `Unauthorized` variants |
| `crates/talon-core/src/inference/types.rs` | Unchanged | Wire types stay the same (consumers normalize) |
| `crates/talon-core/src/inference/provider/mod.rs` | **New** | Provider enum + dispatch trait definitions |
| `crates/talon-core/src/inference/provider/tei.rs` | **New** | TEI implementation (factored from client.rs) |
| `crates/talon-core/src/inference/provider/openai_compat.rs` | **New** | OpenAI-compatible embedding + rerank client |
| `crates/talon-core/src/embed/runner.rs` | Modify | Call provider trait methods instead of direct client |
| `crates/talon-core/src/search/rerank_pipeline.rs` | Modify | Call provider trait methods instead of direct client |
| `crates/talon-cli/src/config.rs` | Modify | Update config template with new fields |
| `crates/talon-cli/src/command/sync.rs` | Modify | Provider dispatch when constructing client |
| `crates/talon-cli/src/command/search.rs` | Modify | Provider dispatch when constructing client |
| `crates/talon-cli/src/mcp/tool/dispatch.rs` | Modify | Provider dispatch for MCP tools |
| `crates/talon-cli/src/mcp/tool/sync.rs` | Modify | Provider dispatch for sync tool |
| `docs/MULTI-PROVIDER-INFERENCE.md` | **New** | This document |
