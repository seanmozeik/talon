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

Ultraclaw does not inject, adapt, or validate Talon config.
