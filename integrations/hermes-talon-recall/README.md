# hermes-talon-recall

Hermes Agent Memory Provider plugin that wraps [`talon recall --format prompt-xml`](../../docs/recall.md) to surface vault-native context from an Obsidian knowledge base on every agent turn.

Talon is **recall-only** and stateless per call. The plugin implements `prefetch()` to retrieve relevant context; the agent host (your Obsidian editor or a cron-driven agent) owns vault mutations.

---

## Installation

**Drop-in:**
```
cp -r integrations/hermes-talon-recall ~/.hermes/plugins/talon-recall
```

**Dev install (editable):**
```
pip install -e integrations/hermes-talon-recall
```

**PyPI (once published):**
```
pip install hermes-talon-recall
```

The talon binary must be on `PATH` or pointed to via `TALON_BIN`.

---

## Configuration

Run the Hermes setup wizard to configure the plugin:
```
hermes memory setup talon-recall
```

Or create `~/.hermes/talon-recall.json` manually:
```json
{
  "vault_path": "/path/to/your/obsidian/vault",
  "budget_tokens": 2000,
  "min_confidence": 0.3,
  "recency_half_life_days": 7,
  "fast": false,
  "prior_message_count": 2
}
```

| Key | Default | Description |
|---|---|---|
| `vault_path` | (TALON_VAULT env) | Absolute path to your Obsidian vault |
| `budget_tokens` | `2000` | Token budget for the recall context block |
| `min_confidence` | `0.3` | Evidence score floor; below this returns empty context |
| `recency_half_life_days` | `7` | Half-life for recency decay weighting |
| `fast` | `false` | Skip LLM expansion + reranking (faster, lower quality) |
| `prior_message_count` | `2` | Last N user turns fed to talon to widen the query |

---

## How it works

Before each agent turn, Hermes calls `prefetch(query)`:

1. The plugin runs `talon recall <query> --format prompt-xml --budget-tokens N ...`
2. Talon performs hybrid search + link traversal + frontmatter aggregation + recency scoring against your Obsidian vault.
3. The resulting `<vault_recall>` XML block is injected into the agent's context.

When evidence is below `min_confidence`, talon returns `<vault_recall skipped="true" .../>` and the plugin returns `""` — the agent sees a clean context with no memory provider noise.

See [docs/recall.md](../../docs/recall.md) for the full recall pipeline documentation and [docs/recall-schema.md](../../docs/recall-schema.md) for the prompt-xml schema.

---

## Troubleshooting

**Binary not found → install talon and check PATH:**
```
which talon           # should print a path
talon --version       # should print 0.1.0 or later
```
Or set `TALON_BIN=/absolute/path/to/talon`.

**Vault path missing → set vault_path or TALON_VAULT:**
```
export TALON_VAULT=/path/to/obsidian
```

**Agent gets no context → check min_confidence threshold:**
Reduce `min_confidence` in `~/.hermes/talon-recall.json` (default 0.3).
Run `talon recall "<your query>" --format prompt-xml` manually to inspect the evidence_score.

**Agent gets stale context → reduce recency_half_life_days:**
Lower `recency_half_life_days` to weight recently-modified notes more heavily.

---

## Publishing

Package version is locked to talon-cli `major.minor` (`pyproject.toml` comment documents this).

Build and publish on a `hermes-plugin-v*` tag:
```
python -m build
twine upload dist/*
```

Or use the justfile recipe:
```
just publish-hermes-plugin
```
