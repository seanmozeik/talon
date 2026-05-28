# Talon

[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE) [![runtime: rust](https://img.shields.io/badge/runtime-rust-orange.svg)](https://www.rust-lang.org)

The memory substrate for the Karpathy [LLM Wiki](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f). One Rust binary that indexes an Obsidian vault and serves it back as hybrid search, grounded answers, graph navigation, and a stateless MCP server. No cloud, no daemon, no Python.

```bash
talon search "lacto-fermentation salt ratios"
talon ask "what's my current thinking on co-packer pricing"
talon mcp     # serve the vault as agent tools over stdio
```

## Why this exists

Karpathy published an [LLM Wiki gist](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f) in April 2026: raw articles, papers, repos, and datasets go into a `raw/` folder, an LLM incrementally compiles them into a wiki, the wiki lives as markdown, Obsidian is the IDE frontend, and every useful agent output gets filed back. Plain files on disk, queryable by the agent on every turn.

The retrieval primitive was already in the wild. Tobi Lütke had shipped [`qmd`](https://github.com/tobi/qmd) (BM25, dense embeddings, RRF over any markdown directory), and it had taken off alongside the OSS agent harnesses [OpenClaw](https://github.com/openclaw/openclaw) and [Hermes](https://github.com/NousResearch/hermes-agent) as the de-facto way to give agents access to their own files.

What that pattern leaves out is the graph. An Obsidian vault is a typed knowledge graph already: wikilinks, backlinks, scopes, frontmatter relations, communities. A bag-of-text retriever drops that signal on the floor. The current workaround is to stitch the graph on by hand: Hermes threaded through Obsidian via custom MCPs, OpenClaw mounted next to a markdown directory, QMD piped into the loop. Each piece works in isolation; the integration is the cost.

Talon is the spiritual successor to QMD, purpose-built for Obsidian. Same retrieval core (BM25, dense embeddings, RRF, cross-encoder reranking), with the link graph promoted to a first-class ranking signal alongside them. A note that the rest of your vault already cites is structurally central, and the ranking reflects that even when the semantic match is weaker.

One Rust binary, no daemon, no Python, no graph database to babysit.

## What agents actually get

- **Recall hook.** `talon recall` fires on every agent turn via a `UserPromptSubmit` hook. It distills the prompt with a local LLM, retrieves relevant notes, and injects a `<vault_recall>` block before the model sees the message. Cold-start context, every turn, in under a second.
- **MCP server.** `talon mcp` is a stateless MCP-over-stdio server. Claude Code, Cursor, Codex, anything that speaks MCP, gets `talon_search`, `talon_read`, and `talon_related` as tools. `talon_ask` is deliberately excluded so the host model owns synthesis.
- **`--agent` everywhere.** Every command takes `--agent` and returns compact JSON with plain vault paths, no envelope, no ANSI. The output is graph metadata the calling agent can act on.
- **Scope system that matches the layout.** `wiki/`, `projects/`, `artifacts/`, `daily/`, `raw/`, `private/`. Per-scope retrieval weights, default on/off, and lint rules. Recall knows a `wiki/` note carries different signal than a `daily/` capture.

## A search engine for humans, too

Drop `--agent` and the same commands render in colour, with clickable URLs, highlighted excerpts, and inline citations. The terminal pretty-printer is the default. Agent JSON is the opt-in.

```bash
$ talon ask "what's my current thinking on co-packer pricing"

Co-packers are quoting £1.80–£2.40 per 250ml unit at 2k MOQ. The blocker is the secondary fermentation hold ...

Sources
  → projects/Calle Sur/Co-Packer Outreach.md  (12 backlinks)
  → daily/2026-05-14.md#co-packer-call  (4 backlinks)
```

## What you can do

Hybrid search with graph-aware reranking:

```bash
talon search "lacto-fermentation salt ratios"
talon --fast search "knife skills"   # BM25 + title only, no sidecar needed
```

Six-signal graph navigation:

```bash
talon related "wiki/Lacto-Fermentation.md"
# direct links, backlinks, shared sources, common neighbours, Louvain community, bridge position
```

Vault health audits:

```bash
talon inspect              # orphans, broken links, dangling sources, unreferenced notes
talon inspect --scope wiki
```

Structured frontmatter queries:

```bash
talon meta --where "status=active" --scope projects
talon meta --since 2026-04-01 --select title,status,tags
```

Changelog for agent pipelines:

```bash
talon changes --since 2026-05-01
```

## Retrieval pipeline

A full hybrid query (`talon search`) runs in six stages.

**1. Lexical probe.** BM25 (SQLite FTS5) and title/alias matching against an initial candidate set. The title matcher handles exact Obsidian wikilink targets and fuzzy variants.

**2. Query expansion.** If a local chat LLM is configured, the query is rewritten into multiple search-optimised reformulations. The expansion model receives a token-budgeted view of the query, not the raw text, to keep inference cost flat.

**3. Parallel vector retrieval.** Dense embeddings are retrieved for each expanded query. Talon stores embedding metadata in SQLite and delegates inference to a local HTTP sidecar, keeping the binary free of model weights.

**4. Weighted RRF fusion.** Four signal lists (BM25, exact alias, fuzzy title, semantic) are fused with per-list weights via Reciprocal Rank Fusion:

```
score(result, list) = WEIGHT[list] / (RRF_K + rank + 1)
```

Scores are summed across lists and normalised against the theoretical maximum for the lists that returned results. A result that dominates one list cannot automatically beat one with consistent moderate presence across all four.

**5. Cross-encoder reranking.** The fused set goes through a cross-encoder that scores query-document pairs directly rather than relying on embedding similarity. This catches relevance misses that vector retrieval tends to produce for paraphrase-heavy queries.

**6. Graph adjustment.** Final scores are adjusted by graph position. Notes that share a Louvain community with top results, link directly to high-scoring notes, or sit on bridge paths between dense clusters get a relevance-gated boost. The boost is capped: a structurally central but content-weak note cannot outrank a strong match.

`--fast` skips stages 2 through 6 and returns BM25 + title only. No sidecar required.

## Graph engine

Talon builds and persists a weighted directed graph over Obsidian wikilinks, rebuilt incrementally on sync.

**Community detection** runs deterministic Louvain modularity optimisation: iterative node reassignment with modularity gain `Q = Σ [A_ij - k_i·k_j/(2m)] · δ(c_i,c_j) / 2m`, converging when gain drops below `1e-7` across up to 20 passes. Community assignments live in SQLite and are reused by search ranking and recall without per-query recomputation.

**`talon related`** scores candidate notes across six signals:

| Signal | What it measures |
|---|---|
| `direct_out` | Target is linked from the source note |
| `direct_backlink` | Target links back to the source note |
| `shared_sources` | Both notes cite overlapping `sources:` frontmatter entries |
| `common_neighbors` | Overlap in the two notes' link neighbourhoods |
| `community_affinity` | Both notes fall in the same Louvain community |
| `type_affinity` | Both notes share the same Obsidian note type |

A `structural_penalty` reduces scores for high-bridge, low-cohesion notes (index pages, routing nodes) so they don't dominate `related` results.

## Recall pipeline

`talon recall` runs as a `UserPromptSubmit` hook on every agent turn, injecting vault context before the model sees the message.

**Phrase extraction.** The incoming prompt is parsed into weighted search phrases without any model call. Quoted strings and Obsidian wikilinks score 1.5. Tags, code identifiers, and file paths score lower. Proper-noun sequences are scored with YAKE (Yet Another Keyword Extractor), a graph-based statistical method that weights terms by position, frequency, and co-occurrence with no training data.

**Distillation decision.** If the prompt exceeds the embedding token budget or is classified as noisy (multi-turn context with low signal density), Talon calls the expansion LLM to distill it into focused search queries. If there's no time before the deadline, it falls back to the phrase-extracted queries. If no LLM is configured, it uses phrase extraction only. The deadline is configurable per-hook (`recall_deadline_ms`).

**Retrieval and scoring.** Recall runs the same hybrid pipeline as search, scoped to `default = true` scopes only (unless overridden). The output is scored with a composite evidence signal:

```
evidence = 0.50 * rerank + 0.20 * lexical + 0.20 * graph_density + 0.10 * recency
```

where `graph_density = min(link_count / 5, 1.0)` and `recency = exp(-days_since_modified / 14)`. A note with strong rerank, high link count, and recent modification outranks one that's merely semantically similar.

**Linked context.** Recall also includes a community-capped linked context: notes that top results link to or cite, deduplicated across communities so no single cluster dominates the injected context.

The output is a `<vault_recall>` XML block injected into the agent's context window.

## Agent output

Every command accepts `--agent`. Agent mode emits compact JSON with plain vault paths, no ANSI formatting, and no envelope metadata.

```bash
talon --agent search "fermentation notes"
talon --agent read "[[Hot Sauce Formulation#Targets]]"
talon --agent related "wiki/Lacto-Fermentation.md"
```

`read` accepts Obsidian references directly. Heading reads return only the requested section with `fromLine` and `toLine`. Search results include resolved `links`, `backlinks`, `tags`, `aliases`, and `citations` as compact graph navigation metadata.

## MCP integration

```bash
talon mcp
```

Stateless MCP-over-stdio. Wire it into `.mcp.json`:

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

Exposed tools: `talon_search`, `talon_read`, `talon_related`. `talon_ask` is intentionally excluded so the host model owns synthesis.

`integrations/hermes-talon-recall/` is a drop-in Hermes memory provider that automates recall injection for Hermes-hosted agents.

## Vault health

`talon inspect` audits the link graph and reports four categories of structural issue:

- **Orphans.** Notes with no incoming links from any other note.
- **Broken links.** Wikilinks pointing at a title or alias that doesn't exist in the index.
- **Dangling refs.** Paths listed in a note's `sources:` frontmatter that don't resolve to an active note.
- **Unreferenced.** Notes with neither outgoing nor incoming links.

```bash
talon inspect
talon inspect --scope wiki    # limit to a specific scope
```

Findings respect scope `inspect = false` flags, so `daily/` and `private/` don't generate noise. The check runs against the live index, so it reflects the current sync state without re-scanning the filesystem.

For agents running curation passes, `talon inspect --agent` emits a compact JSON findings list with vault paths and finding types.

## Scopes

Scopes partition the vault by role and control what gets searched, ranked, and linted.

```toml
[scopes.wiki]
glob = ["wiki/**"]
priority = "boosted"   # 1.2x weight, relevance-gated
default = true
inspect = true

[scopes.daily]
glob = ["daily/**"]
priority = "muted"     # 0.85x weight
default = false        # excluded from default queries
inspect = false        # not reported by talon inspect

[scopes.private]
glob = ["private/**"]
priority = "buried"    # 0.5x weight
default = false
inspect = false
```

Priority weights are applied after relevance scoring, not before. A weak high-priority hit cannot outrank a strong normal-priority match. The `inspect = false` flag excludes a scope from `talon inspect` findings without removing it from the index.

Scope iteration follows TOML declaration order, so narrower globs declared above broader ones win when they overlap.

## Sync

```bash
talon sync            # incremental refresh, stale cleanup, pending embeddings
talon sync --fast     # incremental refresh and cleanup, no embedding pass
talon sync --force    # rebuild embeddings for every active chunk
talon sync --rebuild  # drop and rebuild the index from scratch
```

Sync skips unchanged files by mtime and size. Move and rename detection runs in the same pass: the new path is indexed, the old path soft-deleted, then Talon tries to re-resolve any wikilinks pointing at the old title against current active titles and aliases. Link edits inside changed files are reindexed with the file.

## Install

```bash
# Homebrew (macOS / Linux)
brew install seanmozeik/tap/talon

# Cargo
cargo install talon-cli

# npm (prebuilt binary, works on macOS / Linux / Windows)
npm install -g @seanmozeik/talon

# Or from source
cargo install --path crates/talon-cli
```

## Quick start

```bash
cp examples/config.toml ~/.config/talon/config.toml
# edit vault_path to your Obsidian vault

talon sync
talon search "your query"
talon ask "summarise my notes on X"
```

`examples/config.toml` is fully annotated with every knob. `examples/calle-sur-vault/` is a 78-note synthetic vault (fictional chef-restaurateur, full LLM-Wiki layout) that works out of the box without touching your real vault.

## Credentials

Talon stores API keys in the OS keychain (macOS Keychain, Linux kernel keyring, Windows Credential Manager) as a single encrypted JSON blob. No keys in config files.

```bash
talon secrets set openrouter sk-your-key
talon secrets status
talon secrets delete openrouter
```

Reference a stored credential from `config.toml`:

```toml
[credentials.openrouter]
# no api_key or api_key_env needed, resolved from keychain by name

[chat.expansion]
credential = "openrouter"
base_url = "https://openrouter.ai/api/v1"
model = "mistralai/mistral-7b-instruct"
```

Resolution order per endpoint: inline `api_key`, `api_key_env`, named credential inline key, named credential env var, keychain blob. The keychain is the last leg, so existing env-var workflows keep working unchanged.

## Embedding sidecar

Talon calls a local HTTP sidecar for embeddings and reranking. Any TEI-compatible server works: Hugging Face `text-embeddings-inference`, Infinity, or a local LLM sidecar with the right endpoint shapes (`/embed`, `/embed-chunked`, `/rerank`).

```toml
[embedding]
base_url = "http://localhost:8000"
model = "embed"

[rerank]
base_url = "http://localhost:8000"
model = "rerank"
```

Without a sidecar, Talon runs in lexical-only mode. Search, recall, and all graph features still work.

## License

MIT.
