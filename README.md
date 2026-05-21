# Talon

Talon is a standalone Rust binary for Obsidian vault search, grounded answers, read, sync, related-note traversal, status, recall, and MCP-over-stdio integration.

## Layout

- `crates/talon-core`: typed contracts, config, search/indexing/query logic, shared LLM clients, and output types.
- `crates/talon-cli`: `clap` CLI, `--agent`, `--skill`, human output, `--mcp` entry point, config loading, and process wiring.
- `skill/SKILL.md`: agent-facing contract printed by `talon --skill`.
- `ts/`: thin npm wrapper skeleton for binary resolution and MCP child specs.

## CLI UX Direction

Human output is a product surface. Talon uses `anstyle`/`anstream` for terminal styling, `rattles` for indeterminate spinners, and compact cards for search/read/ask results. Agent output is separate: `--agent` emits compact JSON with plain vault paths and no envelope metadata.

## Development

```bash
just check
cargo run -p talon-cli -- --help
cargo run -p talon-cli -- init
cargo run -p talon-cli -- --skill
```

## Index Lifecycle

`talon sync` keeps the index in step with the configured vault. It scans Markdown files, reindexes new or changed files, skips unchanged files by mtime and size, then cleans up active index rows whose source paths were deleted, moved, renamed, or excluded by the current include/ignore filters.

For a move or rename, the next sync indexes the new path and soft-deletes the old path in the same run. The old note row remains inactive for history/change queries, but its chunks, link rows, aliases, tags, frontmatter fields, and vector metadata are removed. Link edits inside changed files are reindexed with the file. If a link target moves without editing the source file, sync tries to relink unresolved wikilinks against current active note titles and aliases.

Sync flags change cost or depth:

- `talon sync`: incremental index refresh, stale path cleanup, and pending/changed embeddings.
- `talon sync --fast`: same index refresh and stale path cleanup, with no embedding pass.
- `talon sync --force`: incremental index refresh, then rebuild embeddings for every active chunk.
- `talon sync --rebuild`: delete the SQLite index files, recreate the schema, and index the vault from scratch. Combine with `--fast` for a lexical-only rebuild.

## Standalone Embedder

Set `embedding.base_url` and `rerank.base_url` in `~/.config/talon/config.toml` to a local HTTP TEI-compatible endpoint. Good defaults are Hugging Face `text-embeddings-inference`, Infinity, or ultraclaw's local sidecar if you are running one. Talon only expects `/embed`, `/embed-chunked`, and `/rerank` endpoints with the shapes described in the design doc.

## Ask

`talon ask` is a human-facing way to get a compact answer grounded in the vault:

```bash
talon ask "what do my notes say about cooking lamb"
talon --fast ask "summarize my notes on knife skills"
```

Ask uses the configured ask model to plan search queries, runs Talon's normal search stack, expands the matched notes into the most relevant source chunks, and asks the model to synthesize an answer. Unlike `search`, ask may feed multiple snippets from the same document to the synthesis model when several chunks are relevant.

Configure the ask model under `[chat.ask]`; transport defaults inherit from `[chat.expansion]`:

```toml
[chat.expansion]
base_url = "http://localhost:8000/v1"
model = "bonsai"

[chat.ask]
model = "qwen-smol"
planning_reasoning_effort = "none"
synthesis_reasoning_effort = "medium"
```

Non-fast ask does not cap output tokens. `--fast` forces both ask stages to `reasoning_effort = "none"`, sends `chat_template_kwargs.enable_thinking = false`, caps completion output at 2048 tokens, and runs the retrieval stage in fast lexical mode.

## Scopes

Scopes partition the vault by role and let queries opt in or out of each partition. Declare them in `~/.config/talon/config.toml`:

```toml
[scopes.wiki]
glob = "wiki/**"
priority = "boosted"
default = true
lint = true

[scopes.daily]
glob = "daily/**"
priority = "muted"
default = true
lint = false        # daily notes still indexed; just not reported by `talon lint`

[scopes.private]
glob = "private/**"
priority = "buried"
default = false     # excluded from queries unless --scope private / --scope-all
lint = false
```

Scope iteration follows TOML declaration order — narrower or more sensitive scopes declared above broader ones win when their globs overlap.

By default, queries (`search`, `recall`, `related`, `meta`, `changes`, `lint`) cover only scopes with `default = true`. Scopes with `default = false` are **excluded** entirely — not just down-ranked. Scope priority weights are modest by default (`boosted=1.2`, `elevated=1.1`, `normal=1.0`, `muted=0.85`, `buried=0.5`) and positive boosts are gated by relevance so a weak high-priority hit cannot shout over a stronger match.

To include a `default = false` scope, opt in explicitly:

- `--scope NAME` (repeatable, additive, short form `-s`): adds the named scope to the default pool.
- `--scope-only NAME` (repeatable): searches only the named scope(s).
- `--scope-all`: searches every configured scope, overriding the `default` flag.

The three are mutually exclusive on a single invocation. Unknown scope names error with the list of configured names. The response's `meta.scope_set` echoes the resolved active scope names. See `examples/config.toml` for a Karpathy-style preset.

### Lint exclusion

Per-scope `lint = false` skips a scope's files in `talon lint` findings — useful for ephemeral journals (`daily/`), closed work (`archive/`), or sensitive material (`private/`). Excluded files are still indexed and continue to satisfy link-target resolution, so a wiki note linking to a `daily/` file isn't reported as broken.

For globs that don't fit cleanly into a scope, set a global ignore list:

```toml
[lint]
ignore = ["**/_drafts/**", "**/scratch.md"]
```

Global `lint.ignore` takes precedence over per-scope `lint = true`.

## Per-result fields

`search`, `related`, and `meta` results carry these fields in `--json` mode. Search's `isIndex`, `citations`, `links`, `backlinks`, `tags`, and `aliases` are also included in `--agent` mode when non-empty because they are compact navigation cues.

- `scope`: resolved scope name. Omitted for paths that match no scope.
- `mtime`: file modification time in the system local timezone. `"HH:MM"` (e.g. `"15:42"`) for edits within the last 24 hours, `"YYYY-MM-DD"` (e.g. `"2026-04-25"`) otherwise. Recent edits get instantly-readable wall-clock time; older edits collapse to date. For sub-day precision on indexing/deletion events, see `changes`.
- `isIndex` (`search` only): true for generic index pages such as `index.md`, `README.md`, and `*_index.md`.
- `citations` (`search` only): source paths listed in the result note's `sources:` frontmatter, capped for compact output.
- `links` (`search` / `read`): resolved outgoing Obsidian wikilinks, capped for compact search output.
- `backlinks` (`search` only): notes that link to the result, capped for compact output.
- `tags` / `aliases` (`search` / `read`): Obsidian frontmatter and inline metadata indexed for the note.
- `count` (`related` only): number of distinct link rows between source and target — a rough edge-strength signal.

`changes.indexed_at` and `tombstones.deleted_at` use full RFC 3339 UTC (`"2026-04-25T10:23:00Z"`) since `--since` consumers compare exact timestamps.

`read` accepts Obsidian references such as `[[Hot Sauce Formulation]]`, `[[Hot Sauce Formulation#Targets]]`, and `Hot Sauce Formulation#Targets`. Heading reads return only that section plus line metadata.

`search` query text supports tag and heading filters: `#fermentation`, `tag:fermentation`, `heading:Targets`, and `h:Targets`.

## Chunking

Markdown chunking is configured under `[indexer]` in `talon.toml`:

```toml
[indexer]
chunk_tokens = 512
chunk_overlap = 64
chunk_min_tokens = 16
```

The text-splitter/tokenx chunker strips frontmatter from BM25 and embedding text. Existing indexes will be re-chunked on the next `talon sync`, which also queues the new chunks for embedding. On a large vault, the first post-upgrade sync can spend 30+ minutes re-embedding against a real sidecar.

## Integrations

- `talon mcp` exposes public MCP tools for agents: `talon_search`, `talon_read`, and `talon_related`. It intentionally does **not** expose `talon_ask`; agents are better served by searching/reading the vault and synthesizing with their own model.
- [`integrations/hermes-talon-recall/`](integrations/hermes-talon-recall/README.md) — Hermes Agent Memory Provider plugin that wraps `talon recall --format prompt-xml` to inject vault-native context on every agent turn. Recall-only; agent host handles vault writes.
