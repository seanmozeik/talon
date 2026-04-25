# Talon: Extraction & LLM Wiki Primitives

**Date:** 2026-04-25
**Status:** design — architectural questions resolved
**Scope:** Move Talon out of `edge/src/services/talon/` into a standalone Rust project (`talon`), and at the same time extend its query surface so it can serve as the primitive layer behind a Karpathy-style "LLM Wiki" workflow on an Obsidian vault. Refactor ultraclaw to consume Talon as an external dependency, retaining ultraclaw's TypeScript watcher and embedding scheduler as the *callers* of `talon sync`.

## How to use this spec in a new session

This spec is self-contained — a fresh session can implement without re-deriving anything from the codebase. Skim §0 (TL;DR), §2 (mental model), §18 (MCP federation), §22 (resolved decisions) in that order. The decisions already locked in:

- Talon is a **stateless, manual** Rust binary. No file watcher, no embedding scheduler, no clock-driven background work. Every CLI/MCP call opens the DB, answers, and returns. Callers (ultraclaw, bash, cron) own all cadence (§4, Decision 1).
- Talon ships as a standalone Rust binary in a separate repo, with a thin npm wrapper (`@seanmozeik/talon`) that exposes `mcpChildSpec` and `resolveBinary` (§9). Agent skill text is exposed by the binary via `talon --skill`, matching the `ddg` pattern.
- `--mcp` is the only MCP mode: a stateless MCP-over-stdio server backed by the configured DB. There is no `serve` subcommand and no daemon mode (§4, §5).
- Talon owns its host config at `~/.config/talon/config.toml`. Ultraclaw does not inject, adapt, or validate that config.
- npm distribution uses the scoped package `@seanmozeik/talon`; there is no crates.io or Homebrew distribution requirement.
- sqlite + sqlite-vec statically linked into the Rust binary (the macOS Homebrew SQLite quirk goes away).
- Ultraclaw consumes Talon by way of a generic stdio-MCP federation layer at `edge/src/mcp/children/` (~400 LOC); talon is one entry in `config.mcp.children` (§15.2, §18). No talon-specific MCP plugin file or supervisor.
- `DeepResearchSupervisor` plumbing into Talon was dead wiring; dropped.
- **Ultraclaw keeps its existing chokidar watcher and embedding scheduler.** Both survive the cutover; their inner work changes from "call in-process Effect indexer" to "spawn `talon sync <paths>`" (§15.1, §16.2).
- Talon adds a primitive query surface for Karpathy-style LLM Wiki workflows: configurable per-scope weights, frontmatter querying (`meta`), change feeds (`changes`), and link-graph defect queries (`lint`). Semantic dedup, coverage analysis, and editorial decisions are explicitly the *agent's* job, not Talon's (§6, §10, §11, Decision 5).

## 0. TL;DR

Talon becomes its own product. New repo ships:

- A Rust binary `talon` with CLI subcommands (`search`, `read`, `sync`, `status`, `related`, `meta`, `changes`, `lint`) and an `--mcp` flag that puts the same process into MCP-over-stdio mode. The MCP mode is **purely stateless**: each tool call opens the DB, answers, and returns. No watcher, no scheduler, no clock work runs inside the Rust binary.
- A thin TypeScript package `@seanmozeik/talon` (npm) that resolves the per-platform prebuilt binary and exposes a small API for ultraclaw-style hosts to use it as an MCP child (spawn command + args; Talon reads its own host config).

Talon's query surface is extended to make it an effective primitive layer for an agent-curated wiki:

- **Scopes.** The vault is partitioned into named scopes by glob, each with a `priority` tier and a `default` flag. The default search set is the union of `default = true` scopes; `--scope <name>` opts a non-default scope in; `--scope-only` narrows.
- **Ranking.** Scope priority is applied as a calibrated **post-rerank score multiplier** (`boosted=3.0, elevated=1.5, normal=1.0, muted=0.3, buried=0.05`). Multipliers are owned by Talon and are not user-tunable.
- **Frontmatter querying.** A new `meta` action and a `--where` filter (`key OP value`, with `=, !=, <, <=, >, >=, contains, exists`) on `search`/`list`/`meta`. Reverse-source index (`meta --sources <path>`). Tag-counts.
- **Change feed.** A `--since` filter (on `search`/`list`/`meta`) plus a dedicated `changes` action returning `{added, modified, deleted}` with tombstones for files that were removed.
- **Lint primitives.** A `lint` action with `check ∈ {orphans, broken-links, dangling-refs, unreferenced}`. Cheap, deterministic, graph-only. Coverage, dedup, and clustering are the agent's job.
- **Output envelope.** Every JSON response is `{action, version, ok, data, meta}`; failures are `{ok: false, error}`. Already the current shape; extend, don't reinvent.

Ultraclaw deletes ~10.6K LOC of Talon implementation under `services/talon/{embed,indexer,query,search,shared}/...` plus the ~900 LOC of `mcp/tools/talon-*.ts`. **What survives:** the watcher (chokidar) and embedding scheduler — both repurposed as *callers* of the host `talon` binary instead of in-process Effect services. In addition:

- `edge/` adds `@seanmozeik/talon` as an npm dependency.
- A new generic `McpChildren` federation layer in ultraclaw spawns and supervises arbitrary stdio MCP servers, fetches their `tools/list`, and registers each as a proxy `ToolPlugin`. Talon is configured as one such child.
- The container CLI shim continues to use the existing streaming shim route, which now spawns the Rust `talon` binary instead of the in-process `TalonCli` Effect service.

This spec is split into two halves:

- **Part 1 — New `talon` project:** repo layout, binary surface (CLI + MCP), config model (incl. scopes), index extensions (frontmatter, link graph, tombstones), distribution, public TS API.
- **Part 2 — Stripping ultraclaw:** what gets deleted, what survives (watcher + scheduler, now shelling out), the generic MCP-federation architecture, lifecycle/shim wiring, migration plan.

## 1. Why now

The boundary already exists conceptually. The current code:

- Lives entirely under `edge/src/services/talon/` (~10,611 LOC across 107 files in 8 subdirs: `cli`, `embed`, `indexer`, `query`, `search`, `sync`, `watcher`, `shared`).
- Talks to ultraclaw through three narrow surfaces: `EdgeConfig` (the `talon` block), `SidecarClient` (TEI-shaped HTTP for embed/rerank), and a small set of path resolvers.
- Has dead wiring on `DeepResearchSupervisor` — plumbed into `TalonRunDeps` and never actually called. Dropping it is a free win.
- Uses `bun:sqlite` + `sqlite-vec` (with a macOS Homebrew SQLite quirk handled by `Database.setCustomSQLite()`); switching to a Rust binary that statically links sqlite + sqlite-vec eliminates that quirk entirely.

The user-visible contract (`SKILL.md`, the action union, the input schema) is stable and ready to extract. The same opportunity is the right moment to **add** the LLM Wiki primitives — frontmatter querying, change feeds, scopes, lint — because the index is being rebuilt anyway and the new surface is small additive work on top of the existing engine.

## 2. Mental model after the split

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                              talon/ (standalone repo)                        │
│                                                                              │
│   crates/                                                                    │
│     talon-core/        indexer, embed (one-shot), search, query, link graph  │
│                          frontmatter store, scopes, change tracking (lib)    │
│     talon-cli/         binary `talon` — CLI subcommands + --mcp mode         │
│                          (no watcher, no scheduler)                          │
│                                                                              │
│   ts/                  npm package `@seanmozeik/talon`                       │
│     src/index.ts       small surface: mcpChildSpec, resolveBinary            │
│     npm/               per-platform binary subpackages                       │
│                                                                              │
│   skill/SKILL.md       agent skill markdown (single source of truth)         │
└──────────────────────────────────────────────────────────────────────────────┘

                                       │
                                       │ depended on by
                                       ▼

┌──────────────────────────────────────────────────────────────────────────────┐
│                                  ultraclaw                                   │
│                                                                              │
│   edge/                                                                      │
│     package.json                "@seanmozeik/talon": "^x.y.z"                │
│                                                                              │
│     src/services/talon/         MOSTLY DELETED (~10.6K LOC)                  │
│     src/services/talon/         BUT KEEPS thin shells for the survivors:     │
│       watcher/                   chokidar watcher (calls `talon sync`)       │
│       schedule/                  cron-shaped embedding cadence loop          │
│                                  (calls `talon sync` on schedule)            │
│       host-runtime.ts            orchestrates start/stop of the above        │
│       cli-spawn.ts               NEW: spawns the host talon binary,          │
│                                       streams stdout/stderr, returns codes  │
│                                                                              │
│     src/mcp/tools/talon-*.ts    DELETED (no talon-specific MCP plugin file)  │
│                                                                              │
│     src/mcp/children/           NEW (~400 LOC) — generic MCP federation      │
│                                                                              │
│     src/app/server/shim-routes.ts                                            │
│       The talonShim entry now spawns the host `talon` binary directly        │
│       (replacing the in-process TalonCli Effect service).                    │
│                                                                              │
│     src/config/schema.ts                                                     │
│       Removes Talon's *implementation* config (db_path, embed schedule,      │
│       chunk sizes, expansion model, etc.). Keeps *operational* knobs that    │
│       belong to ultraclaw's caller responsibilities (watch on/off, embed     │
│       cadence, file-include patterns the watcher passes to chokidar).        │
│       Adds a new `mcp.children` section.                                     │
└──────────────────────────────────────────────────────────────────────────────┘
```

Three things to internalize:

1. **The Rust binary is the product, and `--mcp` is its only MCP surface.** Standalone hosts (Claude Code, Cursor, anything else) point at the binary directly. Ultraclaw points at it the same way, through a generic federation layer. There is no special "ultraclaw integration mode" on the binary.
2. **Talon does no background work.** No watcher, no scheduler, no cron. `talon --mcp` is a stateless MCP server. Every freshness-related responsibility moves to the caller. Ultraclaw keeps its TS watcher and scheduler — they are the callers.
3. **Ultraclaw becomes a generic MCP aggregator.** Talon-specific MCP code in ultraclaw disappears. The reusable federation layer can mount Talon or any future stdio MCP server.

# Part 1 — The new `talon` project

## 3. Repo layout

```
talon/
├── Cargo.toml                  # workspace root
├── crates/
│   ├── talon-core/             # pure library:
│   │                           #   - indexer (frontmatter, links, FTS, chunking)
│   │                           #   - embed pass (one-shot; runs when sync is called)
│   │                           #   - search (lexical + semantic + rerank + RRF)
│   │                           #   - query (composes search/meta/lint operations)
│   │                           #   - scopes (glob → priority → multiplier)
│   │                           #   - change tracker (tombstones, mtime, since)
│   │                           #   - link graph (wikilinks, backlinks)
│   │                           #   - MCP request handlers (logic only; no I/O)
│   └── talon-cli/              # single binary entry point. Handles:
│                               #   - CLI subcommand parsing
│                               #   - --mcp mode: MCP-over-stdio, owns the request loop
│                               #   - config loading from --config or ~/.config/talon
├── ts/
│   ├── package.json            # npm name: "@seanmozeik/talon"
│   ├── src/
│   │   ├── index.ts            # public API: mcpChildSpec, resolveBinary
│   │   ├── binary.ts           # platform-binary resolution
│   │   └── child.ts            # mcpChildSpec() → { command, args, env } for hosts
│   ├── npm/                    # subpackages: talon-darwin-arm64, talon-darwin-x64,
│   │   │                       #             talon-linux-x64, talon-linux-arm64
│   │   ├── darwin-arm64/
│   │   │   ├── package.json
│   │   │   └── bin/talon
│   │   └── ...
│   └── tsconfig.json
├── skill/
│   └── SKILL.md                # single source of truth; copied from current ultraclaw
├── docs/
│   ├── DESIGN.md               # in-repo Rust-implementation companion
│   ├── CONFIG.md               # config reference (incl. scopes section)
│   └── PROTOCOL.md             # MCP wire protocol notes, CLI exit codes
├── reference/                  # optional: TS implementation snapshot, frozen
└── README.md
```

There is no separate `talon-mcp` crate. The MCP request handler is part of `talon-cli` (or a thin module in `talon-core` if useful for tests). MCP is not a "mode the binary can be reconfigured into" — it's just one of the dispatchers `talon-cli` provides, alongside CLI subcommand dispatch.

There is **no watcher and no scheduler** crate or module in `talon-core`. Those responsibilities are removed from Rust entirely (Decision 1).

The `reference/` directory is a one-time snapshot of the current `edge/src/services/talon/` and `edge/src/mcp/tools/talon-*.ts` source, dropped in unmodified at extraction time, with a note that it is not built. Useful for porting; deleted once the Rust implementation reaches feature parity.

## 4. Process model: stateless, no background work

`talon --mcp` is a **stateless MCP server.** It opens stdin/stdout, reads JSON-RPC frames, dispatches to handlers in `talon-core`, writes responses. Each call opens the DB (or holds a long-lived read connection), runs the query, returns. No clock-driven work. No filesystem watching. No scheduled embedding.

Every freshness concern moves to the caller:

- **Filesystem watching.** Caller (ultraclaw's chokidar watcher; or a launchd plist for standalone users) detects file changes and calls `talon sync <paths>` with the affected paths.
- **Periodic embedding.** Caller (ultraclaw's embed scheduler; or a cron job for standalone users) calls `talon sync` (no paths = full pass) on whatever schedule it likes.
- **DB integrity.** Concurrent writers serialize via SQLite's WAL + a Rust-managed advisory file lock on the DB path; readers (e.g., `talon --mcp` queries) use WAL read snapshots and never block writers.

Standalone users without an external scheduler must either (a) run `talon sync` manually, or (b) write their own cron/launchd entry. The `docs/` directory ships an example launchd plist and a one-line cron snippet. Talon does not solve this for them.

## 5. Binary surface

```
talon search <query> [--scope <name>...] [--scope-only <name>...] [--where '<expr>'...]
                     [--since <ts|relative>] [--mode hybrid|semantic|fulltext|title]
                     [--fast] [--limit N] [--json|--agent]
talon read   <path> [--raw] [--from-line N] [--max-lines N]
talon sync   [paths...] [--fast] [--force]
talon related <path> [--depth N] [--direction outgoing|backlinks|both]
talon status [--json]

talon meta   [--where '<expr>'...] [--since <ts|relative>] [--scope <name>...]
             [--scope-only <name>...] [--select <field>,...] [--tag-counts]
             [--sources <path>] [--limit N] [--json|--agent]

talon changes --since <ts|relative> [--scope <name>...] [--scope-only <name>...]
              [--limit N] [--json|--agent]

talon lint   --check orphans|broken-links|dangling-refs|unreferenced
             [--scope <name>...] [--scope-only <name>...] [--json|--agent]

talon --mcp                                  # stateless MCP-over-stdio
talon --skill                                # print SKILL.md to stdout
talon --version
talon --help
```

Human CLI UX is a first-class surface, not a debug wrapper around JSON. Default human output should be designed for quick reading in modern terminals: colored headings, stable sections, terminal-width-aware wrapping, compact ranked result cards or tables where appropriate, and progress feedback for long-running sync/embed jobs. Machine output is explicit via `--json`/`--agent`; human output should optimize for comprehension.

Initial CLI UX crate choices:

- Styling: `anstyle` plus `anstream` for ANSI-aware output that degrades correctly in pipes.
- Indeterminate waits: `rattles` for lightweight spinners, following the `ddg` pattern.
- Long-running progress: `indicatif` for sync/embed progress bars, multi-progress, ETA, and future tracing integration.
- Tables/layout: defer the final choice until real result rendering lands. Strong candidates are `comfy-table` for rich terminal-width-aware tables and `tabled` when deriving table rows from structs becomes valuable. Do not add either until the renderer has concrete needs.
- Prompts: no prompt crate. Talon should be scriptable and deterministic; `talon init` writes a clear TOML template without interactive questions.

There is no `talon serve` and no `talon daemon`. There is no auto-reindex-on-stale and no mtime-poll-on-call behavior. `talon search` always runs against whatever's in the DB at the moment of the call.

Behavioral guarantees inherited unchanged from current Talon:

- Only one `sync` runs at a time (file lock on the DB path). Parallel `talon sync` invocations from multiple callers either wait or return a busy error (`--no-wait` flag) — same lock semantics as today, but the lock now lives inside the Rust binary rather than ultraclaw.
- `--fast` on `search` means lexical-only (no expansion, no rerank). `--fast` on `sync` means lexical pass only (no embeddings). These are *different* semantics intentionally.
- Returned paths are vault-relative or container-absolute (`/opt/data/workspace/obsidian/...`). Host paths are never returned.
- Magic numbers stay constant: snippet 300, default limit 10, candidate pool `max(limit, 20)`, rerank cap 40, search cache 100, LLM cache 1000, chunk tokens 900, chunk overlap 15%, RRF k=60, strong-signal score/gap 0.85/0.15.

## 6. Scopes

### 6.1 Config shape

Scopes are top-level config primitives. Each scope is `(name, glob, priority, default)`. Talon does **not** know what "wiki" or "private" semantically *mean*; it just sees labels with globs.

```toml
# ~/.config/talon/config.toml — scopes section

[scopes.wiki]
glob     = ["wiki/**", "concepts/**"]    # string or array of strings
priority = "boosted"
default  = true

[scopes.projects]
glob     = "projects/**"
priority = "elevated"
default  = true

[scopes.artifacts]
glob     = "artifacts/**"
priority = "normal"
default  = true

[scopes.raw]
glob     = "raw/**"
priority = "muted"
default  = true

[scopes.daily]
glob     = "daily/**"
priority = "muted"
default  = true

[scopes.archive]
glob     = "archive/**"
priority = "buried"
default  = true

[scopes.private]
glob     = "private/**"
priority = "normal"
default  = false                          # opt-in only via --scope private
```

The README ships this exact configuration as a copy-pasteable Karpathy-shaped preset. No `talon init --llm-wiki` flag — it's a documented snippet.

### 6.2 Priority tiers

Five fixed tiers, calibrated by Talon. Internal multipliers are not user-tunable.

| Tier        | Multiplier | Behavior                                                        |
|-------------|-----------:|-----------------------------------------------------------------|
| `boosted`   | 3.0        | Strong promotion — compiled, curated content (e.g., `wiki/`)    |
| `elevated`  | 1.5        | Mild promotion — actively-used material (e.g., `projects/`)     |
| `normal`    | 1.0        | Neutral — default for unmatched files                           |
| `muted`     | 0.3        | Mild demotion — noisy or unprocessed (e.g., `raw/`, `daily/`)   |
| `buried`    | 0.05       | Strong demotion — only surfaces when relevance is overwhelming  |

If a future deployment needs different tier values, those are edits to a single Rust constant table — not a config-time concern.

### 6.3 File-to-scope resolution

For each file path, Talon walks the configured scopes **in declaration order** and assigns the file to the **first matching scope.** A file that matches no scope is assigned a synthetic "unscoped" bucket with `priority = normal` and `default = true`.

This means specificity is encoded by config order: put narrower or more sensitive scopes (`private`, `archive`) above broader ones (`wiki`) so that overlapping paths resolve to the narrower scope.

### 6.4 CLI semantics

- `talon search "..."` → searches every scope where `default = true`, applying each scope's priority multiplier post-rerank.
- `talon search "..." --scope private` → adds `private` to the active set, additive. (Multi-valued: `--scope a --scope b`.)
- `talon search "..." --scope-only wiki` → searches only `wiki`, ignoring `default` flags entirely. (Multi-valued: `--scope-only a --scope-only b`.)
- `--scope` and `--scope-only` are mutually exclusive on a single invocation.
- Names that don't exist in config produce an error (CLI exit 2; MCP `error.code = invalid-scope`).

### 6.5 Ranking integration

Scope priority is applied as a **post-rerank score multiplier**. Pipeline order (unchanged for the relevance stages, new step at the end):

1. Lexical retrieval (BM25) and semantic retrieval (vector search) produce ranked candidate pools.
2. RRF fuses the two pools into a unified ranking.
3. Reranker (cross-encoder) re-orders the top-K candidates on pure relevance.
4. **(new)** Each candidate's final score is multiplied by its scope's priority multiplier. This is computed in normal numeric space; `final = rerank_score × multiplier`. Results sort by `final` descending.

Reasoning recap (full justification in Decision 4 / §22): the reranker has done the relevance work; the multiplier nudges the final order based on a calibrated prior. A `boosted` candidate above a `normal` one only wins if they were close in raw relevance; a wildly more relevant `normal` result still wins. A `buried` result needs to be ~60× more relevant than a `normal` one to surface — which is what "buried" should mean.

### 6.6 What scopes do NOT do

- Scopes do not affect indexing. Every file matched by `include_patterns` is indexed regardless of scope. Scopes only affect retrieval ranking and visibility.
- Scopes do not affect chunking, embedding, or the link graph. Those are scope-agnostic.
- Scopes are not reflected in the file's stored row beyond the resolved scope name (cached for query-time multiplier lookup).

## 7. Configuration

### 7.1 Sources, in precedence order

1. **Explicit config file.** `--config <path>` or `TALON_CONFIG_FILE=<path>` for humans and standalone MCP hosts that want a non-default config.
2. **`~/.config/talon/config.toml`.** Default host config. Created by `talon init` if absent (writes a commented-out template, including the Karpathy scopes preset).
3. **Built-in defaults.** Last resort for non-path knobs only; `vault_path` and `db_path` must be set before indexing/searching.

Ultraclaw does not inject, adapt, merge, or validate Talon config.

### 7.2 Schema

```toml
# ~/.config/talon/config.toml

vault_path        = "/Users/sean/Library/.../obsidian"
db_path           = "~/.local/share/talon/index.sqlite"
include_patterns  = ["**/*.md"]
ignore_patterns   = [".obsidian/**", ".git/**", "templates/**", "*.canvas"]

[inference]
base_url = "http://localhost:8080"            # any TEI-compatible endpoint

[inference.models]
query_embedding    = "embed"
document_embedding = "embed"
chunk_embedding    = "embed_chunked"
reranker           = "rerank"

[expansion]
provider = "openai-compatible"               # LM Studio, Ollama, etc.
base_url = "http://localhost:1234/v1"
model    = "gemma-smol"

[scopes.wiki]
glob     = "wiki/**"
priority = "boosted"
default  = true

# ... additional [scopes.<name>] entries; see §6.1 for the Karpathy preset.
```

Removed from the previous draft of this spec:

- `index_on_start`, `watch`, `embedding_schedule` — Talon does not run any of these. Move to ultraclaw config (Part 2) where the watcher and scheduler still live.

The TS wrapper does not expose a `TalonConfig` type. Configuration is a Talon-owned host file concern, not an ultraclaw/Bun API concern.

### 7.3 What is NOT in config

These are intentional non-knobs:

- All the magic numbers in §5.
- Priority tier multipliers (§6.2). Owned by Talon. If they need to change, change the binary.
- The DB schema version (managed internally; the binary handles migrations on open).
- The sync lock path (derived from `db_path`).
- The MCP tool name (`talon`, hardcoded — this is the product name, not a setting).

### 7.4 What disappeared from current config

`tools.obsidian.talon` (and any caller-side equivalents) disappear at cutover. Talon does not know about Obsidian-the-tool — it just needs a configured `vault_path`. Ultraclaw does not gate Talon on Obsidian config; it only starts the configured MCP child if the generic `mcp.children.talon.enabled` flag is true.

## 8. Inference abstraction

Talon's only external runtime dependency is the inference endpoint. Current code couples directly to ultraclaw's `SidecarClient`. The standalone version expects any TEI-compatible HTTP endpoint:

```
POST /embed          { inputs: string[] }      → number[][]
POST /embed-chunked  { input: string }         → { data: [{ index, embeddings }] }
POST /rerank         { query, texts, return_text: false } → [{ index, score }]
```

Anyone running Talon standalone can point it at:

- text-embeddings-inference (HuggingFace's TEI server)
- Infinity
- Any custom endpoint matching the three routes above

When used alongside ultraclaw, the user may point Talon's host config at ultraclaw's sidecar because it already speaks this shape. Ultraclaw does not do that wiring for Talon.

The `expansion` endpoint is separate: an OpenAI-compatible chat completions endpoint (typically LM Studio at `localhost:1234/v1`). Used internally by hybrid search for query expansion.

## 9. TypeScript wrapper API

The npm `@seanmozeik/talon` package is a thin helper for hosts that want to mount the talon binary as a federated MCP child or spawn it as a CLI. It does not wrap the MCP protocol or speak JSON-RPC.

```ts
import {
  mcpChildSpec,
  resolveBinary,
} from '@seanmozeik/talon'

// Build a host-agnostic spawn spec for the federation layer to use:
const spec = mcpChildSpec()
// returns: { command, args: ['--mcp'], env: {} }

// resolveBinary() returns the absolute path to the platform-matched talon binary,
// resolved through the optionalDependencies mechanism. Used by:
//   - the shim route, which spawns the binary directly
//   - ultraclaw's surviving watcher, which spawns `talon sync <paths>` on flush
//   - ultraclaw's surviving embed scheduler, which spawns `talon sync` on schedule
const binary = resolveBinary()

// Agent skill text is owned by the Rust binary:
//   talon --skill
```

The wrapper has no `Talon` class, no `.search()`/`.sync()`/`.mcp()` methods. Reasons:

- Standalone hosts that don't want to deal with MCP can call the binary directly via `resolveBinary()` and `child_process.spawn`.
- Ultraclaw doesn't need an in-process `Talon.callTool()` — the federation layer speaks JSON-RPC to the child generically.
- Adding wrapping methods would force the wrapper to maintain a parallel surface to the binary's MCP, recreating exactly the duplication problem we rejected when choosing federation.

There is no in-process Rust binding (no `napi-rs`). The binary always runs as a child process.

## 10. Frontmatter, link graph, and change tracking

These are the index-side foundations that the new query surfaces (`meta`, `changes`, `lint`) read from. They are additive to today's Talon indexer; today the indexer already extracts frontmatter and links during chunking, but stores them in shapes that are not directly queryable.

### 10.1 Frontmatter store

Each indexed file's parsed YAML frontmatter is stored in a structured form:

- A `frontmatter` table with `(file_id, key, value, value_type)`. `value_type ∈ {string, number, bool, date, list}`. Lists are stored as one row per element with the same key. Nested objects are flattened with dotted keys (`sources.0`, `sources.1`).
- A reverse index per `(key, value_type)` for `--where` operator dispatch (range scans on `last_updated`, exact match on `status`, etc.).
- A reverse-source index: `(source_path, file_id)` populated from each file's `sources:` frontmatter list. Drives `meta --sources <path>`.

`--where` filter syntax (Decision 6):

- `key OP value` where `OP ∈ {=, !=, <, <=, >, >=, contains, exists}`.
- Multiple `--where` flags AND together. OR is achieved by issuing multiple Talon calls and unioning client-side.
- Type coercion: dates are parsed leniently (`YYYY-MM-DD` and ISO 8601); numbers and booleans inferred from value shape.

### 10.2 Link graph

Already present in current Talon (drives `related`). Extended to support lint:

- A `links` table with `(source_file_id, target_file_id_or_null, target_text, link_type)`. `link_type ∈ {wikilink, markdown}`. `target_file_id_or_null` is null for unresolved links (drives `lint --check broken-links`).
- A `backlinks` view computed from `links`.
- An `unresolved_targets` view: distinct `target_text` values whose `target_file_id` is null. Drives the agent's "this concept is referenced but doesn't exist" workflow (which, recall from Decision 5, the *agent* computes by reading this view — Talon doesn't have a `coverage` action).

### 10.3 Change tracking and tombstones

`talon sync` records, for each path:

- `last_indexed_at` — when this file was last processed (millis since epoch).
- `last_seen_at` — when this file was last *seen* by a sync pass (whether or not it changed).
- `mtime` — filesystem mtime at last index time.

A `tombstones` table records files that were present in a prior sync and are absent in the current one: `(path, deleted_at)`. Tombstones survive future syncs unless the file reappears (in which case the tombstone is dropped and the file is reindexed normally). Tombstones older than 90 days are pruned.

`--since <ts|relative>`: filters results to rows whose `last_indexed_at >= ts`. Relative forms: `7d`, `24h`, `30m`, `'24 hours ago'`, ISO 8601 absolute. Resolution to absolute timestamp is done in Rust before the SQL filter is applied.

`changes --since <ts>`: returns three lists:

```jsonc
{
  "added":    [{ "path": "...", "indexed_at": 173... }, ...],
  "modified": [{ "path": "...", "indexed_at": 173... }, ...],
  "deleted":  [{ "path": "...", "deleted_at": 173... }, ...]
}
```

`added` are files whose `last_indexed_at >= since` AND no prior `last_indexed_at` entry. `modified` are files whose `last_indexed_at >= since` AND had a prior entry. `deleted` come from the tombstones table where `deleted_at >= since`.

### 10.4 Lint primitives

The `lint` action takes `--check <name>` and runs cheap, deterministic graph queries:

- `orphans` — files in scopes flagged as "graph-rooted" (default: every scope) with no incoming wikilinks. Excludes files whose paths are in a configurable `lint.roots` list (defaults to `index.md`, `README.md`, `_meta/index.md` if present).
- `broken-links` — distinct `(source_file, target_text, line)` triples where the link target doesn't resolve to any indexed file.
- `dangling-refs` — frontmatter `sources:` entries pointing to paths that don't exist in the index.
- `unreferenced` — files with **no** incoming wikilinks AND **no** outgoing wikilinks. Strict subset of `orphans`; surfaced separately because these are the most isolated.

Talon does **not** ship coverage/staleness/duplicate detection. Coverage is computed by the agent (`unresolved_targets` is exposed via `meta` and the agent does set arithmetic). Staleness is a `--where` query on `last_updated` frontmatter. Duplicates are an agent-driven workflow over `talon search` (Decision 5).

### 10.5 Output envelope

Every JSON response uses the unified envelope:

```jsonc
{
  "action":  "search",      // or "meta", "changes", "lint", etc.
  "version": 1,             // bump on breaking shape changes
  "ok":      true,
  "data":    { /* action-specific payload */ },
  "meta":    {
    "duration_ms": 42,
    "result_count": 10,
    "warnings": [],
    "scope_set": ["wiki", "projects"],   // resolved active set, where applicable
    "since":     "2026-04-25T06:00:00Z"  // resolved absolute timestamp, if --since given
  }
}
```

Errors:

```jsonc
{
  "action":  "search",
  "version": 1,
  "ok":      false,
  "error": {
    "code":    "invalid-scope",
    "message": "scope 'foo' is not declared in config",
    "detail":  { /* optional structured context */ }
  }
}
```

Error codes are a small fixed enum (`invalid-scope`, `invalid-where`, `invalid-since`, `db-busy`, `db-corrupt`, `not-indexed`, `internal`, ...). Documented in `docs/PROTOCOL.md`.

## 11. MCP tool surface

A single MCP tool `talon` with action union (Decision 10). The action union is:

```
talon(action: "search"   | { query, scope?, scope_only?, where?, since?, mode?, fast?, limit? })
talon(action: "read"     | { path, raw?, from_line?, max_lines? })
talon(action: "sync"     | { paths?, fast?, force? })
talon(action: "related"  | { path, depth?, direction? })
talon(action: "status"   | { json? })
talon(action: "meta"     | { where?, since?, scope?, scope_only?, select?, tag_counts?, sources?, limit? })
talon(action: "changes"  | { since, scope?, scope_only?, limit? })
talon(action: "lint"     | { check: "orphans" | "broken-links" | "dangling-refs" | "unreferenced", scope?, scope_only? })
```

The `lint.check` parameter is a nested discriminator. All other actions are flat.

The MCP `tools/list` response advertises the tool's full input schema as JSON Schema. Standalone MCP hosts (Claude Code, Cursor) and ultraclaw's federation layer both consume this schema directly; ultraclaw does not translate it into Effect Schema.

## 12. What to copy from the current TS implementation

Aside from being kept under `reference/`, the following pieces transfer almost verbatim as design (not code):

| Piece                                | Notes                                                                     |
|--------------------------------------|---------------------------------------------------------------------------|
| `SKILL.md`                           | Copy unchanged, then extend to document the new actions/flags.            |
| Input schema (`TalonInput` discriminated union by `action`) | Same shape, re-derived in Rust serde, with new `meta`/`changes`/`lint` variants. |
| Output envelope (`TalonResponse`)    | Already the shape current Talon emits; keep.                              |
| Magic numbers (§5)                   | Hardcoded constants in `talon-core`.                                      |
| BM25 / RRF / hybrid-blend formulas   | Copy; OHS-derived.                                                        |
| Chunker (900 tokens, 15% overlap, frontmatter handling, wikilink awareness) | Copy the algorithm; many tests transfer.   |
| Sync lock file format                | Same lockfile semantics; serializes sync runs across multiple callers.    |
| sqlite schema + migrations           | Use as design reference; add new tables for frontmatter store, tombstones, scope cache. Clean rebuild is acceptable at cutover. |

Things explicitly NOT carried over:

- Watcher (`watcher/`) — stays in ultraclaw.
- Embedding scheduler (`embed/scheduler.ts`) — stays in ultraclaw.
- `index_on_start` config knob — moved to ultraclaw if needed at all.
- `DeepResearchSupervisor` plumbing (dead wiring; never called).
- The Mac Homebrew SQLite quirk (`Database.setCustomSQLite`).
- The Effect-specific service/layer composition. Replaced by Rust's normal modular structure.
- Any direct dependency on `EdgeConfig`, `SidecarClient`, ultraclaw error types, or ultraclaw path helpers.

## 13. Distribution

One public distribution channel:

| Channel | Audience                            | Mechanism                                                       |
|---------|-------------------------------------|-----------------------------------------------------------------|
| npm     | Node/Bun consumers (incl. ultraclaw) | `npm i @seanmozeik/talon` pulls the wrapper + platform binary. |

The npm pattern is exactly how `esbuild`, `biome`, and `oxc` distribute: the main package has `optionalDependencies` for each platform, only the matching one installs, and `binary.ts` resolves the path via `require.resolve('@seanmozeik/talon-darwin-arm64/bin/talon')`.

GitHub Actions builds release binaries for the target triples and publishes the npm package/subpackages. Cargo and Homebrew can still be used locally during development, but they are not product distribution channels.

# Part 2 — Stripping ultraclaw

This is the part with the most decisions, because it's where the abstract "Talon is its own project" meets the concrete reality of a working ultraclaw. The new constraint vs. the previous draft of this spec: **the watcher and embed scheduler stay on the TS side**, repurposed to call the host `talon` binary instead of in-process Effect services.

## 14. What gets deleted

```
edge/src/services/talon/embed/                # most of subdir — chunks-*.ts, progress.ts,
                                              # types.ts. scheduler.ts survives, moves to
                                              # services/talon/schedule/ (see §15.1).
edge/src/services/talon/indexer/              # entire subdir — indexing logic
edge/src/services/talon/query/                # entire subdir — query / search engine
edge/src/services/talon/search/               # entire subdir — search runtime
edge/src/services/talon/shared/               # entire subdir — shared helpers used only by the above
edge/src/services/talon/cli/                  # entire subdir — TS CLI runner
edge/src/services/talon/sync/                 # entire subdir — pending-chunks.ts and the
                                              # sync-lock helpers; lock now lives in Rust (§17)
edge/src/services/talon/store.ts              # SQLite store — replaced by Rust binary
edge/src/services/talon/sqlite-vec.ts         # sqlite-vec shim — replaced
edge/src/services/talon/db-path.ts            # path helper — replaced by Rust binary
edge/src/services/talon/searcher.ts           # search adapter — replaced
edge/src/services/talon/runtime.ts            # in-process runtime — replaced

edge/src/mcp/tools/talon-build.ts
edge/src/mcp/tools/talon-db.ts
edge/src/mcp/tools/talon-errors.ts
edge/src/mcp/tools/talon-execute.ts
edge/src/mcp/tools/talon-read.ts
edge/src/mcp/tools/talon-search.ts
edge/src/mcp/tools/talon-search-context.ts
edge/src/mcp/tools/talon-search-input.ts
edge/src/mcp/tools/talon-search-related.ts
edge/src/mcp/tools/talon-status.ts
edge/src/mcp/tools/talon-sync.ts
edge/src/mcp/tools/talon.ts                   # the plugin file too — federation handles it
                                              # ~920 LOC total
```

Combined deletion: ~10K LOC.

There is no `mcp/tools/talon.ts` after the change. Talon's tool definition (description, schemas) comes from the talon child's `tools/list` response, populated into ultraclaw's tool registry by the generic federation layer (§15.2).

## 15. What survives — and what gets added

### 15.1 Surviving from `services/talon/`

Three subdirectories survive the cutover; each is meaningfully simpler in the new shape:

```
edge/src/services/talon/
├── watcher/                # chokidar-based file watcher (kept; calls `talon sync <paths>`)
│   ├── chokidar.ts             # unchanged: createVaultWatcher() factory
│   ├── callbacks.ts            # unchanged: include/ignore filters
│   ├── pending.ts              # unchanged: pendingChanges/pendingDeletes Sets
│   ├── schedule.ts             # unchanged: debounced flush
│   ├── constants.ts            # unchanged
│   ├── paths.ts                # unchanged
│   └── index.ts                # CHANGED: TalonWatcher.start/stop unchanged at the top,
│                               #          the flush handler that previously called
│                               #          TalonIndexer in-process now spawns
│                               #          `talon sync <paths>` via cli-spawn.ts
│
├── schedule/               # cron-shaped embedding cadence loop (renamed from embed/)
│   └── scheduler.ts            # CHANGED: TalonEmbeddingScheduler keeps its public shape
│                               #          (embedStartup, embedManual, scheduleWork),
│                               #          but each call now spawns `talon sync` instead
│                               #          of running the in-process embed pass
│
├── host-runtime.ts         # CHANGED: orchestrates watcher + scheduler.
│                           #          The TalonHostRuntime service still exists; it now
│                           #          (a) does NOT depend on TalonStore/TalonIndexer/
│                           #              SidecarClient,
│                           #          (b) starts/stops only watcher + scheduler,
│                           #          (c) gates on isTalonIndexingEnabled(config) using
│                           #              the new ultraclaw-side talon block (§16.1).
│
├── cli-spawn.ts            # NEW (~80 LOC): single chokepoint for spawning the host
│                           #                talon binary as a subprocess. Resolves the
│                           #                binary path via `@seanmozeik/talon`'s
│                           #                resolveBinary(), streams stdout/stderr,
│                           #                returns exit code + parsed JSON envelope.
│                           #                Used by watcher flush, scheduler runs, and
│                           #                shim routes (§16.4).
│
├── errors.ts               # KEPT but trimmed: TalonStoreError → TalonCliError;
│                           #                   TalonSyncLockBusy maps to a binary exit
│                           #                   code from talon (sync busy).
│
└── sync/                   # DELETED entirely: pending-chunks.ts (embed-pipeline glue —
                            # gone with the indexer); sync-lock.ts and sync-lock-busy.ts
                            # (lock now lives inside the Rust binary, §17).
```

The old `embed/` subdir is renamed to `schedule/`; only `scheduler.ts` survives (drastically simplified — it no longer composes `getPendingChunks`, `embedChunksAndWrite`, etc., because those are gone). All the `chunks-*.ts`, `progress.ts`, `types.ts` files under `embed/` are deleted along with the rest of the indexing pipeline.

### 15.2 New: `edge/src/mcp/children/` — generic MCP federation layer

This is the core new thing in ultraclaw. It is *not* talon-specific. It mounts arbitrary stdio MCP servers as federated tool providers.

```
edge/src/mcp/children/
├── service.ts          # McpChildren: ServiceMap.Service, layer
├── child.ts            # Per-child handle: stdio streams, request id counter, in-flight map
├── handshake.ts        # initialize → notifications/initialized → tools/list
├── proxy-plugin.ts     # ToolPlugin wrapper that forwards execute via tools/call
├── lifecycle.ts        # Restart-with-backoff, crash detection
├── notifications.ts    # Handle notifications/tools/list_changed
├── timeouts.ts         # Per-request timeout + cancellation via notifications/cancelled
└── errors.ts           # McpChildError, McpChildSpawnError, McpChildProtocolError
```

**Lifecycle (per child):**

1. On startup, read `config.mcp.children`, filter to enabled, spawn each.
2. For each spawned child, perform the MCP handshake: `initialize` with standard tool support, wait for the response, send `notifications/initialized`, then `tools/list`.
3. For each tool returned, construct a proxy `ToolPlugin`:
   - `name`, `description` from the child.
   - `getInputSchema`/`getOutputSchema` from the child's `inputSchema`/`outputSchema` as raw JSON Schema. Federated proxy plugins do not translate JSON Schema into Effect Schema.
   - `isEnabled` is gated on the child being healthy.
   - `execute(input, ctx)` writes a `tools/call` JSON-RPC frame to the child's stdin, awaits the matching response on stdout, returns the result as `CallToolResult`.
4. Register all proxy plugins into the existing `ToolRegistry`.

**Crash handling.** If a child's stdio pipe closes or a heartbeat times out, drop its tools from the registry, attempt restart with exponential backoff (default `restartBackoffMs=1000`, doubling, capped). After `maxRestarts` consecutive failures within a window, log loudly and stop trying until config changes.

**Timeouts and cancellation.** Each `tools/call` request has a per-child default timeout (60s, configurable per child). On timeout, send `notifications/cancelled` to the child with the request id and surface a timeout error to the agent.

**Notifications.** `notifications/tools/list_changed` from a child triggers a re-fetch of `tools/list` and a registry update.

### 15.3 `package.json`

```jsonc
{
  "dependencies": {
    "@seanmozeik/talon": "^x.y.z",
    // chokidar already present and stays
    "chokidar": "^x.y.z"
  }
}
```

## 16. Surgical edits to non-deleted files

`grep` shows ~21 ultraclaw files reference `services/talon/...` directly. After the change, most of them just *delete* the in-process Talon import — the service that used to be there is replaced by a federated MCP child plus the slim TS-side watcher/scheduler shells.

### 16.1 `edge/src/config/schema.ts`

Two changes, both substantive:

- **Delete the implementation half of `EdgeConfig.talon`.** Remove the full `talon` block as it exists today: `db_path`, `chunk_*`, expansion model wiring, embed model references, etc. Talon owns those.
- **Keep a minimal ultraclaw-side `talon` block** for *operational caller knobs*:

```ts
talon: {
  enabled: boolean,             // master kill switch for both watcher and scheduler
  watch: boolean,               // run the chokidar watcher
  indexOnStart: boolean,        // run a `talon sync` once on edge startup
  embeddingSchedule: string[],  // HH:MM times to fire scheduled embedding passes
  vaultPath: string,            // chokidar root + path passed via TALON_CONFIG_FILE if needed
  ignorePatterns: string[],     // chokidar ignores (separate from Talon's own ignore_patterns)
  syncTimeoutMs: number,        // hard timeout on each spawned `talon sync` invocation
}
```

This block is *not* injected into Talon's config. It controls only ultraclaw's own watcher/scheduler. Talon reads its own `~/.config/talon/config.toml`.

- **Add `mcp.children`** as in §15.2.

`tools.obsidian.talon` is deleted entirely; readiness gating moves to `talon.enabled` and the generic `mcp.children.talon.enabled`.

### 16.2 `edge/src/services/talon/host-runtime.ts`

Currently composes `TalonStore`, `TalonIndexer`, `TalonWatcher`, `TalonEmbeddingScheduler`, plus the sync-lock helpers. After cutover:

- Drops `TalonStore`, `TalonIndexer`, and the in-process sync-lock dependency.
- Keeps `TalonWatcher` and `TalonEmbeddingScheduler` (now both backed by `cli-spawn.ts`).
- `start()` runs `indexOnStart` (one-shot `talon sync`) followed by `watcher.start()` and the scheduler loop.
- `stop()` shuts both down and waits for any in-flight `talon sync` subprocess to complete or hits `syncTimeoutMs` and SIGTERMs it.
- Gating is `config.talon.enabled` (top-level kill switch). `config.tools.obsidian.*` is no longer consulted.

### 16.3 `edge/src/app/server/server.ts` and `runner-layers-core.ts`

Delete `TalonCli`, `TalonStore`, `TalonIndexer` from the layer graph. Keep `TalonWatcher`, `TalonEmbeddingScheduler`, `TalonHostRuntime` (now in their slimmer form). Add `McpChildren` to the layer graph. There is no talon-specific MCP service entry — talon participates as an entry in `config.mcp.children`.

### 16.4 `edge/src/app/cli/lifecycle-service.ts`, `lifecycle-daemon.ts`, `runner.ts`

Currently start/stop `TalonHostRuntime` alongside other services. Continue to do so — the service still exists in its slimmer form. Additionally start/stop `McpChildren` in the same lifecycle phase.

### 16.5 `edge/src/app/cli/doctor.ts`, `doctor-probes.ts`

Delete Talon-specific doctor probes that introspected the in-process index. Replace with a single thin probe that runs `talon status --json` via `cli-spawn.ts` and surfaces its envelope; agent and CLI users get the canonical readiness view from Talon itself.

### 16.6 `edge/src/app/server/shim-routes.ts`

Currently `talonShim` invokes the in-process `TalonCli` Effect service:

```ts
deps.talon.run(input)  // current
```

After the change, `talonShim` becomes a normal "spawn a host CLI" handler — same shape as `gogShim`, `birdShim`, etc. — pointing at the resolved `talon` binary path through `cli-spawn.ts`:

```ts
import { resolveBinary } from '@seanmozeik/talon'
const binary = resolveBinary()
ctx.spawn(binary, args, { stdin })            // existing streaming-shim machinery
```

The streaming shim infra (POST /shim/talon, NDJSON downstream, SIGINT/SIGWINCH forwarding) is unchanged. PTY mode for `talon` becomes available for free since it's now a real subprocess.

### 16.7 `edge/src/mcp/tools/plugin.ts`

The `ToolDeps` and `ToolContext` types currently include Talon-specific deps (`hostTalonStore`, the various Talon services). Strip those out. No replacements added — federated proxy plugins don't need ultraclaw-specific deps.

### 16.8 `edge/src/clients/...`

No changes. The `SidecarClient` continues to be used by other parts of edge (prompt-guard, transcribe, etc.). Talon-specific use sites are gone, but the client is general-purpose. **Note:** `SidecarClient` is *not* used by the surviving watcher/scheduler — they spawn `talon sync` and Talon talks to inference itself.

### 16.9 `.agents/skills/talon/SKILL.md`

No action. Talon's Rust binary embeds `skill/SKILL.md` with `include_str!` and prints it via `talon --skill`, matching `ddg`. Ultraclaw users are responsible for ensuring the skill exists in ultraclaw if they want it; the container already has the current skill.

### 16.10 Container Dockerfile

No changes initially. The container `talon` shim continues to be a symlink to `_ultraclaw_shim`, which routes through edge. The `talon` binary lives only on the host.

Future option (not part of this change): install the `talon` binary inside the container too, so in-container invocations skip the edge round-trip. Defer until there's a reason.

## 17. The lock — clarified

Current Talon's sync lock lives in TypeScript (`sync/sync-lock.ts`). After the cutover, **the lock moves entirely into the Rust binary.** Specifically:

- The Rust binary takes a Rust-managed advisory file lock on the DB path at the start of every `talon sync` invocation. Concurrent invocations from any source (ultraclaw watcher, ultraclaw scheduler, manual CLI) serialize through this lock.
- Read-only operations (`search`, `read`, `meta`, `changes`, `lint`, `related`, `status`) do not take the write lock; SQLite WAL handles read/write concurrency.
- `talon sync` defaults to *waiting* for the lock; `--no-wait` returns immediately with a busy error.

Ultraclaw's surviving watcher and scheduler do **not** maintain their own lock. They just spawn `talon sync` and react to its exit code:

- `0`: success.
- non-zero (specific code): busy. The watcher coalesces — the next debounce flush will retry naturally because the file change set has been preserved. The scheduler logs and skips the missed slot; the next scheduled tick handles whatever changed.

This eliminates the dual-locking concerns and removes `sync/sync-lock.ts` from the TS side entirely.

## 18. MCP merging — generic federation

The chosen architecture: **ultraclaw's MCP server federates arbitrary stdio MCP servers.** Talon is one configured child. Standalone hosts (Claude Code, Cursor) point at `talon --mcp` directly with the same protocol. Talon does not have, and does not need, any ultraclaw-specific protocol.

### 18.1 Why federation, not plugin embed

A "second flag" approach (talon exposing a non-MCP bridge protocol just for ultraclaw) and a plugin-embed approach (ultraclaw bundles talon's tool definition statically and proxies execute calls without doing federation) were both rejected. Both create a parallel surface that has to be maintained alongside MCP, and both make talon care about ultraclaw. Federation factors the problem correctly: talon speaks one protocol (MCP), ultraclaw learns to mount any MCP server, talon happens to be the first one we mount.

The federation work is bounded (~400 LOC of focused JSON-RPC plumbing and lifecycle bookkeeping) and reusable for any future stdio MCP server.

### 18.2 Request flow

```
agent calls tool "talon" via ultraclaw's MCP server
       │
       ▼
ultraclaw MCP server validates origin/auth/rate-limit (unchanged)
       │
       ▼
ToolRegistry resolves the name → proxy plugin (registered by McpChildren at startup)
       │
       ▼
proxy.execute(input, ctx)
       │
       ├─ child handle healthy?  no → return McpChildError("talon not ready: <reason>")
       │
       ▼
write tools/call JSON-RPC frame to child stdin
       │   id assigned by ultraclaw, in-flight map tracks pending request
       │
       ▼
read response from child stdout (matched by id)
       │
       ▼
return CallToolResult to the MCP server
       │
       ▼
serialize back to the agent
```

The talon child is spawned by `McpChildren` at ultraclaw startup, not lazily. The handshake completes before the registry is populated, so `tools/list` from agents always reflects ready state. If the child crashes, the federation layer drops talon's tools from the registry, restarts with backoff, and re-populates on success.

### 18.3 Standalone use

The same `talon --mcp` binary is what an external MCP host runs:

```jsonc
// e.g. Claude Code's MCP servers config
{
  "mcpServers": {
    "talon": {
      "command": "/usr/local/bin/talon",
      "args": ["--mcp"],
      "env": { "TALON_CONFIG_FILE": "~/.config/talon/config.toml" }
    }
  }
}
```

Standalone hosts get exactly the same tools, schemas, and behavior as ultraclaw — there is no second mode to maintain. Standalone users without ultraclaw must arrange their own indexing cadence (manual `talon sync`, launchd, cron, fswatch, etc.). Talon does not solve that for them.

### 18.4 Concurrency

The Talon MCP server exposes the same tool behavior as the current TypeScript implementation, plus the new `meta`/`changes`/`lint` actions. Parallelism is internal to the Rust server.

## 19. Migration plan

Phased so the Talon repo can mature independently, but the ultraclaw cutover itself is clean. There is no long-running dual backend and no legacy compatibility layer.

### Phase 0 — Extract the reference

Snapshot the current TS implementation into the new repo's `reference/`. Confirms the new repo structure works; gives the Rust port a known-good source to consult. Ultraclaw is unchanged. 1 day.

### Phase 1 — Build the Rust binary at parity-plus

Implement `talon-core`, `talon-cli` in the new repo. Two acceptance bars:

1. **Parity:** `search`, `read`, `sync`, `related`, `status` produce equivalent results on a fixture vault to the legacy TS implementation. The parity test runs both implementations side-by-side over the same input set and asserts envelope equivalence on `data.results[*].path`, `data.results[*].score` (within tolerance), and a stable subset of `meta` fields.
2. **Plus:** `meta`, `changes`, `lint`, scopes, `--where`, `--since`, `--scope`/`--scope-only` all implemented, with their own integration tests against the fixture vault.

Ship `talon` binaries through npm package `@seanmozeik/talon`. Ultraclaw is still unchanged, still using the TS implementation. This is the bulk of the work; planned in the new repo.

### Phase 2a — Build the generic MCP federation layer

Build `edge/src/mcp/children/` (§15.2) without any talon involvement. Test against a trivial fixture child MCP server (a 50-LOC test server that exposes `echo` and `slow_echo` tools). Cover: handshake, tools/list registration, tools/call round-trip, crash + restart, and timeout/cancel.

Independent of talon. Shippable on its own. The fixture child stays in the test suite as a regression target for federation behavior.

Acceptance: federation tests pass. `config.mcp.children` accepts entries; non-talon children can be configured ad-hoc for testing.

### Phase 2b — Repurpose watcher and scheduler

Add `cli-spawn.ts`. Rewrite `TalonWatcher`'s flush handler and `TalonEmbeddingScheduler`'s pass runner to spawn `talon sync` via `cli-spawn.ts`. Drop in-process indexer dependencies. Run integration tests on the resulting host runtime against a real `talon` binary.

This phase can land *before* Phase 2c — it just requires the Rust binary to be installed (which Phase 1 provides). Until Phase 2c lands, the federated MCP path is dormant; the watcher and scheduler are already operating against the new binary.

Acceptance: edge process running with `enabled = true, watch = true, embeddingSchedule = ['03:00']` correctly detects vault changes via chokidar, spawns `talon sync <paths>` on debounce, runs scheduled embed passes, and observes the lock when concurrent invocations race.

### Phase 2c — Clean Talon Cutover

Add `talon` to `edge/package.json`. Add the `talon` entry to `config.mcp.children`, enabled by the generic child config. Delete the TS implementation in the same change:

- Phase 2b should already have moved `embed/scheduler.ts` to `schedule/scheduler.ts`. Now `rm -rf edge/src/services/talon/{embed,indexer,query,search,shared,cli,sync}/`.
- `rm` the in-process `store.ts`, `sqlite-vec.ts`, `db-path.ts`, `searcher.ts`, `runtime.ts`.
- Delete all `edge/src/mcp/tools/talon-*.ts` files (no new plugin file replaces them).
- Trim `EdgeConfig.talon` to the §16.1 shape; delete `tools.obsidian.talon`.
- Drop tests that no longer apply; the Rust binary's own test suite is authoritative for Talon behavior. Keep tests under `edge/src/tests/services/talon/` that cover the surviving watcher/scheduler shells (they now exercise `cli-spawn.ts` against a stub binary).

Acceptance: Talon is configured in `~/.config/talon/config.toml` on the host, ultraclaw starts it as a federated MCP child *and* drives `talon sync` from the surviving watcher and scheduler, the federated `talon` tool works through `tools/list`/`tools/call`, the container shim spawns the host binary, and the ultraclaw repo no longer contains in-process Talon implementation code.

### Phase 3 — Reclaim wins

Now that Talon's implementation is out of `edge/`, reconsider the REFACTOR.md restructure. Some of its goals (collapsing `cli/` into `services/talon/cli/`, splitting `services/talon/shared/` from `talon/`) become moot. Update REFACTOR.md to reflect the post-extraction reality.

## 20. Risks and how we mitigate them

| Risk                                                    | Mitigation                                                                                |
|---------------------------------------------------------|--------------------------------------------------------------------------------------------|
| Rust port has subtle behavior differences vs. TS        | Phase 1 parity test on a fixture vault. Cut over only after parity passes; no dual-backend legacy layer. |
| New surfaces (`meta`/`changes`/`lint`/scopes) regress over time | Each new surface gets dedicated integration tests in the Rust repo, separate from parity. |
| Existing ultraclaw Talon DB is not reusable by Rust     | Accepted. Clean cutover; Talon rebuilds its own index from the configured vault.          |
| Subprocess spawn cost on watcher flush hurts UX         | Watcher debounce (60s) coalesces bursts. Each spawn is ~30ms cold-start; negligible compared to indexing time. Measure if it becomes suspect. |
| Subprocess spawn cost on MCP cold start hurts UX        | Long-lived MCP child (§18). Federation keeps the child running.                           |
| Existing ultraclaw Talon config users must migrate      | Accepted. Talon setup moves to `~/.config/talon/config.toml`; document the one-time migration. |
| Talon project becomes a maintenance burden separate from ultraclaw | Accepted. The standalone-product framing is the explicit goal.                |
| Scope misconfiguration silently changes ranking         | `talon status` reports the resolved scope set, scope counts per file, and any unmatched files. Easy to inspect. |
| Frontmatter `--where` filter syntax becomes a mini-DSL  | Hard limit on operator set (`=, !=, <, <=, >, >=, contains, exists`). No OR, no nesting, no functions. If it grows past this, design a separate query language explicitly. |

## 21. Out of scope for this spec

- Concrete Rust crate choices (`rusqlite` vs `sqlx`, async runtime, JSON-RPC library, etc.). Planned in the new repo.
- The Rust implementation of any specific module (chunker, BM25, RRF). Planned in the new repo.
- Any background work in Talon — no watcher, no scheduler, no auto-reindex, no auto-stale checks. Talon is stateless; callers own cadence (Decision 1).
- Semantic deduplication, coverage analysis, topic clustering, automated frontmatter normalization. These are agent-side responsibilities (Decision 5).
- Container-side `talon` binary install.
- `napi-rs` bindings.
- Legacy compatibility for `tools.obsidian.talon` or the old `EdgeConfig.talon` implementation block. Both are deleted at cutover.
- A dashboard / config UI for `mcp.children`. Hand-edited config only.
- Forwarding `notifications/progress` from federated children to ultraclaw's MCP clients. Not required for Talon parity.
- Multi-vault support. Single vault per Talon installation.
- A `talon init --llm-wiki` preset. The README ships the snippet; users copy-paste.

In scope (and explicitly *not* rejected):

- **Generic MCP federation in ultraclaw.** Core architectural addition; pays for itself the first time a second federated server lands.
- **Survival of ultraclaw's chokidar watcher and embed scheduler** as `talon sync` callers (Decision 1, applied to the cutover).
- **The LLM Wiki primitive layer** (scopes, frontmatter querying, change feed, lint primitives) on the Talon side.

## 22. Resolved Decisions

The following decisions came out of the brainstorming session that produced this spec. All are baked in above; this section is the canonical change-log.

1. **Process model.** Talon is **stateless** — no watcher, no scheduler, no clock-driven background work. Every CLI/MCP call opens the DB, answers, returns. Callers own all cadence. `--mcp` is a stateless MCP server. (§4)

2. **Directory awareness.** Opaque labels — Talon does not know what "wiki" or "private" means semantically. README ships a Karpathy-shaped preset. (§6.1)

3. **Scope config shape.** Top-level `[scopes.<name>]` keys. Each scope = `glob` (string or list) + `priority` enum + `default: bool`. File-to-scope: first match wins, in config order. Unmatched files fall into a synthetic unscoped bucket with `priority = normal`, `default = true`. (§6.1, §6.3, §6.4)

4. **Ranking.** Post-rerank score multiplier. Calibrated, Talon-owned multipliers per priority tier: `boosted=3.0, elevated=1.5, normal=1.0, muted=0.3, buried=0.05`. Not user-tunable. (§6.2, §6.5)

5. **Lint scope.** Family 1 only — graph defects: `orphans, broken-links, dangling-refs, unreferenced`. No coverage/staleness/dups/clustering — agent computes those by composing `meta`, `search`, and `changes` itself. (§10.4)

6. **Frontmatter querying.** Dedicated `meta` action + `--where` filter on `search`/`list`/`meta` with operators `=, !=, <, <=, >, >=, contains, exists`. Multiple `--where` = AND. Reverse-source index (`meta --sources <path>`). Tag-counts. (§10.1, §11)

7. **Change-aware queries.** `--since` filter on `search`/`list`/`meta` + dedicated `changes` action returning `{added, modified, deleted}` with tombstones. (§10.3, §11)

8. **Output envelope.** Unified `{action, version, ok, data, meta}`. Errors: `{ok: false, error}`. Already current pattern; extend rather than reinvent. (§10.5)

9. **Config location.** Single `~/.config/talon/config.toml`. Scopes section alongside existing keys. Ultraclaw does not inject Talon config. (§7)

10. **MCP tool surface.** Single `talon` tool. Top-level actions: `search, read, sync, related, status, meta, changes, lint`. `lint` nests `check ∈ {orphans, broken-links, dangling-refs, unreferenced}`. (§11)

11. **Host Talon config setup.** Talon reads `~/.config/talon/config.toml` by default. `talon init` creates `~/.config/talon/` and `config.toml` if missing, writes a clear TOML template (including the Karpathy scopes preset), and does not overwrite an existing file. (§7.1)

12. **Skill distribution.** Talon embeds `skill/SKILL.md` in the Rust binary and exposes it via `talon --skill`, same as `ddg`. Ultraclaw does not regenerate or verify this file. (§9, §16.9)

13. **Standalone embedder docs.** The README explains that `inference.base_url` should point at a TEI-compatible endpoint such as text-embeddings-inference, Infinity, or ultraclaw's sidecar. (§8)

14. **Distribution name.** npm package name is `@seanmozeik/talon`. npm is the product distribution channel. No crates.io or Homebrew release is required. (§13)

15. **Tool-name collisions.** No special collision policy. Talon exports `talon`; the existing registry uniqueness behavior is enough.

16. **MCP capability and parity.** Talon MCP must expose at minimum the same tool behavior as the current TypeScript implementation — same tool name, same action variants for the existing actions (search/read/sync/related/status), same response shape — plus the new `meta`/`changes`/`lint` actions added in this spec. (§11, §19 Phase 1)

17. **Doctor probes.** A single thin doctor probe that runs `talon status --json` and surfaces its envelope. No federated-child doctor model. (§16.5)

18. **Schema handling.** Federated MCP children provide JSON Schema in `tools/list`; ultraclaw proxy plugins carry that JSON Schema directly. No Effect Schema translation; no Talon-specific Effect Schema exports. (§15.2)

19. **Container shim.** The container `talon` command routes through edge's normal host CLI shim, like gog/remi/bird. Edge spawns the host `talon` binary. (§16.6, §16.10)

20. **Sync lock ownership.** The DB write lock lives inside the Rust binary. Ultraclaw's watcher and scheduler do not maintain their own lock; they spawn `talon sync` and react to its exit code. (§17)

21. **Ultraclaw config split.** `EdgeConfig.talon` is reduced to operational caller knobs only (`enabled, watch, indexOnStart, embeddingSchedule, vaultPath, ignorePatterns, syncTimeoutMs`). Implementation knobs (`db_path`, embedding model wiring, chunk parameters, expansion model) move to `~/.config/talon/config.toml`. (§16.1)
