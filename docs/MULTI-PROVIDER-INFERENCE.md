# Multi-Provider HTTP Endpoints

Talon configures embedding, reranking, and chat as **independent HTTP capabilities**. Each block carries its own `base_url`, optional credential reference, `adapter` (wire protocol), and model slug(s).

There is no monolithic `[inference]` table and no named provider profiles — only optional `[credentials.*]` blocks to deduplicate API keys.

## Configuration

```toml
[credentials.openrouter]
api_key_env = "OPENROUTER_API_KEY"

[credentials.openai]
api_key_env = "OPENAI_API_KEY"

[embedding]
base_url = "https://openrouter.ai/api/v1"
credential = "openrouter"
adapter = "openai"
model = "openai/text-embedding-3-small"
document_model = "openai/text-embedding-3-small"
context_tokens = 8192

[rerank]
base_url = "http://localhost:8000"
adapter = "minimal"
model = "rerank"
score_scale = "normalized"

[chat.expansion]
base_url = "https://api.openai.com/v1"
credential = "openai"
model = "gpt-4o-mini"
context_tokens = 128000
max_output_tokens = 768

[chat.ask]
model = "gpt-4o"
context_tokens = 128000
max_output_tokens = 4096
```

Unset `[chat.ask]` transport fields inherit from `[chat.expansion]`.

## Adapters

| Capability | `adapter` | HTTP |
|------------|-----------|------|
| embedding | `tei` | `/embed`, `/embed-chunked` |
| embedding | `openai` | `/embeddings` (chunked notes emulated client-side) |
| rerank | `minimal` / `tei` | `/rerank` with TEI/minimal bodies |
| rerank | `cohere` / `jina` | `/rerank` with Cohere-style `{ query, documents }` |
| chat | `openai` | `/chat/completions` with Bearer auth |

OpenRouter uses `adapter = "openai"` or `cohere` with `base_url = "https://openrouter.ai/api/v1"` and OpenRouter model slugs.

## Runtime wiring

`TalonClients::from_config` builds `EmbeddingClient`, `RerankClient`, and `ExpansionClient` at CLI/MCP boundaries. Search and sync pass split clients into the pipeline; graceful degradation when a capability is unavailable is unchanged.

Switching embedding models with different dimensions requires `talon embed --force`.
