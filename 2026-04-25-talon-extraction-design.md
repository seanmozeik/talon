# Talon: Extraction to Standalone Rust Project

**Date:** 2026-04-25
**Status:** design — architectural questions resolved
**Scope:** Move Talon out of `edge/src/services/talon/` into a standalone repository. Reimplement the core in Rust, wrap with a small TypeScript package for ergonomic Bun consumption, and refactor ultraclaw to consume Talon as an external dependency.

## How to use this spec in a new session

This spec is self-contained — a fresh session can implement without re-deriving anything from the codebase. Skim §0 (TL;DR), §2 (mental model), §14 (MCP federation), and §18 (resolved decisions) in that order. The decisions already locked in:

- Talon ships as a standalone Rust binary in a separate repo, with a thin npm wrapper (`talon`) that exposes `mcpChildSpec` and `resolveBinary` (§8). Agent skill text is exposed by the binary via `talon --skill`, matching the `ddg` pattern.
- `--mcp` is the only MCP mode; when configured with `watch: true` it also runs the watcher and embed scheduler in-process (§4, §7). No `serve` subcommand.
- Ultraclaw consumes Talon by way of a generic stdio-MCP federation layer at `edge/src/mcp/children/` (~400 LOC) — talon is one entry in `config.mcp.children` (§12.2, §12.3, §14). No talon-specific MCP plugin file, supervisor, config adapter, or config injection.
- Talon owns its host config at `~/.config/talon/config.toml`, same operational model as tools like bird/gog. Ultraclaw assumes Talon is already installed and configured.
- npm distribution uses the scoped package `@seanmozeik/talon`; there is no crates.io or Homebrew distribution requirement.
- `DeepResearchSupervisor` plumbing into Talon was dead wiring; dropped.
- sqlite + sqlite-vec statically linked into the Rust binary (the macOS Homebrew SQLite quirk goes away).
- Container shim continues to route through edge; the shim path spawns the host `talon` binary directly (§13.4).

No additional architecture exploration is required — all relevant findings are already inlined (file paths and LOC counts in §11, external imports in §1, talon's current ultraclaw coupling surface in §1, container-shim machinery in §13.4).

## 0. TL;DR

Talon becomes its own product. New repo ships:

- A Rust binary `talon` with CLI subcommands (`search`, `read`, `sync`, `status`, `related`) and an `--mcp` flag that puts the same process into MCP-over-stdio mode. When `--mcp` is run with `watch: true` and a `vaultPath` configured, the same process *also* runs the watcher and embed scheduler in-process. One process, all roles.
- A thin TypeScript package `@seanmozeik/talon` (npm) that resolves the per-platform prebuilt binary and exposes a small API for ultraclaw-style hosts to use it as an MCP child (spawn command + args only; Talon reads its own host config).

Ultraclaw deletes ~10.6K LOC under `services/talon/` plus the ~900 LOC of `mcp/tools/talon-*.ts`. In its place:

- `edge/` adds `@seanmozeik/talon` as an npm dependency.
- A new generic `McpChildren` federation layer in ultraclaw spawns and supervises arbitrary stdio MCP servers, fetches their `tools/list`, and registers each as a proxy `ToolPlugin`. Talon is configured as one such child. The same federation works for any future stdio MCP server (rust-analyzer, filesystem MCP, etc.).
- No talon-specific supervisor, no talon-specific MCP plugin file, no Talon config adapter, and no Talon config block in ultraclaw after cutover.
- The container CLI shim continues to use the existing streaming shim route, which now spawns the Rust `talon` binary instead of the in-process `TalonCli` Effect service.

This spec is split into two halves:

- **Part 1 — New `talon` project:** what the standalone project contains, configuration model, distribution, public TS API. Implementation-level Rust details are deliberately deferred to be planned inside the new repo.
- **Part 2 — Stripping ultraclaw:** what gets deleted, what replaces it, the generic MCP-federation architecture, lifecycle/shim wiring, migration plan.

## 1. Why now

The boundary already exists conceptually. The current code:

- Lives entirely under `edge/src/services/talon/` (~10,611 LOC across 107 files in 8 subdirs: `cli`, `embed`, `indexer`, `query`, `search`, `sync`, `watcher`, `shared`).
- Talks to ultraclaw through three narrow surfaces: `EdgeConfig` (the `talon` block), `SidecarClient` (TEI-shaped HTTP for embed/rerank), and a small set of path resolvers.
- Has dead wiring on `DeepResearchSupervisor` — plumbed into `TalonRunDeps` and never actually called. Dropping it is a free win.
- Uses `bun:sqlite` + `sqlite-vec` (with a macOS Homebrew SQLite quirk handled by `Database.setCustomSQLite()`); switching to a Rust binary that statically links sqlite + sqlite-vec eliminates that quirk entirely.
- Watches via `chokidar`; `notify` (Rust) gives equivalent FSEvents fallback for iCloud-backed dirs.

Talon is not feature-frozen, but it has reached the point where the *user-visible contract* (`SKILL.md`, the four actions, the input schema) is stable. That is the right moment to extract — before the implementation accretes more ultraclaw-specific assumptions.

## 2. Mental model after the split

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                              talon/ (standalone repo)                        │
│                                                                              │
│   crates/                                                                    │
│     talon-core/        indexer, embed, search, query, watcher, sync (lib)    │
│     talon-cli/         binary `talon` — CLI subcommands + --mcp mode         │
│                                                                              │
│   ts/                  npm package `@seanmozeik/talon`                       │
│     src/index.ts       small surface: mcpChildSpec, resolveBinary            │
│     npm/               per-platform binary subpackages                       │
│                                                                              │
│   skill/SKILL.md       agent skill markdown (single source of truth)         │
│                                                                              │
│   Output:                                                                    │
│     - npm i @seanmozeik/talon (pulls platform-specific prebuilt binary)      │
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
│     src/services/talon/         DELETED                                      │
│     src/mcp/tools/talon-*.ts    DELETED (no talon-specific MCP plugin file)  │
│                                                                              │
│     src/mcp/children/           NEW (~400 LOC) — generic MCP federation      │
│       Spawns + supervises arbitrary stdio MCP servers configured in          │
│       config.mcp.children. Does the initialize/tools/list handshake.         │
│       Registers each child tool as a proxy ToolPlugin in the existing        │
│       ToolRegistry. Restart-on-crash with backoff.                           │
│                                                                              │
│     src/app/server/shim-routes.ts                                            │
│       The talonShim entry now spawns the host `talon` binary directly        │
│       (replacing the in-process TalonCli Effect service).                    │
│                                                                              │
│     src/config/schema.ts                                                     │
│       Removes the existing `talon` config block. Adds a new                  │
│       `mcp.children` section listing federated MCP servers.                  │
└──────────────────────────────────────────────────────────────────────────────┘
```

Three things to internalize:

1. **The Rust binary is the product, and `--mcp` is its only MCP surface.** Standalone hosts (Claude Code, Cursor, anything else) point at the binary directly. Ultraclaw points at it the same way, through a generic federation layer. There is no special "ultraclaw integration mode" on the binary.
2. **MCP can also run background work.** When `--mcp` runs with `watch: true`, the process also runs the watcher and embed scheduler. CLI subcommands (`talon search`, `talon sync`) are ordinary host CLI invocations, not MCP clients.
3. **Ultraclaw becomes a generic MCP aggregator.** Talon-specific code in ultraclaw disappears. The reusable federation layer can mount Talon or any future stdio MCP server.

# Part 1 — The new `talon` project

## 3. Repo layout

```
talon/
├── Cargo.toml                  # workspace root
├── crates/
│   ├── talon-core/             # pure library: indexer, embed, search, query, watcher, sync,
│   │                           # MCP request handlers (logic only; no I/O)
│   └── talon-cli/              # single binary entry point. Handles:
│                               #   - CLI subcommand parsing (search, read, sync, ...)
│                               #   - --mcp mode: MCP-over-stdio, owns the request loop
│                               #   - config loading from --config or ~/.config/talon
│                               #   - daemon-like behavior (watcher + scheduler) when
│                               #     --mcp is run with watch=true and a vault configured
├── ts/
│   ├── package.json            # npm name: "@seanmozeik/talon"
│   ├── src/
│   │   ├── index.ts            # public API: mcpChildSpec, resolveBinary
│   │   ├── binary.ts           # platform-binary resolution
│   │   └── child.ts            # mcpChildSpec() → { command, args, env } for hosts to spawn
│   ├── npm/                    # subpackages: talon-darwin-arm64, talon-darwin-x64,
│   │   │                       #             talon-linux-x64, talon-linux-arm64
│   │   ├── darwin-arm64/
│   │   │   ├── package.json    # name: "talon-darwin-arm64", os: ["darwin"], cpu: ["arm64"]
│   │   │   └── bin/talon
│   │   └── ...
│   └── tsconfig.json
├── skill/
│   └── SKILL.md                # single source of truth; copied from current ultraclaw
├── docs/
│   ├── DESIGN.md               # this file's Rust-implementation companion (planned in-repo)
│   ├── CONFIG.md
│   └── PROTOCOL.md             # MCP wire protocol notes (just standard MCP), CLI exit codes
├── reference/                  # optional: the current TS implementation, frozen, as a porting reference
└── README.md
```

There is no separate `talon-mcp` crate. The MCP request handler is part of `talon-cli` (or a thin module in `talon-core` if useful for tests). MCP is not a "mode the binary can be reconfigured into" — it's just one of the dispatchers `talon-cli` provides, alongside CLI subcommand dispatch.

The `reference/` directory is a one-time snapshot of the current `edge/src/services/talon/` and `edge/src/mcp/tools/talon-*.ts` source, dropped in unmodified at extraction time, with a note that it is not built. Useful for porting; deleted once the Rust implementation reaches feature parity.

## 4. Binary surface

```
talon search <query> [--mode hybrid|semantic|fulltext|title] [--fast] [--limit N] ...
talon read <path> [--raw] [--from-line N] [--max-lines N]
talon sync [paths...] [--fast] [--force]
talon related <path> [--depth N] [--direction outgoing|backlinks|both]
talon status [--json]

talon --mcp                                  # MCP-over-stdio.
                                             # If config has vault_path + watch=true, the same
                                             # process also runs the watcher + embed scheduler
                                             # in-process (in additional tasks/threads, sharing
                                             # the same DB and watcher state).
                                             # If watch=false, --mcp is a stateless MCP server
                                             # that opens the DB on each call.

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

There is no `talon serve` subcommand. "Daemon" is not a separate public mode — it is a behavior `--mcp` exhibits when configured to watch a vault. Concretely: a `talon --mcp` process configured with watch will do all three jobs (MCP request handling, filesystem watching, scheduled embed runs). One-shot CLI invocations (`talon search`, `talon sync`, etc.) are normal CLI executions that read `~/.config/talon/config.toml` and operate on Talon's DB/index directly under the same file-lock rules.

This keeps the process model simple: MCP is for agents that speak MCP; CLI is for humans and shims that execute host commands. Shared DB mutation safety is enforced by Talon's lock file, not by routing CLI calls through MCP.

Behavioral guarantees inherited unchanged from current Talon:

- Only one `sync` runs at a time (host lock file). Watcher incremental batches defer if the lock is held.
- `--fast` on `search` means lexical-only (no expansion, no rerank). `--fast` on `sync` means lexical pass only (no embeddings). These are *different* semantics intentionally.
- Returned paths are vault-relative or container-absolute (`/opt/data/workspace/obsidian/...`). Host paths are never returned.
- Magic numbers stay constant: snippet 300, default limit 10, candidate pool `max(limit, 20)`, rerank cap 40, search cache 100, LLM cache 1000, chunk tokens 900, chunk overlap 15%, watcher debounce 60s, RRF k=60, strong-signal score/gap 0.85/0.15.

## 5. Configuration

### 5.1 Sources, in precedence order

1. **Explicit config file.** `--config <path>` or `TALON_CONFIG_FILE=<path>` for humans and standalone MCP hosts that want a non-default config.
2. **`~/.config/talon/config.toml`.** Default host config. Created by `talon init` if absent (writes a commented-out template).
3. **Built-in defaults.** Last resort for non-path knobs only; `vault_path` and `db_path` must be set before indexing/searching.

Ultraclaw does not inject, adapt, merge, or validate Talon config. It assumes the host has already configured Talon, same as other host tools such as bird/gog.

### 5.2 Schema

```toml
# ~/.config/talon/config.toml

vault_path        = "/Users/sean/Library/.../obsidian"
db_path           = "~/.local/share/talon/index.sqlite"
index_on_start    = true
watch             = true
embedding_schedule = ["03:00", "15:00"]
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
```

The TS wrapper does not expose a `TalonConfig` type. Configuration is a Talon-owned host file concern, not an ultraclaw/Bun API concern.

### 5.3 What is NOT in config

These are intentional non-knobs (carried forward from the current Talon design):

- All the magic numbers in §4.
- The DB schema version (managed internally; the binary handles migrations on open).
- The sync lock path (derived from `db_path`).
- The MCP tool name (`talon`, hardcoded — this is the product name, not a setting).

### 5.4 What disappeared from current config

`tools.obsidian.talon` disappears at cutover. Talon does not know about Obsidian-the-tool — it just needs a configured `vault_path`. Ultraclaw does not gate Talon on Obsidian config; it only starts the configured MCP child if the generic `mcp.children.talon.enabled` flag is true.

## 6. Inference abstraction

Talon's only external dependency at runtime is the inference endpoint. Current code couples directly to ultraclaw's `SidecarClient`. The standalone version expects any TEI-compatible HTTP endpoint:

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

The `expansion` endpoint is separate: an OpenAI-compatible chat completions endpoint (typically LM Studio at `localhost:1234/v1`). Used internally by hybrid search for query expansion. This is deliberately *not* the same as `inference` because expansion is a chat model, not an embedding model.

## 7. Daemon behavior of `--mcp`

When `talon --mcp` is invoked with `watch: true` and a `vault_path`, the process runs three concurrent things in-process (Rust async tasks or threads, sharing in-memory state and a single DB connection pool):

1. **MCP request handler.** Reads JSON-RPC frames from stdin, dispatches to handlers in `talon-core`, writes responses to stdout. Standard MCP server.
2. **File watcher.** `notify` on `vault_path`, 60s debounce, incremental indexing of changed notes. Updates the DB; the MCP handler sees fresh results immediately.
3. **Embed scheduler.** Fires at the configured `embedding_schedule` times. Holds the host sync lock for its duration. The MCP handler's `sync` action also acquires the same lock; only one mutator runs at a time.

When `talon --mcp` is invoked without `watch: true` (or without a vault path), it skips items 2 and 3 — it is just an MCP server backed by the configured DB/index.

CLI and MCP share the same underlying Rust handlers and DB lock semantics, but they do not communicate through each other.

## 8. TypeScript wrapper API

The npm `@seanmozeik/talon` package is a thin helper for hosts that want to mount the talon binary as a federated MCP child. It does not wrap the MCP protocol or speak JSON-RPC — that is the federation layer's job (in ultraclaw, `McpChildren`). Two consumers are envisioned:

1. Ultraclaw, which uses it to compute the spawn command and args for the federated child.
2. Anyone else who wants to spawn `talon --mcp` from Node/Bun: same helpers, no Rust dependency on the consumer side.

```ts
import {
  mcpChildSpec,
  resolveBinary,
} from '@seanmozeik/talon'

// Build a host-agnostic spawn spec for the federation layer to use:
const spec = mcpChildSpec()
// returns: { command, args: ['--mcp'], env: {} }

// resolveBinary() returns the absolute path to the platform-matched talon binary,
// resolved through the optionalDependencies mechanism. Useful for shim routes
// that spawn the binary directly without going through MCP.
const binary = resolveBinary()

// Agent skill text is owned by the Rust binary:
//   talon --skill
```

The wrapper has no `Talon` class, no `.search()` / `.sync()` / `.mcp()` methods. Reasons:

- Standalone hosts that don't want to deal with MCP can call the binary directly via `resolveBinary()` and `child_process.spawn`. They don't need a wrapper for that.
- Ultraclaw doesn't need an in-process `Talon.callTool()` either — the federation layer speaks JSON-RPC to the child, generically, the same way it does to any other federated MCP server.
- Adding wrapping methods would force the wrapper to maintain a parallel surface to the binary's MCP, recreating exactly the duplication problem we rejected when choosing federation.

There is no in-process Rust binding (no `napi-rs`): the binary always runs as a child process. Reasons:

- Identical behavior under Bun and Node.
- No native ABI churn during Bun upgrades.
- The startup cost is tolerable: the Rust binary cold-starts in ~30ms, and ultraclaw keeps the federated child long-lived.

## 9. Distribution

One public distribution channel:

| Channel | Audience                          | Mechanism                                                        |
|---------|-----------------------------------|------------------------------------------------------------------|
| npm     | Node/Bun consumers (incl. ultraclaw) | `npm i @seanmozeik/talon` pulls the wrapper + platform binary. |

The npm pattern is exactly how `esbuild`, `biome`, and `oxc` distribute: the main package has `optionalDependencies` for each platform, only the matching one installs, and `binary.ts` resolves the path via `require.resolve('@seanmozeik/talon-darwin-arm64/bin/talon')`. See `npm/` subpackages in §3.

GitHub Actions builds release binaries for the target triples and publishes the npm package/subpackages. Cargo and Homebrew can still be used locally during development, but they are not product distribution channels.

## 10. What to copy from the current TS implementation

Aside from being kept under `reference/`, the following pieces transfer almost verbatim as design (not code):

| Piece                                | Notes                                                                     |
|--------------------------------------|---------------------------------------------------------------------------|
| `SKILL.md`                           | Copy unchanged. This is the user contract.                                |
| Input schema (the `TalonInput` discriminated union by `action`) | Same shape, re-derived in Rust serde. |
| Output schema (`TalonResponse`)      | Same shape.                                                               |
| Magic numbers (§4)                   | Hardcoded constants in `talon-core`.                                      |
| BM25 / RRF / hybrid-blend formulas   | The current TS code has these; they are the OHS-derived formulas.         |
| Chunker (900 tokens, 15% overlap, frontmatter handling, wikilink awareness) | Copy the algorithm; many tests transfer. |
| Watcher debounce + scan rules        | Copy the include/ignore semantics.                                        |
| Sync lock file format                | Same lockfile semantics; serializes sync/scheduled/startup runs.          |
| sqlite schema + migrations           | Copy migrations as Rust string constants where useful, but the cutover does not require opening the legacy ultraclaw DB. A clean rebuild is acceptable. |

Things explicitly NOT carried over:

- `DeepResearchSupervisor` plumbing (dead wiring; never called).
- The Mac Homebrew SQLite quirk (`Database.setCustomSQLite`) — the Rust binary statically links sqlite + sqlite-vec, so the host's sqlite is irrelevant.
- The Effect-specific service/layer composition. Replaced by Rust's normal modular structure (planned in-repo).
- Any direct dependency on `EdgeConfig`, `SidecarClient`, ultraclaw error types, or ultraclaw path helpers.

# Part 2 — Stripping ultraclaw

This is the part with the most decisions, because it's where the abstract "Talon is its own project" meets the concrete reality of a working ultraclaw.

## 11. What gets deleted

```
edge/src/services/talon/              # entire directory, ~107 files, ~10,611 LOC
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
edge/src/mcp/tools/talon.ts           # the plugin file too — federation handles it
                                      # ~920 LOC total, no replacement plugin file
```

There is no `mcp/tools/talon.ts` after the change. Talon's tool definition (description, schemas) comes from the talon child's `tools/list` response, populated into ultraclaw's tool registry by the generic federation layer (§12.2).

In-tree tests under `edge/src/tests/talon-*.test.ts` (or wherever they live after the REFACTOR.md restructure) move to the new project, ported to Rust integration tests.

Files that *reference* talon and need edits — not deletions — are listed in §13.

## 12. What gets added

### 12.1 `edge/package.json`

```jsonc
{
  "dependencies": {
    "@seanmozeik/talon": "^x.y.z"
  }
}
```

That's it. The npm `talon` package brings the wrapper helpers + the platform binary.

### 12.2 `edge/src/mcp/children/` — generic MCP federation layer

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
4. Register all proxy plugins into the existing `ToolRegistry`. From the perspective of the rest of the MCP server, they are indistinguishable from in-process plugins.

**Crash handling.** If a child's stdio pipe closes or a heartbeat times out, drop its tools from the registry, attempt restart with exponential backoff (default `restartBackoffMs=1000`, doubling, capped). After `maxRestarts` consecutive failures within a window, log loudly and stop trying until config changes.

**Tool name collisions.** No special machinery. Talon exports `talon`; there should not be a collision. If an operator configures one anyway, the existing registry uniqueness behavior is enough.

**Timeouts and cancellation.** Each `tools/call` request has a per-child default timeout (60s, configurable per child). On timeout, send `notifications/cancelled` to the child with the request id and surface a timeout error to the agent.

**Notifications.** `notifications/tools/list_changed` from a child triggers a re-fetch of `tools/list` and a registry update. No progress-notification bridge is required for Talon parity with the current TypeScript tool.

### 12.3 `EdgeConfig.mcp.children` config block

```ts
// edge/src/config/schema.ts — new section
mcp: {
  children: {
    talon: {
      enabled: true,
      command: '<resolved at runtime via talon.resolveBinary()>',
      args: ['--mcp'],
      restartOnCrash: true,
      maxRestarts: 5,
      restartBackoffMs: 1000,
      requestTimeoutMs: 60_000,
    },
    // future: rust-analyzer, fs-mcp, etc.
  },
}
```

There is no `configRef` or talon-specific adapter. `McpChildren` only knows how to spawn configured children and speak MCP. Talon reads `~/.config/talon/config.toml` on the host.

## 13. Surgical edits to non-deleted files

`grep` shows ~21 ultraclaw files reference `services/talon/...` directly. After the change, almost all of them just *delete* the Talon import — the service that used to be there is replaced by a federated MCP child, which the rest of edge does not see.

### 13.1 `edge/src/app/server/server.ts` and `runner-layers-core.ts`

Delete `TalonCli`, `TalonStore`, `TalonIndexer`, `TalonWatcher`, `TalonEmbeddingScheduler`, `TalonHostRuntime` from the layer graph. Add `McpChildren` to the layer graph. There is no talon-specific service entry — talon participates as an entry in `config.mcp.children`.

### 13.2 `edge/src/app/cli/lifecycle-service.ts`, `lifecycle-daemon.ts`, `runner.ts`

Currently start/stop `TalonHostRuntime` alongside other services. Replace with `McpChildren.start()` / `.stop()`. The federation layer's start spawns and handshakes all enabled children (including talon); its stop terminates them.

### 13.3 `edge/src/app/cli/doctor.ts`, `doctor-probes.ts`

Delete Talon-specific doctor probes. Talon readiness is available through normal Talon surfaces (`talon status` for CLI users and the `status` action for MCP users). Ultraclaw does not need a special federated-child doctor model for Talon.

### 13.4 `edge/src/app/server/shim-routes.ts`

Currently `talonShim` invokes the in-process `TalonCli` Effect service:

```ts
deps.talon.run(input)  // current
```

After the change, `talonShim` becomes a normal "spawn a host CLI" handler — same shape as `gogShim`, `birdShim`, etc. — pointing at the resolved `talon` binary path:

```ts
import { resolveBinary } from '@seanmozeik/talon'
const binary = resolveBinary()
ctx.spawn(binary, args, { stdin })            // existing streaming-shim machinery
```

The streaming shim infra (POST /shim/talon, NDJSON downstream, SIGINT/SIGWINCH forwarding) is unchanged. Talon shifts from "in-process Effect call" to "spawn host CLI" — joining the rest of the unguarded shims, which is conceptually cleaner anyway. PTY mode for `talon` becomes available for free since it's now a real subprocess.

When invoked from a shim, edge spawns the host `talon` binary exactly like gog/remi/bird-style host shims. This path has nothing to do with MCP.

### 13.5 `edge/src/mcp/tools/plugin.ts`

The `ToolDeps` and `ToolContext` types currently include Talon-specific deps (`hostTalonStore`, the various Talon services). Strip those out. No replacements added — federated proxy plugins don't need ultraclaw-specific deps; they have their child handle and that's all.

### 13.6 `edge/src/config/schema.ts`

Delete `TalonSection` and any Talon-specific validation (`validateModelReferences` entries, expansion model references, include/ignore pattern defaults, etc.). Talon config is no longer part of ultraclaw config.

A new `mcp.children` section is added (see §12.3). `talon` is registered there. This is the only place ultraclaw's config mentions talon.

### 13.7 `edge/src/clients/...`

No changes. The `SidecarClient` continues to be used by other parts of edge (prompt-guard, transcribe, etc.). Talon-specific use sites are gone, but the client is general-purpose.

### 13.8 `.agents/skills/talon/SKILL.md`

No action. Talon's Rust binary embeds `skill/SKILL.md` with `include_str!` and prints it via `talon --skill`, matching `ddg`. Ultraclaw users are responsible for ensuring the skill exists in ultraclaw if they want it; the container already has the current skill.

### 13.9 Container Dockerfile

No changes initially. The container `talon` shim continues to be a symlink to `_ultraclaw_shim`, which routes through edge. The `talon` binary lives only on the host.

Future option (not part of this change): install the `talon` binary inside the container too, so in-container invocations skip the edge round-trip. Defer until there's a reason — the streaming shim already handles the latency well, and putting another binary in the container increases image size without solving a real problem.

## 14. MCP merging — generic federation

The chosen architecture: **ultraclaw's MCP server federates arbitrary stdio MCP servers.** Talon is one configured child. Standalone hosts (Claude Code, Cursor) point at `talon --mcp` directly with the same protocol. Talon does not have, and does not need, any ultraclaw-specific protocol.

### 14.1 Why federation, not plugin embed

A "second flag" approach (talon exposing a non-MCP bridge protocol just for ultraclaw) and a plugin-embed approach (ultraclaw bundles talon's tool definition statically and proxies execute calls without doing federation) were both rejected. Both create a parallel surface that has to be maintained alongside MCP, and both make talon care about ultraclaw. Federation factors the problem correctly: talon speaks one protocol (MCP), ultraclaw learns to mount any MCP server, talon happens to be the first one we mount.

The federation work is bounded (~400 LOC of focused JSON-RPC plumbing and lifecycle bookkeeping; see §12.2) and it's reusable for any future stdio MCP server.

### 14.2 Request flow

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

### 14.3 Standalone use

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

Standalone hosts get exactly the same tools, schemas, and behavior as ultraclaw — there is no second mode to maintain.

### 14.4 Concurrency

The Talon MCP server should expose the same tool behavior as the current TypeScript implementation: the `talon` tool with the same action schema and response shape. Parallelism can be internal to the Rust server; there is no separate product mode to design here.

## 15. Migration plan

Phased so the Talon repo can mature independently, but the ultraclaw cutover itself is clean. There is no long-running dual backend and no legacy compatibility layer.

### Phase 0 — Extract the reference

Snapshot the current TS implementation into the new repo's `reference/`. Confirms the new repo structure works; gives the Rust port a known-good source to consult. Ultraclaw is unchanged. 1 day.

### Phase 1 — Build the Rust binary

Implement `talon-core`, `talon-cli`, `talon-mcp` in the new repo. Reach feature parity with the TS implementation. Ship `talon` binaries through npm package `@seanmozeik/talon`. Ultraclaw is still unchanged, still using the TS implementation. This is the bulk of the work; planned in the new repo, not here.

Acceptance: a fresh checkout of the new repo passes a parity test that runs the same set of inputs through both the binary and the legacy TS implementation, asserting result equivalence on a fixture vault. The test runs in CI for the new repo and is the gate for cutover.

### Phase 2a — Build the generic MCP federation layer

Build `edge/src/mcp/children/` (§12.2) without any talon involvement. Test against a trivial fixture child MCP server (a 50-LOC test server that exposes `echo` and `slow_echo` tools). Cover: handshake, tools/list registration, tools/call round-trip, crash + restart, and timeout/cancel.

Independent of talon. Shippable on its own. The fixture child stays in the test suite as a regression target for federation behavior.

Acceptance: federation tests pass. `config.mcp.children` accepts entries; non-talon children can be configured ad-hoc for testing.

### Phase 2b — Clean Talon Cutover

Add `talon` to `edge/package.json`. Add the `talon` entry to `config.mcp.children`, enabled by the generic child config only. Delete the TS implementation in the same change:

- `rm -rf edge/src/services/talon/`
- Delete all `edge/src/mcp/tools/talon-*.ts` files (no new plugin file replaces them).
- Delete `EdgeConfig.talon`, `tools.obsidian.talon`, and all Talon-specific config validation.
- Drop tests that no longer apply; the Rust binary's own test suite is authoritative for Talon behavior.

Acceptance: Talon is configured in `~/.config/talon/config.toml` on the host, ultraclaw starts it only as a generic MCP child, the federated `talon` tool works through `tools/list`/`tools/call`, the container shim spawns the host binary, and the ultraclaw repo no longer contains `services/talon/` or Talon config schema code outside of git history.

### Phase 3 — Reclaim wins

Now that Talon is out of `edge/`, reconsider the REFACTOR.md restructure. Some of its goals (collapsing `cli/` into `services/talon/cli/`, splitting `services/talon/shared/` from `talon/`) become moot. Update REFACTOR.md to reflect the post-extraction reality.

## 16. Risks and how we mitigate them

| Risk                                                    | Mitigation                                                                                |
|---------------------------------------------------------|--------------------------------------------------------------------------------------------|
| Rust port has subtle behavior differences vs. TS        | Phase 1 parity test on a fixture vault. Cut over only after the Rust repo is good enough; no dual-backend legacy layer. |
| Existing ultraclaw Talon DB is not reusable by Rust     | Accepted. This is a clean cutover; Talon can rebuild its own index from the configured vault. |
| `notify` behaves differently from `chokidar` on iCloud-backed dirs | Smoke-test on the actual dev vault during Phase 1; this is the canary.                     |
| Spawn cost on MCP cold start hurts UX                   | Long-lived MCP child (§14.3). Measure; revisit `napi-rs` only if measurements justify it.  |
| Existing ultraclaw Talon config users must migrate      | Accepted. Talon setup moves to `~/.config/talon/config.toml`; document the one-time migration. |
| Talon project becomes a maintenance burden separate from ultraclaw | Accepted. The standalone-product framing is the explicit goal; the cost is owning a second release cadence. |

## 17. Out of scope for this spec

- Concrete Rust crate choices (`rusqlite` vs `sqlx`, async runtime, JSON-RPC library, etc.). Planned in the new repo.
- The Rust implementation of any specific module (chunker, BM25, RRF). Planned in the new repo.
- Container-side `talon` binary install.
- `napi-rs` bindings.
- Legacy compatibility for `tools.obsidian.talon` or `EdgeConfig.talon`. Both are deleted at cutover.
- Touching the Hermes side of the world. Hermes-in-container does not interact with Talon directly today; that doesn't change.
- A dashboard / config UI for `mcp.children`. Hand-edited config only.
- Forwarding `notifications/progress` from federated children to ultraclaw's MCP clients. Not required for Talon parity.

In scope (and explicitly *not* rejected anymore):

- **Generic MCP federation in ultraclaw.** This is the core architectural addition; it is what makes the talon extraction clean, and it pays for itself the first time a second federated server lands.

## 18. Resolved Decisions

1. **Host Talon config setup.** Talon reads `~/.config/talon/config.toml` by default. `talon init` creates `~/.config/talon/` and `config.toml` if missing, writes a clear TOML template, and does not overwrite an existing file.

2. **Skill distribution.** Talon embeds `skill/SKILL.md` in the Rust binary and exposes it with `talon --skill`, same as `ddg`. Ultraclaw does not regenerate or verify this file. Users are responsible for ensuring the skill exists in ultraclaw if they want it; the container already has the current skill.

3. **Standalone embedder docs.** The README explains that `inference.base_url` should point at a TEI-compatible endpoint such as text-embeddings-inference, Infinity, or ultraclaw's sidecar.

4. **Distribution name.** npm package name is `@seanmozeik/talon`. npm is the product distribution channel. No crates.io or Homebrew release is required.

5. **Config injection.** There is none for Talon. Talon reads host config from `~/.config/talon/config.toml` by default, with optional `--config <path>` / `TALON_CONFIG_FILE=<path>` for standalone override. Ultraclaw does not know or care about Talon's config shape.

6. **Tool-name collisions.** There should not be collisions. Talon exports `talon`. No auto-prefixing or special collision policy is needed beyond the existing registry uniqueness behavior.

7. **MCP capability and parity.** Talon MCP must expose the same tool behavior as the current TypeScript implementation: same tool name, same action schema, and same response shape. Do not defer pieces of the existing TS tool surface.

8. **Doctor probes.** No special federated-child doctor model for Talon. Use normal Talon surfaces: `talon status` for CLI users and the `status` action for MCP users.

9. **Schema handling.** Federated MCP children provide JSON Schema in `tools/list`; ultraclaw proxy plugins should carry that JSON Schema directly. Do not translate JSON Schema into Effect Schema for federated children, and do not add Talon-specific Effect Schema exports to the npm package.

10. **Container shim.** The container `talon` command routes through edge's normal host CLI shim, like gog/remi/bird-style shims. Edge spawns the host `talon` binary. This has nothing to do with MCP or daemon sockets.
