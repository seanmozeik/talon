# hermes-talon-recall

Hermes Agent Memory Provider plugin that wraps [`talon recall --format prompt-xml`](../../docs/recall.md) to surface vault-native context from an Obsidian knowledge base on every agent turn.

Talon is **recall-only** and stateless per call. The plugin implements `prefetch()` to retrieve relevant context; the agent host (your Obsidian editor or a cron-driven agent) owns vault mutations.

---

## Installation

Hermes loads memory providers from `plugins/memory/<name>/` in the Hermes install
or `$HERMES_HOME/plugins/<name>/` for user-installed providers. The
`hermes-memory/talon-recall` directory is the small discovery shim Hermes loads;
the Python package contains the actual provider implementation.

**Drop-in for a Hermes profile:**
```
pip install -e integrations/hermes-talon-recall
cp -r integrations/hermes-talon-recall/hermes-memory/talon-recall ~/.hermes/plugins/talon-recall
```

**Image or system install:**
```
pip install -e integrations/hermes-talon-recall
cp -r integrations/hermes-talon-recall/hermes-memory/talon-recall /opt/hermes/plugins/memory/talon-recall
```

**PyPI (once published):**
```
pip install hermes-talon-recall
```
After installing from PyPI, still install the `hermes-memory/talon-recall` shim
into the Hermes memory-provider directory used by your deployment.

The talon binary must be on `PATH` or pointed to via `TALON_BIN`. Talon reads
`~/.config/talon/config.toml` under the subprocess `HOME`. In Hermes profiles,
the plugin follows Hermes' convention and runs Talon with `HOME=$HERMES_HOME/home`.

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
  "budget_tokens": 500,
  "min_confidence": 0.4,
  "fast": false,
  "prior_message_count": 2
}
```

| Key | Default | Description |
|---|---|---|
| `vault_path` | (TALON_VAULT env) | Absolute path to your Obsidian vault |
| `budget_tokens` | `500` | Token budget for the recall context block |
| `min_confidence` | `0.4` | Evidence score floor; below this returns empty context |
| `fast` | `false` | Skip LLM expansion + reranking (faster, lower quality) |
| `prior_message_count` | `2` | Last N user turns fed to talon to widen the query |

This file configures only the Hermes memory wrapper. Talon's vault, database,
scope, and inference settings live in `~/.config/talon/config.toml` under the
subprocess `HOME`.

---

## How it works

Before each agent turn, Hermes calls `prefetch(query)`:

1. The plugin runs `talon recall <query> --format prompt-xml --budget-tokens N ...`
2. Talon performs hybrid search + link traversal against your Obsidian vault, returning the top notes with one-line excerpts and modification dates.
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

**Wrong Talon config → check the subprocess HOME:**
```
echo "$HERMES_HOME/home/.config/talon/config.toml"
```

**Agent gets no context → check min_confidence threshold:**
Reduce `min_confidence` in `~/.hermes/talon-recall.json` (default 0.4).
Run `talon recall "<your query>" --format prompt-xml` manually to inspect the evidence_score.

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
