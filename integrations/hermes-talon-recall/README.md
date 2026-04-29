# hermes-talon-recall

Hermes Agent Memory Provider plugin that surfaces vault-native context from an Obsidian knowledge base on every agent turn via a persistent `talon mcp` process.

The plugin implements `prefetch()` to retrieve relevant context and `sync_turn()` to store the agent's response for next-turn enrichment; the agent host (your Obsidian editor or a cron-driven agent) owns vault mutations.

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
  "fast": false
}
```

| Key | Default | Description |
|---|---|---|
| `vault_path` | (TALON_VAULT env) | Absolute path to your Obsidian vault |
| `budget_tokens` | `500` | Token budget for the recall context block |
| `min_confidence` | `0.4` | Evidence score floor; below this returns empty context |
| `fast` | `false` | Skip LLM expansion + reranking (faster, lower quality) |

This file configures only the Hermes memory wrapper. Talon's vault, database,
scope, and inference settings live in `~/.config/talon/config.toml` under the
subprocess `HOME`.

---

## How it works

On `initialize()`, the plugin starts `talon mcp` as a persistent stdio child process and performs the MCP handshake. A single long-lived process serves the entire session — no per-turn spawning overhead.

Before each agent turn, Hermes calls `prefetch(query)`:

1. The plugin calls `talon_hook_recall` via JSON-RPC 2.0 over the MCP process's stdin/stdout.
2. Talon performs hybrid search + link traversal against your Obsidian vault, returning the top notes with one-line excerpts and modification dates.
3. The resulting `<vault_recall>` XML block is injected into the agent's context.
4. Duplicate context is suppressed automatically: the same chunks won't be re-injected in adjacent turns thanks to turn-level score-decay tracked inside the MCP process.

After each turn, Hermes calls `sync_turn()`, which calls `talon_hook_turn_end` with the assistant's response. This stores the response for next-turn context enrichment without any manual `prior_message_count` configuration.

Vault file changes trigger an automatic index refresh via the watcher built into `talon mcp` (60-second debounce). No restart is needed when notes are added or edited.

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

**MCP process failed to start → check talon binary and vault path, then check `talon mcp` manually:**
```
talon mcp             # should start the MCP server without error
```
Verify `TALON_VAULT` is set and points to a valid vault directory, then retry.

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
