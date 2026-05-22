# Talon

Talon is a Rust binary that indexes an Obsidian vault and makes it queryable: hybrid search, grounded answers, graph traversal, and an MCP server that drops vault context into any AI agent. One binary, no cloud, no daemon.

---

## What it does

**Search** your vault with natural language. Talon fuses five retrieval signals per query: BM25 lexical matching, dense vector search, title and alias matching, query expansion via a local LLM, and graph-aware reranking that boosts notes your other notes already trust. Results are reranked by a cross-encoder before they reach you.

```
talon search "what did I write about fermentation and salt ratios"
```

**Ask** a question and get a grounded answer synthesized from the matching notes, not a hallucination.

```
talon ask "what's my current thinking on pricing the hot sauce line"
```

**Navigate the graph.** `talon related` traverses link and backlink edges with graph-community scoring so the most structurally central notes rank above loosely-connected ones.

**Serve agents.** `talon mcp` is an MCP-over-stdio server exposing `talon_search`, `talon_read`, and `talon_related`. Point Claude Code, Cursor, or any MCP-compatible agent at it and the vault becomes searchable context.

**Inject recall automatically.** The `UserPromptSubmit` hook fires `talon recall` on every turn, scores the prompt against the vault, and injects the highest-signal notes into the agent's context window before the model sees the message.

---

## Why it's built in Rust

The indexer, BM25 store, vector metadata store, graph engine, and query pipeline all run in a single process with no coordinator. Cold queries on a 3,000-note vault take under 100ms lexical-only; hybrid queries depend on your embedding sidecar latency, not Talon's overhead. Memory use is flat at rest.

---

## Quick start

```bash
cargo install --path crates/talon-cli

# Point at your vault
cp examples/config.toml ~/.config/talon/config.toml
# edit vault_path to your Obsidian vault

# Build the index
talon sync

# Search
talon search "your query here"

# Ask
talon ask "summarize my notes on X"
```

For the full config reference, see `examples/config.toml`. It's annotated.

---

## Hybrid retrieval

A normal search query runs this pipeline:

1. BM25 and title/alias probes retrieve a candidate set
2. If a local LLM is configured, a query expansion step rewrites the query into search-optimized forms
3. Dense vector retrieval runs in parallel against the BM25 candidates
4. Reciprocal-rank fusion merges the two result lists
5. A cross-encoder reranks the fused set
6. Graph scoring adjusts final ranks: notes at the center of your link graph, in tightly-knit communities with your top results, get a modest structural boost

`--fast` skips steps 2-6 and returns BM25+title results only. Useful when you need speed and the query is exact enough that lexical matching is sufficient.

---

## Agent output

Every command accepts `--agent`. Agent mode emits compact JSON with plain vault paths and no formatting, designed to be parsed by a calling agent rather than read by a human.

```bash
talon --agent search "fermentation notes"
talon --agent read "[[Hot Sauce Formulation#Targets]]"
talon --agent related "wiki/Lacto-Fermentation.md"
```

Read accepts Obsidian references directly. Heading reads return only the requested section, with line numbers, so the agent knows where in the file it landed.

---

## MCP integration

```bash
talon mcp
```

Runs a stateless MCP-over-stdio server. Wire it into Claude Code's `.mcp.json` or any MCP-compatible host:

```json
{
  "mcpServers": {
    "talon": {
      "command": "talon",
      "args": ["mcp"]
    }
  }
}
```

Exposed tools: `talon_search`, `talon_read`, `talon_related`. `talon_ask` is intentionally not exposed: agents are better served searching and synthesizing with their own model.

---

## Recall hook (Claude Code)

Talon ships a recall command designed to run on every agent turn:

```bash
talon recall --format prompt-xml
```

It distills the incoming prompt into weighted search phrases, runs hybrid retrieval, and emits a `<vault_recall>` XML block the agent host injects into context. The `integrations/hermes-talon-recall/` directory contains a drop-in Hermes memory provider that automates this for Hermes-hosted agents.

For Claude Code, add to your `settings.json`:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "matcher": "",
        "hooks": [
          {
            "type": "command",
            "command": "talon recall --format prompt-xml"
          }
        ]
      }
    ]
  }
}
```

---

## Scopes

Scopes partition the vault by role. Declare them in `config.toml`, set `default = false` on sensitive or noisy directories, and they're excluded from queries unless you opt in.

```toml
[scopes.wiki]
glob = ["wiki/**"]
priority = "boosted"
default = true

[scopes.private]
glob = ["private/**"]
priority = "buried"
default = false   # excluded unless you pass --scope private
```

Priority weights are `boosted=1.2`, `elevated=1.1`, `normal=1.0`, `muted=0.85`, `buried=0.5`. Boosts are relevance-gated: a weak high-priority hit won't outrank a stronger normal-priority match.

---

## Sync

```bash
talon sync            # incremental refresh, stale cleanup, pending embeddings
talon sync --fast     # incremental refresh and cleanup, no embedding pass
talon sync --force    # rebuild embeddings for every active chunk
talon sync --rebuild  # drop and rebuild the index from scratch
```

The index lives in a SQLite file alongside the vault. Sync is safe to run at any frequency; it skips unchanged files by mtime and size.

---

## Embedding sidecar

Talon expects a local HTTP sidecar with `/embed`, `/embed-chunked`, and `/rerank` endpoints. Any TEI-compatible server works: Hugging Face `text-embeddings-inference`, Infinity, or a local ultraclaw sidecar.

```toml
[embedding]
base_url = "http://localhost:8000"
model = "embed"

[rerank]
base_url = "http://localhost:8000"
model = "rerank"
```

Without a sidecar, Talon runs in lexical-only mode. Search and recall still work; they just skip vector retrieval and reranking.

---

## Example vault

`examples/calle-sur-vault/` is a 78-note synthetic vault built around a fictional chef-restaurateur. It uses the Karpathy LLM-Wiki layout (`wiki/`, `projects/`, `artifacts/`, `daily/`, `raw/`, `private/`, `archive/`, `_meta/`) and works out of the box with `examples/config.toml` for hands-on testing without touching your real vault.

---

## License

MIT
