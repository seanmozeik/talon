# Talon Recall: Efficient Cross-Turn Memory Design

**Date:** 2026-04-29  
**Status:** design draft  
**Scope:** Improve `talon recall` and the Hermes `talon-recall` memory provider so automatic vault recall is fast, cache-friendly, deduplicated across turns, and optionally complemented by explicit agent tools.

## 0. Summary

Talon recall currently behaves like a synchronous stateless pre-turn lookup:

1. Hermes calls `prefetch(query)`.
2. The provider shells out to `talon --agent recall <query> --format prompt-xml`.
3. Talon runs recall and returns XML for injection into the current user message.
4. Hermes does not persist that injected context into the conversation history.

That works, but it has three costs:

- **Latency:** the main agent blocks while recall runs.
- **Repeated context:** adjacent turns often retrieve the same notes again.
- **Cache churn:** injected context changes the current user message every turn, and hidden model reasoning can amplify the prompt replay problem.

The target design is not “make Talon return less useful context.” It is:

- precompute recall before the next turn when possible,
- reuse recent recall state instead of re-searching blindly,
- suppress or summarize duplicate notes across a conversation,
- keep injected context bounded and stable,
- expose explicit tools for deeper recall so automatic injection can stay small.

## 1. Current Hermes Memory Patterns

Hermes memory providers have two relevant surfaces:

- `system_prompt_block()` returns static provider instructions/status.
- `prefetch(query)` returns dynamic context injected at API-call time.

Built-in Hermes memory is relatively cache-stable because `MEMORY.md` and `USER.md` are folded into the system prompt snapshot. External providers such as Hindsight, Mem0, and Talon use dynamic `prefetch()` injection.

Hindsight and Mem0 mitigate the dynamic-injection cost in two ways:

- They can queue background prefetch work after a turn, then consume it on the next `prefetch()`.
- They tend to return compact, deduplicated memory facts rather than whole retrieved note snippets.

Talon currently uses `sync_turn()` only to remember prior user messages for `--prior-message`; it does not queue a background recall result, maintain a recent-note ledger, or deduplicate injected note content across turns.

## 2. Preserve Reasoning vs Replay Reasoning

There are two separate concepts that are easy to conflate:

- **Preserve thinking/reasoning:** keep the model’s hidden reasoning in the response object as `reasoning_content`.
- **Replay reasoning:** send prior assistant `reasoning_content` back in the next request as assistant history.

Replaying reasoning is useful for some providers because their protocol expects thinking blocks, signatures, or provider-native continuity fields to survive across tool calls. It can also help local KV cache only when the replayed text tokenizes to exactly the same prefix the server already cached.

For local `llama-server`, replay is not automatically free:

- it adds hidden prompt material,
- it must match the previous generated token stream exactly,
- any mismatch before or inside the replayed block forces prefill,
- checkpoint granularity can still require replaying thousands of tokens,
- dynamic memory injection can move the current-turn boundary and reduce reuse.

So Talon should not depend on reasoning replay for efficiency. Recall should be efficient even when hidden reasoning replay is disabled for local custom providers.

## 3. Goals

- Keep automatic recall under a predictable latency budget.
- Avoid injecting the same note snippets every turn.
- Make recall state conversation-aware without making Talon Hermes-specific.
- Preserve high-relevance recall quality.
- Make all injected context auditable: source paths, stable note/chunk IDs, scores, and reason.
- Let agents explicitly request deeper recall instead of overloading automatic memory injection.

## 4. Non-Goals

- Do not make Talon responsible for Hermes prompt assembly.
- Do not store raw conversation transcripts in Talon.
- Do not require Hermes patches for the core recall algorithm.
- Do not make automatic recall perform expensive multi-hop research.
- Do not hide retrieved source identities from the agent.

## 5. State Model

Add an optional recall session state keyed by `session_id`.

Talon should accept a stable `session_id` from the provider and maintain a small ledger:

```text
session_id
last_query_hash
last_query_embedding_id or embedding hash
recent_turns[]
recent_injections[]
  - note_id / chunk_id
  - path
  - heading / line range
  - injected_at_turn
  - score
  - token_count
  - content_hash
suppressed_candidates[]
```

The state can live in process memory first. Later, if Talon runs as a daemon, persist it in the Talon DB or a lightweight side table. It should be bounded by session count and TTL.

Recommended initial bounds:

- keep last `8` turns per session,
- keep last `32` injected chunks,
- expire inactive session state after `6h`,
- cap state storage to a few MB.

## 6. Recall Pipeline

### 6.1 Query Construction

Inputs:

- current user query,
- last `N` user messages,
- optional assistant-visible topic summary,
- optional active project/context hints,
- session ID.

Do not concatenate too much history. Current default `prior_message_count=2` is reasonable. Add a query fingerprint:

```text
query_fingerprint = hash(normalized_current_query + normalized_recent_user_turns)
```

Use the fingerprint for request caching and duplicate suppression.

### 6.2 Candidate Retrieval

Run the normal Talon hybrid pipeline:

- lexical candidates,
- semantic candidates,
- optional expansion,
- rerank.

But split this from injection. Retrieval may find many relevant chunks; injection should select only the smallest useful set.

### 6.3 Recent-Result Suppression

Before injection, compare candidates to `recent_injections`.

Suppress a candidate when:

- same `chunk_id` was injected in the last `K` turns,
- same `note_id` has already supplied enough context recently,
- content hash is identical or near-identical,
- candidate score is close to prior injected score and query drift is small.

Allow reinjection when:

- current query has high novelty versus recent queries,
- candidate score is extremely high,
- candidate was injected many turns ago,
- user explicitly asks to revisit that note/topic,
- the requested scope changed.

Suggested first-pass thresholds:

```text
same_chunk_cooldown_turns = 4
same_note_soft_limit_per_window = 2
query_drift_reinject_threshold = 0.35
force_reinject_score = 0.92
```

### 6.4 Novelty-Aware Selection

Rank candidates with a freshness penalty:

```text
effective_score =
  rerank_score
  + scope_boost
  + explicit_path_boost
  - recent_chunk_penalty
  - recent_note_penalty
  - duplicate_content_penalty
```

The output should prefer:

- one or two high-confidence new snippets,
- compact note summaries for recently seen notes,
- source pointers over repeated large excerpts.

### 6.5 Injection Modes

Talon recall should support at least three automatic injection modes:

```text
off       no automatic context, tools only
summary   inject compact recall summary + source IDs
snippets  inject selected snippets within budget
```

Recommended default for Hermes local models:

```text
mode = "summary"
budget_tokens = 300-600
```

The `snippets` mode remains available when the user wants more automatic recall.

## 7. Background Prefetch

Match the Hindsight/Mem0 pattern:

- `queue_prefetch(query, session_id)` starts recall work after the agent completes a turn.
- `prefetch(query, session_id)` first tries to consume a queued result.
- If the queued result matches the new query fingerprint or is close enough, return it immediately.
- If stale, either return no recall or run a bounded synchronous fallback.

Provider-side behavior:

```text
sync_turn(user, assistant):
  talon recall prefetch --session-id S --query user --prior-message ...

prefetch(query):
  talon recall consume --session-id S --query query --max-age-ms 30000
  if no usable result:
    talon recall --sync --timeout-ms 1500 ...
```

This avoids blocking the next user turn on full hybrid search/rerank most of the time.

Important: this should be implemented in Talon, not by Python string surgery in the Hermes provider. The Hermes provider should become a thin caller.

## 8. Output Contract

Add a structured JSON mode in addition to prompt XML:

```json
{
  "session_id": "20260429_...",
  "query_fingerprint": "...",
  "mode": "summary",
  "confidence": 0.81,
  "budget_tokens": 500,
  "injected_tokens": 312,
  "results": [
    {
      "note_id": "n_...",
      "chunk_id": "c_...",
      "path": "wiki/Agent Memory.md",
      "heading": "Recall Design",
      "score": 0.89,
      "status": "injected",
      "reason": "new_high_score",
      "content": "..."
    }
  ],
  "suppressed": [
    {
      "chunk_id": "c_...",
      "path": "wiki/Agent Memory.md",
      "score": 0.86,
      "reason": "same_chunk_recently_injected"
    }
  ]
}
```

Then render prompt XML from that structured object. This makes testing and logging much easier.

## 9. Explicit Tool Surface

Do expose tools beyond automatic memory, but keep them small and predictable.

Recommended tools:

### `talon_search`

Agent-initiated search over the vault.

Use for broad lookup when automatic recall is insufficient.

Fields:

```text
query
scope
limit
fast
include_snippets
```

### `talon_read`

Read a specific note, path, docid, or chunk.

Use after `talon_search` finds a source.

Fields:

```text
path_or_id
raw
from_line
max_lines
```

### `talon_related`

Walk wikilinks/backlinks from a known note.

Use for graph exploration.

Fields:

```text
path_or_id
depth
direction
limit
```

### `talon_recall_status`

Debug current automatic recall state.

Fields:

```text
session_id
```

Returns recent injected notes, suppressed candidates, last latency, and current mode.

Avoid exposing too many tools initially. Automatic recall should stay a memory provider; explicit tools are for deliberate retrieval.

## 10. Hermes Provider Changes

The Hermes Python provider should remain thin:

- load config,
- pass `session_id`,
- pass current query and recent prior messages,
- consume queued results,
- expose optional tool schemas.

Current provider changes needed:

- implement `queue_prefetch()` to call Talon prefetch asynchronously or via a daemon command,
- have `prefetch()` consume precomputed recall first,
- pass `--session-id`,
- pass `--mode`,
- pass `--max-sync-ms`,
- optionally expose `talon_search`, `talon_read`, `talon_related`, `talon_recall_status`.

Do not implement note dedupe or suppression in the provider. Talon owns that because Talon has stable chunk IDs, scores, scopes, and source metadata.

## 11. Config

Suggested `talon-recall.json` shape:

```json
{
  "vault_path": "/opt/data/workspace/obsidian",
  "mode": "summary",
  "budget_tokens": 500,
  "min_confidence": 0.4,
  "fast": false,
  "prior_message_count": 2,
  "background_prefetch": true,
  "max_sync_ms": 1500,
  "queued_result_max_age_ms": 30000,
  "dedupe": {
    "enabled": true,
    "same_chunk_cooldown_turns": 4,
    "same_note_soft_limit_per_window": 2,
    "window_turns": 8,
    "query_drift_reinject_threshold": 0.35,
    "force_reinject_score": 0.92
  },
  "tools": {
    "enabled": true,
    "search": true,
    "read": true,
    "related": true,
    "status": true
  }
}
```

## 12. Observability

Log structured counters, not content:

```text
recall_total_ms
cache_hit=true|false
queued_result_used=true|false
query_chars
candidate_count
rerank_count
injected_count
suppressed_count
injected_tokens
suppressed_reasons
mode
confidence
session_id
```

The Hermes provider should log the same top-level timing fields it already logs, plus whether the result came from queued prefetch.

## 13. Rollout Plan

### Phase 1: Structured Recall Result

- Add `talon recall --format json`.
- Include stable chunk IDs, note IDs, scores, token estimates, and suppression fields.
- Keep existing XML output unchanged.

### Phase 2: Session-Aware Deduplication

- Add `--session-id`.
- Maintain in-memory session recall ledger.
- Suppress same chunks/notes across adjacent turns.
- Render XML from novelty-filtered result set.

### Phase 3: Background Prefetch

- Add `talon recall prefetch`.
- Add `talon recall consume`.
- Update Hermes provider `queue_prefetch()` and `prefetch()`.
- Synchronous fallback should be short and bounded.

### Phase 4: Explicit Tools

- Add provider tool schemas.
- Wire tool calls to Talon CLI/MCP commands.
- Keep automatic injection in `summary` mode by default.

### Phase 5: Daemon Integration

- Move recall state into the long-running Talon MCP/daemon process.
- One-shot CLI calls can ask the daemon for session-aware recall.
- Persist bounded recall state if needed.

## 14. Test Plan

- Same query across 5 adjacent turns injects the same chunk once, then emits summary/source pointer or suppresses it.
- Query drift above threshold allows a previously seen note to reappear.
- Explicit “read/search again” request bypasses suppression.
- Background prefetch hit returns under 100ms provider-side.
- Synchronous fallback respects `max_sync_ms`.
- Prompt XML never exceeds `budget_tokens` by more than one snippet.
- Structured JSON includes suppressed reasons without leaking hidden state.
- Tool mode works when automatic recall is `off`.

## 15. Recommended Defaults

For Ultraclaw/Hermes local models:

```text
automatic recall mode: summary
budget_tokens: 500
background_prefetch: true
max_sync_ms: 1500
dedupe: enabled
explicit tools: enabled
```

This keeps normal chat fast while preserving the ability to search/read deeply when the agent needs it.

