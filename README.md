# Talon

Talon is a Rust binary that indexes an Obsidian vault and makes it queryable from the command line and from AI agents: hybrid search with graph-aware reranking, grounded answers, structured vault queries, and an MCP server. One binary. No cloud. No daemon.

---

## Why Talon

Most note search tools treat your vault as a bag of text. Talon treats it as a knowledge graph: wikilinks, backlinks, community structure, scope roles, and frontmatter all contribute to ranking. A note your other notes already cite heavily isn't just similar in content -- it's structurally important, and the ranking reflects that.

Talon is also designed for the [Karpathy LLM-Wiki](https://karpathy.github.io/2024/04/27/hacking-the-wiki/) workflow: a vault structured as `wiki/`, `projects/`, `artifacts/`, `daily/`, `raw/`, `private/`. The scope system maps directly to this layout. Recall knows that a `wiki/` note and a `daily/` note are different kinds of signal. The example vault (`examples/calle-sur-vault/`) uses this layout out of the box.

The other design decision that shapes everything: Talon is agent-native. `--agent` output, MCP tools, and the recall hook are first-class, not wrappers around a human CLI. Every query returns vault paths and graph metadata the calling agent can act on, not formatted prose the agent has to parse.

---

## What it does

**Search** with natural language. Talon fuses four retrieval signals with weighted RRF, reranks with a cross-encoder, then adjusts scores using the vault's link graph.

```bash
talon search "lacto-fermentation salt ratios"
talon --fast search "knife skills"   # lexical only, no sidecar needed
```

**Ask** and get a grounded answer synthesized from the matching notes, not a hallucination.

```bash
talon ask "what's my current thinking on co-packer pricing"
```

**Navigate the graph.** `talon related` scores related notes across six graph signals: direct links, backlinks, shared sources, common neighbors, Louvain community membership, and bridge position.

```bash
talon related "wiki/Lacto-Fermentation.md"
```

**Inspect the vault.** `talon inspect` reports structural issues: orphan notes with no incoming links, broken wikilink targets, dangling frontmatter source references, notes with neither links nor backlinks.

```bash
talon inspect
```

**Query frontmatter.** `talon meta` runs structured queries over indexed frontmatter fields with typed comparisons and `--since` filters.

```bash
talon meta --where "status=active" --scope projects
talon meta --since 2026-04-01 --select title,status,tags
```

**Track changes.** `talon changes` returns indexed and tombstoned events with RFC 3339 timestamps for agent pipelines that need to know what changed since a given point.

**Serve agents.** `talon mcp` runs a stateless MCP-over-stdio server so Claude Code, Cursor, or any MCP-compatible host can search and read the vault as a tool.

**Inject recall per turn.** `talon recall` fires on every agent turn via a `UserPromptSubmit` hook: it distills the prompt, retrieves relevant notes, and injects context before the model sees the message.

---

## Retrieval pipeline

A full hybrid query (`talon search`) runs in this order:

**1. Lexical probe.** BM25 (SQLite FTS5) and title/alias matching retrieve an initial candidate set. The title matcher handles exact Obsidian wikilink targets and fuzzy variants.

**2. Query expansion.** If a local chat LLM is configured, the query goes through an expansion step that rewrites it into multiple search-optimized reformulations. The expansion model receives a token-budgeted view of the query, not the raw text, to keep inference cost flat.

**3. Parallel vector retrieval.** Dense embeddings are retrieved for each expanded query. Talon stores embedding metadata in SQLite and delegates inference to a local HTTP sidecar, keeping the binary free of model weights.

**4. Weighted RRF.** Four signal lists (BM25, exact alias, fuzzy title, semantic) are fused per result using Reciprocal Rank Fusion with per-list weights:

```
score(result, list) = WEIGHT[list] / (RRF_K + rank + 1)
```

Scores are summed across lists and normalized against the theoretical maximum for the lists that returned results. A result that dominates one list doesn't automatically beat one with consistent moderate presence across all four.

**5. Cross-encoder reranking.** The fused set goes through a cross-encoder that scores query-document pairs directly rather than relying on embedding similarity. This catches relevance misses that vector retrieval tends to produce for paraphrase-heavy queries.

**6. Graph adjustment.** Final scores are adjusted by graph position. Notes that share a Louvain community with top results, have direct link relationships to high-scoring notes, or sit on bridge paths between dense clusters get a relevance-gated boost. The boost is capped: a structurally central but content-weak note cannot outrank a strong match.

`--fast` skips steps 2-6 entirely and returns BM25+title results only. No sidecar required.

---

## Graph engine

Talon builds and persists a weighted directed graph over Obsidian wikilinks, rebuilt incrementally on sync.

**Community detection** runs deterministic Louvain modularity optimization: iterative node reassignment with modularity gain `Q = Σ [A_ij - k_i·k_j/(2m)] · δ(c_i,c_j) / 2m`, converging when gain drops below `1e-7` across up to 20 passes. Community assignments are stored in SQLite and reused by search ranking and recall without recomputation per query.

**`talon related`** scores candidate notes across six signals:

| Signal | What it measures |
|---|---|
| `direct_out` | Target is linked from the source note |
| `direct_backlink` | Target links back to the source note |
| `shared_sources` | Both notes cite overlapping `sources:` frontmatter entries |
| `common_neighbors` | Overlap in the two notes' link neighborhoods |
| `community_affinity` | Both notes fall in the same Louvain community |
| `type_affinity` | Both notes share the same Obsidian note type |

A `structural_penalty` reduces scores for high-bridge / low-cohesion notes -- index pages and routing nodes that connect the graph but don't belong strongly to any community.

---

## Recall pipeline

`talon recall` is designed to run as a `UserPromptSubmit` hook on every agent turn, injecting vault context before the model sees the message.

**Phrase extraction.** The incoming prompt is parsed into weighted search phrases without calling any model. Quoted strings and Obsidian wikilinks get weight 1.5. Tags, code identifiers, and file paths get lower weights. Proper noun sequences are scored with YAKE (Yet Another Keyword Extractor): a graph-based statistical method that weights terms by position, frequency, and co-occurrence without needing training data.

**Distillation decision.** If the prompt exceeds the embedding token budget or is classified as noisy (multi-turn context with low signal density), Talon calls the expansion LLM to distill it into focused search queries. If there's no time before the deadline, it falls back to the phrase-extracted queries. If no LLM is configured, it uses phrase extraction only. The deadline is configurable per-hook (`recall_deadline_ms`).

**Retrieval and scoring.** Recall runs the same hybrid pipeline as search, scoped to `default = true` scopes only (unless overridden). The output is scored with a composite evidence signal:

```
evidence = 0.50 * rerank + 0.20 * lexical + 0.20 * graph_density + 0.10 * recency
```

where `graph_density = min(link_count / 5, 1.0)` and `recency = exp(-days_since_modified / 14)`. A note with a strong rerank score, high link count, and recent modification ranks higher than one that's merely semantically similar.

**Linked context.** Recall also includes a community-capped linked context: notes that the top results link to or cite, deduplicated across communities so no single cluster dominates the injected context.

The output is an `<vault_recall>` XML block injected into the agent's context window.

---

## Agent output

Every command accepts `--agent`. Agent mode emits compact JSON with plain vault paths, no ANSI formatting, and no envelope metadata.

```bash
talon --agent search "fermentation notes"
talon --agent read "[[Hot Sauce Formulation#Targets]]"
talon --agent related "wiki/Lacto-Fermentation.md"
```

`read` accepts Obsidian references directly. Heading reads return only the requested section with `fromLine` and `toLine`. Search results include resolved `links`, `backlinks`, `tags`, `aliases`, and `citations` as compact graph navigation metadata.

---

## MCP integration

```bash
talon mcp
```

Stateless MCP-over-stdio server. Wire it into `.mcp.json`:

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

Exposed tools: `talon_search`, `talon_read`, `talon_related`. `talon_ask` is intentionally excluded: agents search and synthesize better with their own model.

`integrations/hermes-talon-recall/` is a drop-in Hermes memory provider that automates recall injection for Hermes-hosted agents.

---

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

---

## Sync

```bash
talon sync            # incremental refresh, stale cleanup, pending embeddings
talon sync --fast     # incremental refresh and cleanup, no embedding pass
talon sync --force    # rebuild embeddings for every active chunk
talon sync --rebuild  # drop and rebuild the index from scratch
```

Sync skips unchanged files by mtime and size. Move and rename detection runs in the same pass: the new path is indexed and the old path soft-deleted, then Talon tries to re-resolve any wikilinks pointing at the old title against current active titles and aliases. Link edits inside changed files are reindexed with the file.

---

## Quick start

```bash
cargo install --path crates/talon-cli

cp examples/config.toml ~/.config/talon/config.toml
# edit vault_path to your Obsidian vault

talon sync
talon search "your query"
talon ask "summarize my notes on X"
```

`examples/config.toml` is fully annotated with every knob. `examples/calle-sur-vault/` is a 78-note synthetic vault (fictional chef-restaurateur, full LLM-Wiki layout) that works out of the box without touching your real vault.

---

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

---

## License

MIT
