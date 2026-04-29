# Talon MCP Memory Overhaul Plan

## 1. Overview

Talon's normal CLI remains a stateless command surface. `talon mcp` is the exception: it is still part of the single `talon-cli` product and still runs as a stdio MCP server, but while it is alive it may hold process state, watch the vault, refresh the index, run background embedding work, maintain turn-aware recall state, and serve MCP tools over one long-lived JSON-RPC connection.

This plan covers two connected workstreams:

1. Overhaul the MCP server from a stateless action-union wrapper into a long-running vault context server.
2. Add Claude Code and Hermes integrations that use the running MCP server for automatic memory recall hooks without asking the model to call recall manually.

Codex is explicitly deferred. Codex can continue using the public MCP tools later, but this plan does not depend on a Codex hook mechanism.

## 2. Product Position

Talon is not a general memory database and should not become one by accident. The vault remains the source of truth. Talon indexes, retrieves, cites, and injects compact context from vault material. Talon must not store agent-derived inferences as facts unless a user or agent explicitly writes to the vault through normal user-controlled workflows outside this MCP memory layer.

The practical split is:

| Surface | Process model | Purpose |
| --- | --- | --- |
| `talon search/read/related/recall/...` | One-shot, stateless | Manual CLI use, tests, scripts, fallback paths |
| `talon mcp` | Long-running stdio process | Agent tool access, file watching, background sync/embed, turn-aware recall hooks |
| Claude Code plugin | Host lifecycle hooks + MCP config | Auto-inject recall before each prompt; notify Talon after turns |
| Hermes provider/plugin | Provider lifecycle wrapper + MCP client | Auto-inject recall before each model call; notify Talon after turns |

## 3. Goals

- Keep `talon mcp` as a stdio MCP server invoked by the existing `talon mcp` subcommand.
- Do not add a separate MCP crate or daemon binary.
- Replace the broad single `talon` action-union MCP tool with a narrow named tool surface.
- Make MCP output use Talon's existing compact agent JSON shape.
- Move automatic recall into hook-only MCP tools that are called by host/plugin lifecycle code, not by the model as ordinary tools.
- Make automatic recall turn-aware so repeated adjacent turns do not spam the context window with the same notes.
- Add long-running MCP process state for sessions, background file refresh, background embeddings, and diagnostics.
- Support Claude Code and Hermes first.
- Keep explicit agent tools small: search, read, related.
- Keep sync, status, meta, changes, and lint on the CLI surface unless a concrete MCP need appears later.

## 4. Non-Goals

- No standalone `talon-mcp` crate.
- No non-stdio MCP transport in the first implementation.
- No Codex hook integration in this phase.
- No automatic writes to the vault.
- No agent-inferred memory retention.
- No public MCP `sync`, `meta`, `changes`, `lint`, or `status` tools.
- No large general-purpose memory tool set.
- No linter config changes.

## 5. External Patterns

The design should follow the shape used by successful Claude Code memory systems:

- Hindsight's Claude Code plugin uses lifecycle hooks: `SessionStart` for health checks, `UserPromptSubmit` for recall injection, `Stop` for retain/background work, and `SessionEnd` for cleanup. See: https://hindsight.vectorize.io/sdks/integrations/claude-code
- Hindsight's OpenClaw integration documents the important product lesson: automatic recall should happen before each turn because models do not reliably remember to call a search-memory tool. See: https://hindsight.vectorize.io/sdks/integrations/openclaw
- Mem0's Claude Code integration separates "MCP only" from "plugin with lifecycle hooks"; MCP-only exposes tools, while the plugin adds automatic capture/retrieval behavior. See: https://docs.mem0.ai/integrations/claude-code
- Claude Code hooks can inject context with `additionalContext`, and MCP tools can be called from hooks when the MCP server is already connected. See: https://code.claude.com/docs/en/hooks

Talon should adapt these patterns without copying the memory-retention model. Talon recalls from the vault and records session-local injection history; it does not extract facts from conversations and store them as durable memory.

## 6. Current Implementation Baseline

Current MCP behavior:

- `talon mcp` already exists as a dedicated CLI subcommand.
- MCP currently exposes one tool named `talon`.
- That tool accepts an `action` union for `search`, `read`, `sync`, `status`, `related`, `meta`, `changes`, `lint`, and `recall`.
- The current tool description says it runs a stateless Talon action.
- MCP results include the full `TalonEnvelope` in `structuredContent` and serialized text content.

Current agent output behavior:

- Talon already has compact agent JSON serializers under `crates/talon-cli/src/output/json/agent/`.
- These serializers currently write to stdout. They should be refactored into reusable value builders so MCP can return the same compact shapes without inventing a new format.

Current recall behavior:

- `recall` computes relevant active notes and linked context.
- It supports budget, scope filters, exclusions, prompt-XML, and JSON output.
- It is currently request-local. It does not know what it injected on previous turns.

## 7. Target MCP Tool Surface

### 7.1 Public Agent Tools

These are exposed for normal model-initiated tool use.

#### `talon_search`

Use for broad lookup over the vault when automatic recall is insufficient.

Input:

```json
{
  "query": "string",
  "scope": ["string"],
  "scopeOnly": ["string"],
  "scopeAll": false,
  "mode": "hybrid|semantic|fulltext|title",
  "fast": false,
  "limit": 10,
  "candidateLimit": 40,
  "where": [],
  "includeSnippets": true,
  "anchors": false
}
```

Output: existing compact agent search JSON.

#### `talon_read`

Use after search when the agent needs source text, exact wording, or a section body.

Input:

```json
{
  "path": "vault/path.md or [[Obsidian Ref]]",
  "raw": false,
  "fromLine": null,
  "maxLines": null
}
```

Output: existing compact agent read JSON.

#### `talon_related`

Use for deliberate graph traversal from a known note.

Input:

```json
{
  "path": "vault/path.md",
  "direction": "outgoing|backlinks|both",
  "depth": 1,
  "limit": 10
}
```

Output: existing compact agent related JSON.

### 7.2 Hook-Only Tools

These are MCP tools because Claude Code hooks need to call MCP tools over the existing connection. They are not intended for model-initiated use. Their descriptions must say "hook-only" plainly.

#### `talon_hook_recall`

Called before a model turn. Returns host-specific hook output that injects recall context.

Input:

```json
{
  "host": "claude-code|hermes",
  "sessionId": "string",
  "turnId": "string",
  "cwd": "string",
  "transcriptPath": "string",
  "message": "string",
  "priorMessages": ["string"],
  "budgetTokens": 500,
  "maxSyncMs": 1500,
  "format": "hook-json|agent-json|prompt-xml",
  "scope": ["string"],
  "scopeOnly": ["string"],
  "scopeAll": false
}
```

For Claude Code, output text should be JSON in Claude hook format:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "UserPromptSubmit",
    "additionalContext": "<vault_recall>...</vault_recall>"
  }
}
```

If recall is skipped, return either no additional context or a tiny diagnostic only when debug is enabled. Skipped recall is normal behavior, not an error.

For Hermes, output should be compact agent JSON or prompt-XML according to the provider's expected injection format. Hermes should not parse full `TalonEnvelope` internals.

#### `talon_hook_turn_end`

Called after a turn completes. Updates Talon's session ledger and may schedule background work. It should not write agent-derived facts to the vault.

Input:

```json
{
  "host": "claude-code|hermes",
  "sessionId": "string",
  "turnId": "string",
  "cwd": "string",
  "transcriptPath": "string",
  "lastUserMessage": "string",
  "lastAssistantMessage": "string",
  "toolCalls": [],
  "outcome": "completed|failed|cancelled"
}
```

Output:

```json
{
  "ok": true,
  "queuedPrefetch": true,
  "sessionStats": {
    "turns": 12,
    "lastRecallMs": 74,
    "injectedChunks": 18,
    "suppressedChunks": 9
  }
}
```

#### `talon_hook_session_start`

Optional for Claude Code and Hermes. Used for warmup and health checks.

Input:

```json
{
  "host": "claude-code|hermes",
  "sessionId": "string",
  "cwd": "string",
  "transcriptPath": "string"
}
```

Behavior:

- Load config.
- Start watcher if not started.
- Open DB or validate DB path.
- Begin or resume session ledger.
- Kick a low-priority refresh if the index is stale.

#### `talon_hook_session_end`

Optional cleanup hook.

Behavior:

- Mark session ended.
- Keep bounded ledger in memory until TTL expires.
- Do not stop the MCP process unless the host itself exits.

## 8. Agent Contract Source of Truth

Today `skill/SKILL.md` is a hand-written agent-facing contract. MCP tool descriptions are separate and can drift.

Target:

- Add an `agent_contract` module in `talon-cli`.
- Define Rust constants or structured records for:
  - search guidance,
  - read guidance,
  - related guidance,
  - recall hook guidance,
  - result contract,
  - safety and scope notes.
- Generate `--skill` output from the same source.
- Generate MCP tool descriptions from the same source.
- Keep Markdown formatting for the skill, but concise one-paragraph descriptions for MCP tool schemas.

Suggested shape:

```rust
pub struct AgentToolContract {
    pub name: &'static str,
    pub description: &'static str,
    pub when_to_use: &'static str,
    pub when_not_to_use: &'static str,
}

pub const SEARCH: AgentToolContract = AgentToolContract { ... };
pub const READ: AgentToolContract = AgentToolContract { ... };
pub const RELATED: AgentToolContract = AgentToolContract { ... };
```

`skill/SKILL.md` can stay checked in at first if generated output is disruptive, but tests should assert that important snippets in the skill and MCP descriptions come from shared constants. Later, generate it in `build.rs` or at runtime.

## 9. MCP Server State

Add a process-local state object owned by `talon mcp`.

```rust
pub struct McpServerState {
    config: Arc<ConfigState>,
    clients: Arc<ClientState>,
    sessions: Arc<SessionStore>,
    background: BackgroundRuntime,
    watcher: VaultWatcher,
    diagnostics: Arc<DiagnosticsState>,
}
```

### 9.1 Config State

Holds:

- loaded Talon config,
- config path,
- vault path,
- DB path,
- scope defaults,
- search/rerank/cache settings,
- last config reload time.

Config should be loaded once at startup and reloadable when config files change, but first implementation can reload on explicit background refresh or session start.

### 9.2 Client State

Holds reusable:

- inference client,
- expansion client,
- rerank cache capacity setup,
- embedding client config.

Failures should degrade gracefully. If inference sidecars are unavailable, search and recall fall back to fast lexical behavior and report warnings in diagnostics.

### 9.3 Session Store

Process-local store keyed by host plus session ID.

```rust
pub struct SessionKey {
    host: HostKind,
    session_id: String,
}

pub struct SessionState {
    created_at_ms: i64,
    last_seen_at_ms: i64,
    turns: VecDeque<TurnRecord>,
    injected_chunks: LruByChunkId,
    injected_notes: LruByPath,
    suppressed: VecDeque<SuppressedRecall>,
    queued_prefetch: Option<PrefetchRecord>,
    last_recall: Option<RecallTelemetry>,
}
```

The session store is in memory for the first release. Optional bounded persistence can come later.

### 9.4 Diagnostics State

Track:

- watcher running or stopped,
- last refresh time,
- last refresh error,
- embedding queue size,
- last embedding error,
- per-session recall latency,
- prefetch hit/miss counts,
- count of suppressed duplicate chunks.

No public `talon_status` MCP tool in phase 1. Diagnostics can be emitted in debug hook responses and logs. If a status tool becomes necessary, add it later as a narrow hook/debug-only tool.

## 10. Background Work

### 10.1 Watcher

`talon mcp` should watch the configured vault path.

Behavior:

- Debounce filesystem events using the existing watcher debounce constant.
- Queue a refresh job after debounced changes.
- Coalesce multiple changes into one refresh.
- Do not run refresh while a full sync/embed pass is active.
- If refresh lock is busy, skip and retry after a short backoff.

First implementation can depend on `notify` if not already present. Add dependency through `cargo add`, not manual `Cargo.toml` editing.

### 10.2 Refresh

Refresh updates the SQLite index so recent file edits become searchable. It should mirror existing no-embed auto-refresh behavior.

Rules:

- Keep writes serialized by the existing sync lock.
- Keep search/read requests non-blocking when possible.
- If a hook call arrives while refresh is running, prefer stale-but-fast recall over blocking beyond `maxSyncMs`.
- Record the index generation or refresh timestamp so recall results can explain freshness if needed.

### 10.3 Embedding

Background embedding should run after refresh, not on every hook call.

Rules:

- Embedding jobs are low priority.
- Only embed pending or changed chunks unless forced by CLI.
- Full force embedding remains a CLI operation.
- Hook recall must not block on embedding completion.
- If semantic search is unavailable, fall back to BM25/title and record a warning.

## 11. Turn-Aware Recall

The core anti-spam requirement is that automatic recall should inject useful new context, not the same note snippets every turn.

### 11.1 Recall Modes

Add mode semantics internally even if the public CLI flag comes later:

| Mode | Caller | Default budget | Purpose |
| --- | --- | ---: | --- |
| `auto` | hook recall | 500 to 800 tokens | Small, high-confidence context before every turn |
| `explicit` | CLI or future explicit recall | 2000+ tokens | User/model deliberately asks for broader context |
| `tool` | search/read/related | tool-specific | Agent-driven exploration |

`talon_hook_recall` always uses `auto` unless input explicitly overrides for debugging.

### 11.2 Turn Record

Each hook recall and turn end updates a bounded ledger.

```rust
pub struct TurnRecord {
    turn_id: String,
    query_fingerprint: String,
    user_message_hash: String,
    injected: Vec<InjectedChunk>,
    suppressed: Vec<SuppressedRecall>,
    recall_started_at_ms: i64,
    recall_duration_ms: u64,
    skipped: bool,
}

pub struct InjectedChunk {
    chunk_id: String,
    note_id: String,
    path: String,
    heading: Option<String>,
    score: f64,
    reason: InjectionReason,
}

pub struct SuppressedRecall {
    chunk_id: Option<String>,
    note_id: Option<String>,
    path: String,
    score: f64,
    reason: SuppressionReason,
}
```

Stable chunk IDs are preferred. Until stable chunk IDs exist in the recall response, use a deterministic fallback:

```text
hash(vault_path + heading + snippet_start_line_or_rank + normalized_snippet_prefix)
```

This fallback should be clearly marked as temporary in code.

### 11.3 Query Fingerprint

Build a query fingerprint from:

- normalized current user message,
- selected prior message summaries or hashes,
- scope set,
- mode,
- host kind.

Use the fingerprint to detect repeated or near-repeated turns.

Rules:

- Exact same fingerprint within a short TTL should usually return no new recall unless there was a vault/index change.
- Similar query with same top candidates should suppress repeated chunks and include only novel context.
- Query drift above threshold can allow previously injected notes to reappear.

First implementation can use simple normalized text similarity:

- lowercase,
- trim whitespace,
- drop punctuation,
- token set Jaccard similarity,
- optional query hash for exact matches.

Do not introduce a heavy new similarity model for this.

### 11.4 Suppression Policy

Suppress repeated context at chunk and note levels.

Default policy:

- Same chunk injected in last 3 turns: suppress.
- Same note injected in last 2 turns: suppress unless score is much higher or query drift is high.
- Same chunk injected earlier in session: allow only if current query has high drift and high score.
- Linked context repeated from last turn: suppress aggressively.
- Explicit `talon_read` and `talon_search` are never suppressed. Suppression applies only to automatic recall injection.

Suggested reasons:

```rust
pub enum SuppressionReason {
    SameChunkRecentlyInjected,
    SameNoteRecentlyInjected,
    RepeatedLinkedContext,
    QueryRepeated,
    BelowConfidenceGate,
    BudgetTrimmed,
}
```

### 11.5 Novelty-Aware Selection

Pipeline:

1. Run normal recall retrieval.
2. Convert active notes and linked context into candidate chunks.
3. Annotate each candidate with previous injection state.
4. Apply confidence gate.
5. Apply suppression policy.
6. Select highest-scoring novel candidates under budget.
7. If all candidates are suppressed, return no `additionalContext` or a tiny pointer only in debug mode.

Important behavior:

- Do not inject stale repeated content just because nothing novel was found.
- False negatives are acceptable. The model can call `talon_search` or `talon_read`.
- False positives are expensive because they poison every turn.

### 11.6 Prefetch

After `talon_hook_turn_end`, Talon can queue a prefetch for the likely next turn using:

- last user message,
- last assistant message,
- current session ledger,
- changed files since last turn.

On the next `talon_hook_recall`:

- If queued prefetch fingerprint matches or is close enough, use it.
- If stale but still useful, apply suppression and return if under max age.
- If stale or mismatched, discard and run bounded synchronous recall.

Defaults:

```text
prefetch enabled: true
prefetch max age: 30000 ms
hook max sync: 1500 ms
auto budget: 500 tokens
confidence threshold: 0.4
session ledger max turns: 32
session idle ttl: 6 hours
```

## 12. Claude Code Plugin Plan

### 12.1 Plugin Contents

Package a Claude Code plugin that includes:

- `.mcp.json` or plugin MCP configuration for `talon`.
- `hooks/hooks.json` defining lifecycle hooks.
- optional scripts only if direct MCP hook handlers cannot express all required input mapping.
- optional skill/instructions telling Claude when to use public `talon_search`, `talon_read`, and `talon_related`.

MCP config:

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

Public MCP tools visible to Claude:

- `mcp__talon__talon_search`
- `mcp__talon__talon_read`
- `mcp__talon__talon_related`
- hook-only tools may also appear, but their descriptions must clearly discourage model use.

### 12.2 Hooks

Use Claude Code lifecycle events:

| Event | Tool | Purpose |
| --- | --- | --- |
| `SessionStart` | `talon_hook_session_start` | Warm config, watcher, and session state |
| `UserPromptSubmit` | `talon_hook_recall` | Inject relevant vault context through `additionalContext` |
| `Stop` | `talon_hook_turn_end` | Update ledger and queue prefetch |
| `SessionEnd` | `talon_hook_session_end` | Mark session idle |

Claude Code hook example:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "mcp_tool",
            "server": "talon",
            "tool": "talon_hook_recall",
            "input": {
              "host": "claude-code",
              "sessionId": "${session_id}",
              "turnId": "${session_id}:${transcript_path}:${prompt_hash}",
              "cwd": "${cwd}",
              "transcriptPath": "${transcript_path}",
              "message": "${prompt}",
              "budgetTokens": 500,
              "format": "hook-json"
            }
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "mcp_tool",
            "server": "talon",
            "tool": "talon_hook_turn_end",
            "input": {
              "host": "claude-code",
              "sessionId": "${session_id}",
              "turnId": "${session_id}:${transcript_path}:stop",
              "cwd": "${cwd}",
              "transcriptPath": "${transcript_path}",
              "outcome": "completed"
            }
          }
        ]
      }
    ]
  }
}
```

The exact variable interpolation syntax must be verified against Claude Code's current hook runtime. If MCP hook input cannot interpolate every field directly, use a tiny command hook script that reads Claude's hook JSON from stdin and calls a hook-only MCP tool through an approved local helper.

### 12.3 Claude Output Contract

`talon_hook_recall` for Claude must emit:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "UserPromptSubmit",
    "additionalContext": "..."
  }
}
```

The `additionalContext` content should be concise:

```xml
<vault_recall source="talon" mode="auto" session="..." skipped="false">
  <note path="projects/Foo.md" mtime="2026-04-28" score="0.82">
    ...
  </note>
</vault_recall>
```

When skipped:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "UserPromptSubmit"
  }
}
```

No context is better than low-confidence repeated context.

## 13. Hermes Provider Plan

Hermes does not need Claude Code hook files. Its provider/plugin becomes the lifecycle bridge.

### 13.1 Process Ownership

The Hermes Talon provider should:

1. Start `talon mcp` as a stdio child process when the provider starts.
2. Send MCP `initialize`.
3. Keep the JSON-RPC connection open for the provider lifetime.
4. Call hook-only tools before and after each model turn.
5. Expose public `talon_search`, `talon_read`, and `talon_related` tools to the agent if Hermes supports tool surfacing.
6. Restart the MCP child if it exits unexpectedly, with backoff and a clear error.

The provider should not shell out to one-shot `talon recall` per turn except as a fallback.

### 13.2 Provider Lifecycle Calls

Before model call:

```text
provider.prefetch_or_recall(turn):
  call tools/call talon_hook_recall {
    host: "hermes",
    sessionId,
    turnId,
    message: current_user_message,
    priorMessages: recent_messages,
    budgetTokens: configured_auto_budget,
    format: "prompt-xml" or "agent-json"
  }
  inject returned context into Hermes memory/context slot
```

After model call:

```text
provider.on_turn_end(turn):
  call tools/call talon_hook_turn_end {
    host: "hermes",
    sessionId,
    turnId,
    lastUserMessage,
    lastAssistantMessage,
    outcome
  }
```

On provider shutdown:

```text
call talon_hook_session_end
send MCP shutdown
close child stdin
wait for child
```

### 13.3 Hermes Responsibilities

Hermes owns:

- deciding where returned recall context is inserted in the prompt,
- passing current user message and recent prior messages,
- preserving a stable session ID,
- calling turn-end hook,
- surfacing public search/read/related tools if desired.

Talon owns:

- retrieval,
- confidence gate,
- novelty suppression,
- prefetch,
- session recall ledger,
- index freshness,
- watcher and background embedding,
- output shaping for recall.

Hermes must not implement dedupe by string surgery. It lacks stable Talon chunk IDs, scores, scopes, and source metadata.

## 14. Internal API and Module Plan

Suggested `talon-cli` layout:

```text
crates/talon-cli/src/
  agent_contract.rs
  mcp/
    mod.rs
    protocol.rs
    transport.rs
    server.rs
    state.rs
    tool/
      mod.rs
      public.rs
      hook.rs
      schema.rs
      dispatch.rs
      agent_value.rs
    background/
      mod.rs
      watcher.rs
      refresh.rs
      embed.rs
    session/
      mod.rs
      ledger.rs
      fingerprint.rs
      suppression.rs
      prefetch.rs
```

Suggested `talon-core` additions:

```text
crates/talon-core/src/query/recall/
  ids.rs
  novelty.rs
```

Keep most host-specific MCP behavior in `talon-cli`; keep reusable recall candidate identity and novelty logic in `talon-core` when it is independent of MCP.

## 15. Data Model Changes

Phase 1 can avoid database migrations by deriving temporary chunk IDs.

Longer-term, add stable IDs to recall/search outputs:

- `note_id`
- `chunk_id`
- `heading`
- `from_line`
- `to_line`
- `mtime`

If existing tables already have stable note/chunk primary keys, expose them through output structs rather than adding new schema.

Recall output should eventually include:

```json
{
  "sessionId": "...",
  "turnId": "...",
  "queryFingerprint": "...",
  "mode": "auto",
  "confidence": 0.81,
  "budgetTokens": 500,
  "injectedTokens": 312,
  "results": [
    {
      "noteId": "n_...",
      "chunkId": "c_...",
      "path": "wiki/Agent Memory.md",
      "heading": "Recall Design",
      "score": 0.89,
      "status": "injected",
      "reason": "novel_high_score",
      "content": "..."
    }
  ],
  "suppressed": [
    {
      "chunkId": "c_...",
      "path": "wiki/Agent Memory.md",
      "score": 0.86,
      "reason": "same_chunk_recently_injected"
    }
  ]
}
```

## 16. Implementation Phases

### Phase 0: Contract and Tests Around Current Behavior

- Add tests documenting current MCP tools/list and tools/call behavior.
- Add tests for compact agent output builders once extracted.
- Add a compatibility note that the single `talon` action-union is deprecated.

Acceptance criteria:

- `just check` passes.
- Tests assert current behavior before replacement.

### Phase 1: Reusable Agent Output Values

- Refactor `output/json/agent/*` so each response can produce `serde_json::Value`.
- Keep CLI `--agent` output byte-for-byte compatible where practical.
- Add tests for search/read/related/recall compact values.

Acceptance criteria:

- CLI `--agent` still emits compact JSON.
- MCP can call the same value builders without writing to stdout.

### Phase 2: Named MCP Public Tools

- Add `talon_search`, `talon_read`, and `talon_related`.
- Keep old `talon` action-union behind compatibility if needed for one release.
- Tool descriptions come from `agent_contract`.
- MCP `tools/list` advertises the narrow tool surface.

Acceptance criteria:

- MCP `tools/list` includes `talon_search`, `talon_read`, `talon_related`.
- Public tools return compact agent JSON in text and structured content.
- No public `sync`, `meta`, `changes`, `lint`, or `status`.

### Phase 3: MCP Server State

- Introduce `McpServerState`.
- Load config once.
- Reuse inference/expansion clients.
- Add per-session store.
- Thread state through protocol handling and tool dispatch.

Acceptance criteria:

- `talon mcp` still speaks stdio JSON-RPC.
- Public tools work through state-backed dispatch.
- Existing one-shot CLI commands are unchanged.

### Phase 4: Hook-Only Recall Tools

- Add `talon_hook_session_start`.
- Add `talon_hook_recall`.
- Add `talon_hook_turn_end`.
- Add `talon_hook_session_end`.
- Implement Claude hook JSON output.
- Implement Hermes agent JSON or prompt-XML output.

Acceptance criteria:

- Calling `talon_hook_recall` twice with the same session and prompt suppresses duplicate injection on the second call.
- Claude output includes valid `hookSpecificOutput` for `UserPromptSubmit`.
- Hermes output is directly injectable without full envelope parsing.

### Phase 5: Turn-Aware Suppression

- Add session ledger.
- Add query fingerprint.
- Add chunk/note injection records.
- Add suppression reasons.
- Add novelty-aware selection.
- Add debug telemetry in structured output.

Acceptance criteria:

- Same top chunk is injected once across adjacent turns.
- Query drift can allow re-injection.
- Explicit search/read tools bypass suppression.
- Suppression reasons appear in debug/structured recall results.

### Phase 6: Background Prefetch

- Queue prefetch after `talon_hook_turn_end`.
- Consume matching prefetch in next `talon_hook_recall`.
- Add max-age and fingerprint checks.
- Fall back to bounded synchronous recall when no usable prefetch exists.

Acceptance criteria:

- Prefetch hit returns under 100 ms in tests or local fixture runs.
- Stale prefetch is discarded.
- Hook recall respects `maxSyncMs`.

### Phase 7: Watcher and Background Refresh

- Add vault watcher.
- Debounce file changes.
- Queue no-embed refresh.
- Expose watcher errors through diagnostics/logs.

Acceptance criteria:

- Editing a vault file causes MCP process to refresh index without an explicit sync tool call.
- Search reflects text changes after debounce.
- Concurrent refresh attempts serialize safely.

### Phase 8: Background Embeddings

- Queue embedding pass after refresh.
- Run low-priority pending-chunk embedding.
- Do not block hooks on embedding.
- Record embedding failures in diagnostics.

Acceptance criteria:

- Changed chunks become semantically searchable after background embedding.
- Hook recall still works when embedding sidecar is unavailable.

### Phase 9: Claude Code Plugin

- Add Claude plugin package files.
- Configure MCP server as `talon` using `talon mcp`.
- Add lifecycle hooks.
- Add optional skill/instructions for public tools.
- Document installation and verification.

Acceptance criteria:

- Starting Claude Code connects to `talon mcp`.
- `UserPromptSubmit` auto-injects recall through `additionalContext`.
- `Stop` updates session ledger and queues prefetch.
- Repeated prompts do not duplicate the same recall block.

### Phase 10: Hermes Provider Rewrite

- Replace one-shot recall calls with a persistent MCP JSON-RPC client.
- Call hook tools around each turn.
- Expose public tools if Hermes supports it.
- Add process restart/backoff.
- Remove provider-side dedupe.

Acceptance criteria:

- Hermes starts `talon mcp` once.
- Hermes injects recall before turns.
- Hermes calls turn-end hook after turns.
- Duplicate recall is suppressed by Talon, not Hermes.

## 17. Testing Strategy

### Unit Tests

- Query fingerprint exact and near-match behavior.
- Session ledger insertion, TTL, and LRU eviction.
- Suppression policy by chunk, note, and query repetition.
- Novelty selection under token budget.
- Agent output value builders.
- MCP tool schema generation from contract constants.

### Integration Tests

- MCP lifecycle: initialize, tools/list, tools/call, shutdown.
- Public `talon_search`, `talon_read`, `talon_related`.
- Hook recall first turn injects context.
- Hook recall second same turn suppresses duplicates.
- Turn end queues prefetch.
- Next hook consumes prefetch.
- Watcher refresh updates search results.

### Fixture Tests

Use existing fixture vaults:

- Same query across 5 adjacent turns.
- Query drift from "spring lamb dish" to "hot sauce co-packer".
- Edited file appears in search after watcher refresh.
- Sidecar unavailable fallback path.

### Manual Verification

Claude Code:

1. Configure local plugin.
2. Start `claude`.
3. Ask a prompt with known vault relevance.
4. Confirm recall is injected invisibly through hook context.
5. Ask a similar follow-up.
6. Confirm duplicate snippets are not re-injected.
7. Call `talon_search` manually.
8. Confirm public tool still works.

Hermes:

1. Start Hermes with Talon provider.
2. Confirm one `talon mcp` child process.
3. Run repeated adjacent turns.
4. Confirm recall injection once, suppression afterwards.
5. Edit vault file.
6. Confirm watcher refresh and later recall/search sees edit.

## 18. Observability

Use stderr/logging only for MCP process diagnostics so stdout remains MCP frames.

Track:

- startup config path,
- watcher state,
- refresh start/end/error,
- embedding start/end/error,
- hook recall duration,
- prefetch hit/miss/stale,
- number of injected/suppressed candidates,
- confidence-gate skips,
- fallback mode when inference is unavailable.

Avoid logging full note content by default. Log paths, scores, IDs, and reasons.

## 19. Compatibility and Migration

Short term:

- Keep `talon mcp` command unchanged.
- Consider keeping the current `talon` action-union tool for one release as deprecated, or remove immediately if this project is still pre-release and no external users depend on it.

Recommended:

- Remove broad action-union from `tools/list` when named tools land.
- If compatibility is needed, keep `talon_legacy` hidden only by description and mark deprecated. Do not document it in the skill.

CLI commands remain unchanged.

## 20. Security and Safety

- Hook tools must not write to the vault.
- Public MCP tools should be read-only in the first pass.
- Watcher and background sync write only to Talon's local index DB.
- No shell command execution from MCP inputs.
- All paths must remain vault-relative or validated against configured vault root.
- Avoid returning raw full files through recall hooks. Full reads require explicit `talon_read`.
- Hook-only tools should have low token budgets and confidence gates by default.

## 21. Open Questions

- Does Claude Code's `mcp_tool` hook input support direct variable interpolation for all fields we need, or do we need a small command-hook adapter?
- Should hook-only tools be included in normal `tools/list`, or can Talon use MCP annotations to discourage model use strongly enough?
- Should session ledgers be persisted across MCP process restarts, or is in-memory enough for phase 1?
- What exact prompt slot should Hermes use for injected recall so it is visible to the model but not confused with user-authored text?
- Should background embedding be enabled by default, or gated behind config until watcher refresh is stable?

## 22. Done Definition

The overhaul is complete when:

- `talon mcp` is a long-running stdio MCP server with process state.
- Public MCP tools are limited to search, read, and related.
- Claude Code can install a Talon plugin that auto-injects recall before user prompts.
- Hermes uses the same running MCP server for turn-aware recall hooks.
- Repeated adjacent turns do not duplicate the same recalled chunks.
- Background watcher refresh works without exposing sync as a public MCP tool.
- Existing stateless CLI commands continue to work.
- `just check` passes.

