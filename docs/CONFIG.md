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

`[inference.rerank]` controls the `/rerank` protocol Talon expects from the
sidecar or adapter.

```toml
[inference.models]
query_embedding_context_tokens = 512
reranker_context_tokens = 512

[inference.rerank]
request_shape = "minimal"  # minimal | tei
score_scale = "normalized" # normalized | logits
truncate = true            # sent only for request_shape = "tei"
```

The minimal request shape is:
`POST /rerank { query, texts, return_text } -> [{ index, score }]`.
Scores should be normalized to `[0, 1]` unless `score_scale = "logits"`.

`[ask]` optionally selects a larger chat model for `talon ask`. It reuses the
OpenAI-compatible `[expansion]` endpoint, so only ask-specific model and
reasoning overrides live here.

```toml
[expansion]
context_tokens = 32768
max_output_tokens = 768

[ask]
model = "qwen-smol"
context_tokens = 65536
max_output_tokens = 2048
planning_reasoning_effort = "none"
synthesis_reasoning_effort = "medium"

[mcp.hooks]
recall_deadline_ms = 45000
```

Recall distillation keeps a safety margin below `expansion.context_tokens`; a
32k context model receives a prompt view below roughly 30k estimated tokens.

Ultraclaw does not inject, adapt, or validate Talon config.
