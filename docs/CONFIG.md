# Configuration

Configuration loading is host-owned.

Talon reads `~/.config/talon/config.toml` by default. `--config <path>` and `TALON_CONFIG_FILE=<path>` are reserved for explicit standalone overrides.

[search] controls retrieval defaults and process-local cache/client tunables. CLI flags override these values when a matching flag exists.

```toml
[search]
candidate_limit = 60
limit = 10
cache_size = 200
rerank_cache_size = 2000
rerank_batch_size = 4
rerank_max_tokens = 128
```

`[embedding]` and `[rerank]` configure the HTTP endpoints Talon calls for vectors and cross-encoder scoring. Each block has its own `base_url`, optional auth fields, and `adapter` wire protocol.

```toml
[embedding]
base_url = "http://localhost:8000"
adapter = "tei"              # tei | openai
model = "embed"
document_model = "embed_chunked"
context_tokens = 512

[rerank]
base_url = "http://localhost:8000"
adapter = "minimal"          # minimal | tei | cohere | jina
model = "rerank"
score_scale = "normalized"   # normalized | logits
truncate = true              # sent for tei-style adapters
```

The minimal rerank adapter expects:
`POST /rerank { query, texts, return_text } -> [{ index, score }]`.
Scores should be normalized to `[0, 1]` unless `score_scale = "logits"`.

`[chat.expansion]` configures query expansion and recall distillation. `[chat.ask]` optionally selects a larger chat model for `talon ask`; unset transport fields inherit from expansion.

```toml
[chat.expansion]
base_url = "http://localhost:8000/v1"
model = "bonsai"
context_tokens = 16000
max_output_tokens = 768

[chat.ask]
model = "qwen-smol"
context_tokens = 65536
max_output_tokens = 2048
planning_reasoning_effort = "none"
synthesis_reasoning_effort = "medium"

[mcp.hooks]
recall_deadline_ms = 45000
```

Recall distillation derives its prompt-view budget from
`chat.expansion.context_tokens` minus output, prompt-overhead, and safety reserves.
For small local models, set `context_tokens` to the effective reliable window,
not necessarily the model's advertised maximum.

Named credentials live under `[credentials.*]` and are referenced from capability blocks via `credential = "name"` or inline `api_key_env`.

Ultraclaw does not inject, adapt, or validate Talon config.
