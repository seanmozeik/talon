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
[ask]
model = "qwen-smol"
planning_reasoning_effort = "none"
synthesis_reasoning_effort = "medium"
```

Ultraclaw does not inject, adapt, or validate Talon config.
