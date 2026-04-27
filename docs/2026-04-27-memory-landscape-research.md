---
title: 2025-2026 Agent Memory Landscape — Implications for Talon `recall`
date: 2026-04-27
author: research-agent
status: draft
---

# Executive summary

- **Selective injection has won.** Across mem0, Zep, supermemory, Hindsight and Anthropic, full-history replay is gone. Per-turn budgets cluster at **1.5k–10k tokens**, with mem0 at ~1,800 tokens/turn for 91.6 LoCoMo and supermemory's static-profile-every-N-turns pattern as the canonical low-cost design. Talon's 2,000-token default sits exactly in the sweet spot.
- **Format convergence is loose XML / tagged markdown, not JSON.** Letta compiles core memory blocks into the prompt in "an XML-like format"; Anthropic recommends `<background_information>`, `<instructions>` style tags but says "exact formatting is becoming less important"; Zep ships pre-rendered context strings, not JSON. JSON is for tooling/eval, not for the model. **Talon's `prompt-xml` is the right default; the JSON path should remain for hooks and observability.**
- **The big shift is "just-in-time" memory.** Anthropic's official guidance (Sep 2025) is to keep "lightweight identifiers" in context and let the agent pull bodies on demand via tools. Hindsight, Letta and supermemory all do a hybrid: a small auto-injected block plus tools the agent can reach for. **Talon already has both halves — it has not packaged them as a coherent two-mode product.**
- **Recency by itself is harmful.** Multiple 2026 postmortems (Beyond Last-K Turns, the "73-turn drift" study, mem0's stale-employer example, dbreunig's context-poisoning taxonomy) converge on the same finding: recency-only retrieval over-fits to recent turns, evicts user preferences, and amplifies confabulations. Talon's recency half-life is fine for `recent_edits` but should never be a primary selector.
- **Talon's hidden advantage is provenance.** The vault is a markdown filesystem with wikilinks, frontmatter, headings and stable paths. That gives Talon what every poisoning paper begs for: traceable sources, no fabrication, no agent-derived "beliefs" stored as facts. The redesign should lean into that — present recall as *citations into a curated vault*, not as *retrieved memories*.

---

# 1. Inventory of shipped systems (2025–2026)

## 1.1 mem0 (mem0.ai)
- **Architecture.** Selective fact extraction from conversations into a vector store, optional graph layer (Mem0g). Single-pass ADD-only extraction since the 2026 token-efficient algorithm; older two-pass ADD/UPDATE/DELETE deprecated.
  Source: [Introducing The Token-Efficient Memory Algorithm](https://mem0.ai/blog/mem0-the-token-efficient-memory-algorithm).
- **Per-turn budget.** ~1,800 tokens/retrieval on LoCoMo; ~6,800–7,000 on LongMemEval/BEAM. Compared to ~25,000+ for full-context. ~90% reduction with a 6-point accuracy trade.
- **Selection.** Three parallel scoring passes — semantic similarity, keyword (BM25), entity matching — fused with a reranker. Scope filters by `user_id`, `agent_id`, `run_id`, `app_id`. Recency timestamps weight; metadata filters since v1.0.0.
- **Format.** Memory entries are prose strings (extracted facts) returned as a list, prepended to the prompt. No XML structure.
- **Eviction.** Latest fact wins on conflict; staleness is a known failure mode (their own blog cites "the user's employer" as the example: "highly relevant until it is not, at which point it becomes confidently wrong").
- **Benchmarks.** LoCoMo 91.6 (LLM-as-judge), LongMemEval 93.4, BEAM-1M 64.1, BEAM-10M 48.6.
  Source: [State of AI Agent Memory 2026](https://mem0.ai/blog/state-of-ai-agent-memory-2026).

## 1.2 supermemory.ai
- **Architecture.** Postgres + vector + graph behind a single `client.profile()` call returning `{profile.static, profile.dynamic, searchResults}`. Sells itself as a "memory and context layer" with hybrid (RAG + memory) retrieval.
  Source: [supermemory README](https://github.com/supermemoryai/supermemory).
- **Per-turn budget.** "Up to 70% token savings on long conversations" via the Infinite Chat proxy; selective retrieval kicks in past 20k tokens. Profile latency target ~50ms.
  Source: [Infinite Chat](https://supermemory.ai/docs/model-enhancement/context-extender).
- **Selection.** Hybrid (semantic + recency + temporal). Static profile facts are explicit invariants; dynamic facts are recent activity context; search results are per-query.
- **Format.** Three labeled sections injected into the system message: static facts, dynamic facts, recent memories. `maxRecallResults` defaults to 10 items per section.
- **The big idea: profile every N turns.** `profileFrequency` defaults to **50** — the static + dynamic profile are re-injected on turn 1, 51, 101, while search results refresh every turn. Rationale: profile data is invariant-ish; search results are per-question.
  Source: [DeepWiki: Profile Frequency and Context Injection](https://deepwiki.com/supermemoryai/clawdbot-supermemory/6.5-profile-frequency-and-context-injection).
- **Eviction.** Newer facts supersede older. Temporal contradiction resolution is automatic.

## 1.3 Hindsight (vectorize.io)
- **Architecture.** Four "logical networks" — world facts, agent experiences, synthesized entity summaries, evolving beliefs — with three operations: `retain`, `recall`, `reflect`.
  Source: [Hindsight is 20/20 (arXiv 2512.12818)](https://arxiv.org/abs/2512.12818).
- **Per-turn budget.** Three preset budgets `low / mid / high`; `mid` is the default in the OpenAI Agents SDK integration. Trims final results "as needed to fit within the token limit".
  Source: [Persistent Memory for OpenAI Agents](https://hindsight.vectorize.io/blog/2026/04/17/openai-agents-persistent-memory).
- **Selection.** 4-way hybrid retrieval: semantic (dense vectors), keyword (BM25), graph (entity / temporal / causal links), temporal (time-range filter). Reciprocal rank fusion + cross-encoder reranker.
- **Format.** The OpenAI integration auto-prepends recalled memories to the system prompt; the explicit `hindsight_recall` tool is also exposed. Format details aren't in the public docs — appears to be a labeled prose block.
- **Eviction.** Reflection layer rewrites beliefs, but world facts persist with provenance. Their differentiation against mem0/Zep: "memory needs to capture things that are not already sitting in a static document".
  Source: [Your agent is not forgetful](https://hindsight.vectorize.io/blog/2026/04/23/your-agent-is-not-forgetful).
- **Benchmarks.** 91.4 on LongMemEval, up to 89.61 on LoCoMo.

## 1.4 Letta (formerly MemGPT)
- **Architecture.** Hierarchical memory blocks: **core** (always in prompt), **message buffer** (rolling), **archival** (vector store), **recall** (rehydration on demand). The agent itself edits its own blocks via tool calls.
  Sources: [Letta memory blocks](https://docs.letta.com/guides/core-concepts/memory/memory-blocks), [Tim Kellogg: Layers of Memory](https://timkellogg.me/blog/2025/06/15/compression).
- **Per-turn budget.** Each block has a per-block character `limit`. Core memory is the L1 cache — small, every turn; archival is unbounded but only via retrieval. Default sizes are unpublished; common practice is core ≈ 2k tokens, message buffer = the rolling K turns.
- **Selection.** Core blocks are unconditional. Archival uses semantic retrieval. Recall is the agent's deliberate request.
- **Format.** **"Prepended to the agent's prompt in an XML-like format"** — explicit per-block tags labeled by `label`, `description`, `value`. Description fields are essential; without them agents misuse blocks.
- **Eviction.** Message buffer overflows oldest into archival. Core blocks never evict; agent edits in place via `core_memory_replace` and friends.

## 1.5 Zep / Graphiti
- **Architecture.** Bi-temporal knowledge graph (Graphiti). Tracks *valid_at* (when fact became true) and *invalid_at* (when superseded) on every edge; preserves history rather than overwriting.
  Sources: [Graph Overview](https://help.getzep.com/graph-overview), [Zep paper (arXiv 2501.13956)](https://arxiv.org/abs/2501.13956).
- **Per-turn budget.** The "Context Block" is a single pre-rendered string assembled by Zep server-side. Configurable templates with `%{user_summary}`, `%{edges limit=N}`, `%{entities limit=N}`, `%{episodes}`. Default ranges: 5–10 facts, 4–5 entities.
- **Selection.** Graph search by relevance to the running thread; edge filters by validity windows; reranker.
- **Format.** Markdown-ish prose with timestamped relationship lines:
  ```
  - (2024-11-15 14:23:00 - present) [CONTACTED_SUPPORT_ABOUT]
    <Emily contacted support regarding API rate limit concerns>
  ```
  Source: [Context templates](https://help.getzep.com/context-templates).
- **Eviction.** None destructive — facts are *invalidated* with a timestamp, not deleted. Stale facts can be filtered out at query time.
- **Benchmarks.** DMR 94.8 (vs MemGPT 93.4); LongMemEval improvements up to 18.5%, latency reduced ~90%.

## 1.6 Cognee
- **Architecture.** Extract → Cognify → Load pipeline. Vector + graph + relational stores in parallel. Adds temporal context and entity relationships during ingestion.
  Source: [Cognee architecture](https://docs.cognee.ai/core-concepts/architecture).
- **Selection / format.** Multi-store retrieval; graph traversal for relational queries; vector for semantic. Notable for being multi-store-by-design rather than treating graph as an add-on.
- **Relevance to Talon.** Cognee-style cognification (entity extraction, relation typing) is what Talon explicitly *avoids* by trusting the markdown vault. Talon's wikilinks and frontmatter are the human-curated equivalent.

## 1.7 ChatGPT memory (OpenAI)
- **Architecture.** Two stores — "saved memories" (explicit) and "chat history" (implicit). The April 2025 upgrade made memory automatic, with prioritized key details.
  Source: [Memory and new controls for ChatGPT](https://openai.com/index/memory-and-new-controls-for-chatgpt/).
- **Per-turn budget.** Not disclosed; benchmark on LoCoMo is 52.9 (significantly lower than mem0/supermemory/Hindsight, suggesting either small budget or weak selection).
- **Format.** Prose, system-prompt-injected. User can inspect via the memory UI.
- **Eviction.** User-controlled. No automatic temporal decay.

## 1.8 Claude memory (Anthropic)
- **claude.ai memory** (rolled out paid tiers Sep–Oct 2025). Uses the same memory-tool mechanics under the hood. Project-scoped; users can edit memories directly.
- **API memory tool** (`memory_20250818`, beta Sep 29 2025). **A directory of files at `/memories`. The agent reads and writes via `view / create / str_replace / insert / delete / rename` tool calls. No automatic injection — the agent always opens with `view /memories` and pulls only what it needs.**
  Source: [Memory tool docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/memory-tool).
- **Context editing** (`clear_tool_uses_20250919`). Triggers at configurable input-token thresholds (e.g. 5,000 in demos, **30k–40k in production**), keeps the last `K` tool uses, clears at least N tokens.
- **Format.** Files are arbitrary text; the cookbook examples are *XML-tagged markdown* (`/memories/customer_service_guidelines.xml`). Memory contents come back with line numbers when `view`'d.
- **Multi-session pattern.** Bootstrap a `progress log` + `feature checklist` in an initializer session; subsequent sessions begin by reading those, finish by updating them.
- **Key design choice.** **No auto-injection.** Anthropic's design is "Claude pulls" not "the system pushes." Compaction is the orthogonal feature for older context.

## 1.9 LangMem / LangGraph
- **Architecture.** `BaseStore` interface (put/get/search) over a backend (in-memory, Postgres, etc.). LangMem SDK adds memory creation/refinement primitives over any `BaseStore`.
  Source: [LangMem](https://github.com/langchain-ai/langmem).
- **Selection.** Semantic search; namespacing by user/session. Three memory types modeled explicitly: **semantic** (facts), **episodic** (events), **procedural** (system prompts that evolve).
- **Format.** Caller-controlled — LangMem returns memory dicts; you decide how to render them.
- **Notable.** Procedural memory (the system prompt itself is editable by the agent) is the conceptually distinctive piece.

## 1.10 Microsoft Amplifier (context injection budgets)
- Worth flagging because it gives the cleanest published *budget defaults* outside mem0.
  Source: [Amplifier context budgets](https://deepwiki.com/microsoft/amplifier-core/10.2-context-injection-budgets).
- **Defaults: 10 KB per injection, 10,000 tokens per turn.** Estimation heuristic: 1 token ≈ 4 chars. Either limit can be `null` for unlimited.

## 1.11 Other 2025–2026 entrants worth flagging
- **Amazon Bedrock AgentCore Memory** — managed service, OpenAI-style memory + retrieval API.
- **Redis Agent Memory Server** — open-source; ships query optimization (LLM rewrites the query before vector search).
- **Microsoft Azure AI Search "Agentic Retrieval"** — multi-turn parallel query planner; up to 40% relevance gain claim. Treats retrieval itself as an agent.
- **A-RAG** ([arXiv 2602.03442](https://arxiv.org/pdf/2602.03442)) — hierarchical retrieval surfaces three tools to the model: `keyword_search`, `semantic_search`, `chunk_read`. Multi-granularity is becoming the norm.
- **HaluMem** ([arXiv 2511.03506](https://arxiv.org/abs/2511.03506)) — first benchmark explicitly evaluating *hallucinations inside memory systems* (fabrication, error, conflict, omission).

---

# 2. Convergence patterns

## 2.1 Per-turn token budget
| System | Default budget per turn |
|--------|-------------------------|
| mem0 (LoCoMo) | ~1,800 tokens |
| mem0 (LongMemEval / BEAM) | ~6,800–7,000 tokens |
| Microsoft Amplifier | 10,000 tokens |
| Letta core memory | ~2,000 tokens (unwritten convention) |
| Zep context block | 5–10 facts + summary (~1–2k tokens) |
| Hindsight | low / mid / high presets |
| Talon today | **2,000 tokens** |

**Convergent answer:** **1.5k–2k for "always-on" facts; 5k–10k for query-conditioned retrieval; never the full window.**

## 2.2 Selection
The Pareto-optimal recipe in 2026 is:
1. **Semantic similarity** (dense vectors) — primary signal.
2. **Lexical match** (BM25 or equivalent) — protects against embedding misses on rare proper nouns.
3. **Entity / link / graph** — captures relations the embedding can't see.
4. **Recency / temporal validity** — *as a tiebreaker and freshness filter, never the primary signal*.
5. **Scope priority** — `user_id`, `project_id`, `agent_id` filtering before scoring.
6. **Reranker** — cross-encoder (or LLM-as-reranker) on the merged top-K.

Talon's hybrid pipeline already does (1)+(2)+(6); scope priorities cover (5); the link graph is (3). The only missing leg is (4)-as-validity, not (4)-as-primary.

## 2.3 Format
**Tag-structured markdown wins.** What everyone actually ships:

- **Letta:** "XML-like" blocks with explicit labels and descriptions.
- **Anthropic guidance:** `<background_information>`, `<instructions>`, `<tool_guidance>` — but said "exact formatting is becoming less important".
- **Zep:** pre-rendered prose block with timestamped fact lines.
- **mem0:** prose list of extracted facts.
- **supermemory:** three labeled sections (static / dynamic / search).
- **Claude memory tool:** files-on-disk; cookbook examples use `.xml` files inside `/memories`.

**JSON is the ops format, not the model format.** Every system that returns JSON does so for *the runtime*, then renders to prose/XML before injection.

## 2.4 Two-pass / preview-then-expand
This is the "just-in-time" pattern, now mainstream:
- **Anthropic:** lightweight identifiers (paths, URLs) live in context; the agent reads bodies via tools when needed.
- **A-RAG:** three retrieval tools (`keyword_search`, `semantic_search`, `chunk_read`) are exposed; the model picks granularity.
- **Hindsight OpenAI integration:** auto-injects a small block; full `hindsight_recall` and `hindsight_reflect` tools are also exposed.
- **Letta:** core (preview) + archival (expand on tool call).
- **supermemory:** static profile every 50 turns + per-turn search results.

The convergent shape: **a tiny always-on block (~1–2k tokens of titles/summaries/links) + tools to fetch full content on demand.**

## 2.5 Working vs long-term split
- **Working:** last 2–4 turns, verbatim. ([Beyond Last-K Turns](https://rahulrraj.blogspot.com/2026/04/beyond-last-k-turns-building-memory.html))
- **Episodic / semantic:** retrieved on relevance, not recency.
- **Procedural / persona / always-on:** invariants that never evict.

Letta's L1/L2/L3/L4 cache analogy is the cleanest articulation; supermemory's static-vs-dynamic split is the simplest practical one.

## 2.6 Provenance and timestamps
The single most important convergence in 2026:
- Zep timestamps every fact with valid/invalid windows.
- mem0 v1.0.0 added metadata filters explicitly to support staleness checks.
- Schneider's poisoning paper demands per-entry provenance metadata as Defense Layer 2.
- HaluMem's whole point is that memory hallucinates because provenance is lossy.

**Every memory entry should be traceable to a source the agent didn't generate.** Talon's vault — markdown files with stable paths — wins this category by construction.

---

# 3. Failure modes documented in 2025–2026

## 3.1 Stale memory poisoning
**The mem0 employer example.** "A highly-retrieved memory about a user's employer is highly relevant until it is not, at which point it becomes confidently wrong." Once a fact is wrong, every retrieval makes the agent more confident in the wrong answer.
Mitigation: bi-temporal validity (Zep), reinforcement-decay (Schneider Layer 3), provenance + freshness signal at retrieval time.

## 3.2 Adversarial memory poisoning
MINJA-class attacks reach **>95% injection success** across tested LLM agents. The instruction enters via a document, persists in memory, fires on an unrelated turn weeks later.
Source: [Persistent Memory Poisoning](https://christian-schneider.net/blog/persistent-memory-poisoning-in-ai-agents/).
Defenses: (a) trust-scored ingestion, (b) per-entry provenance metadata, (c) trust-aware retrieval that down-weights low-trust sources, (d) behavioral monitoring for off-baseline tool calls.

## 3.3 Lost in the middle / context bloat
Original 2023 paper still bites in 2026. Recent reproduction on long-context models:
- **Gemini 2.5 Pro:** degradation past ~100k tokens.
- **Llama 3.1 405B:** correctness drops at ~32k.
- **GeoEngine benchmark:** Llama 3.1 8B failed with 46 tools, succeeded with 19 — both inside its 16k window.
Source: [How Long Contexts Fail](https://www.dbreunig.com/2025/06/22/how-contexts-fail-and-how-to-fix-them.html).

dbreunig's four named modes — **poisoning / distraction / confusion / clash** — are now the standard taxonomy.

## 3.4 Confabulation as fact
HaluMem categorizes four memory hallucination types: **fabrication, errors, conflicts, omissions**. The pernicious one: an agent generates an inference, that inference is stored in memory, future retrievals treat it as ground truth.
Mitigation: store only sources, not inferences; mark agent-derived content with `derived: true`; never let `reflect`-style summaries get retrieved as facts without an explicit "synthesized" tag.

## 3.5 Recency over-fit
**73-turn drift.** Median onset of measurable agent drift across 847 simulated workflows. ([Smeuse blog](https://blog.smeuse.org/posts/ai-agent-memory-drift-73-turns)).
**Last-N failure modes:** "ok" / "thanks" turns evict structured user preferences; weeks-old context treated identically to seconds-old. ([Beyond Last-K Turns](https://rahulrraj.blogspot.com/2026/04/beyond-last-k-turns-building-memory.html)).
Mitigation: pinned/persona memory that never evicts; semantic relevance over time-ranked.

## 3.6 Memory shouts
A high-priority memory drowns the actual question. If the agent vault has a `priority: critical` note about coding style, every retrieval surfaces it even when the user asks about laundry. Talon has an explicit precedent here: the **scope multiplier** stack (Boosted 3.0× through Buried 0.05×). The risk is that Boosted notes get *always* surfaced; mitigation is to require relevance ∧ priority, not relevance ∨ priority.

## 3.7 Sharded prompt collapse
Microsoft/Salesforce 2025 study: sharded conversations (info delivered across multiple turns) drop performance by ~39%. OpenAI o3 dropped 98.1 → 64.1.
Implication for Talon: prior turns matter for query expansion, but the *current turn's intent* must dominate. Talon's `prior_messages` already feeds expansion — guard against letting it dilute the current-turn signal.

## 3.8 Tool-set bloat
Berkeley Function-Calling Leaderboard: every model degrades with more tools. Talon's MCP surface should be lean — `recall`, `read`, plus a small navigation set — not a sprawl.

## 3.9 Cognitive degradation under multi-turn pressure
2025 CSA framework (Cognitive Degradation Resilience). Agents progressively degrade under adversarial prompts and resource starvation; memory + retrieval are the primary attack surface.

---

# 4. Implications for Talon

Talon's design surface is unusually well-positioned. The vault is already the source of truth, the agent already curates, and the hybrid pipeline already covers the dominant selection signals. The work isn't to add more — it's to **lean into the two distinct modes** and **stop pretending Talon is a memory database when it's actually a citation engine.**

### 4.1 Two modes, one engine

| Mode | Caller | Volume | Format | Purpose |
|------|--------|--------|--------|---------|
| **A: Auto-recall** | Lifecycle hook | Tiny: ~600–800 tokens | Tag-structured prose with paths | "What's relevant right now?" — every turn |
| **B: Agent-driven** | The agent | Large: 2k–8k tokens, multi-shot | JSON + `read` tool | "Let me actually look this up" |

These should share a pipeline but have different defaults and outputs.

### 4.2 What "Boosted/Elevated/Normal/Muted/Buried" really means

Talon's scope priority is a `priority × relevance` interaction, *not* a stand-alone rank. Concretely:
- **Boosted (3.0×)** should mean "if this matches at all, surface it" — but only when `relevance > 0.4`. Below 0.4, multiplier doesn't fire. This stops Boosted notes from shouting on irrelevant queries.
- **Buried (0.05×)** should function as a *negative provenance signal* — these notes are still searchable by name (Mode B) but never auto-injected (Mode A).

### 4.3 The vault wins the provenance war for free

Every recommendation Schneider, HaluMem, Zep make about "tag every memory with source, valid_at, trust_score" is solved by `vaultPath`. Talon must propagate this signal:
- Always include `vaultPath` in every emitted item (already true).
- Always include `lastModified` (gives the agent freshness without forcing decay math server-side).
- Add a `derived` flag for any synthesized content (today: none; tomorrow: if Talon ever auto-summarizes, it must mark it).

### 4.4 Section design

The current 5-section split is structurally right but presentationally wrong. Specifically:
- `active_notes` and `linked_context` are the load-bearing pair.
- `frontmatter`, `recent_edits`, `fuzzy_anchors` add noise more often than signal in Mode A.
- `recent_edits` is a *Mode B* feature — agents asking "what was I working on yesterday" want it, but it has no business in a per-turn auto-injection.

### 4.5 Format

`prompt-xml` is right; the schema is the right shape. Two changes:
1. Drop `<fuzzy_anchors>` and `<recent_edits>` from Mode A by default.
2. Inline `mtime` on every `<note>` so the agent can judge freshness itself rather than relying on Talon's recency math. Zep's `(2024-11-15 - present)` style is the model.

### 4.6 The confidence gate is doing real work

The `evidence_score < min_confidence` skip is the single most important behavior in `recall`. Every memory paper in 2026 begs for it under different names ("retrieval ≠ valid answer", "confidence gates"). **Make it more aggressive in Mode A** — if the top hit is below 0.4 hybrid score, return `skipped=true` and let the agent ask explicitly. False negatives are cheap (the agent reaches for `read`); false positives poison context.

### 4.7 Don't store inferences

Talon should never write to the vault on the agent's behalf. The agent curates. This is the critique that mem0/Hindsight pretend to have ("we extract facts!") but actually fail at — they store agent-derived inferences as ground truth and have no way to distinguish. **Talon's "we don't write" stance is a feature, not a gap.** Document it loudly.

---

# 5. Recommendations for Talon

Numbered, opinionated, sized.

## High impact / low effort

1. **Split `recall` into two modes: `--mode auto` (default) and `--mode explicit`.**
   *Auto* returns ≤800 tokens, only `active_notes` + `linked_context`, prompt-xml. *Explicit* returns the current 5-section payload at 2k+ tokens.
   Rationale: the "what to inject per turn" vs "the agent reaches for it" distinction is the most consistent design pattern in 2026.
   **Impact: high. Effort: low.** It's a flag + a section filter.

2. **Default `budget_tokens` per mode: auto=600, explicit=2000.** Document that the auto budget puts Talon in line with mem0's LoCoMo regime.
   *Impact: med. Effort: low.*

3. **Inline `mtime` (or `days_ago`) on every `<note>` in prompt-xml output.** Steal Zep's `(YYYY-MM-DD - present)` style. Gives the agent freshness signal without server-side decay math.
   *Impact: high. Effort: low.* One serializer change.

4. **Tighten the confidence gate in auto mode.** Default `min_confidence=0.4` for auto; keep `0.0` for explicit. Document that a `skipped=true` is *not* an error — it's the gate working.
   *Impact: high. Effort: low.* One default change.

5. **Drop `recent_edits` and `fuzzy_anchors` from auto mode.** Keep them in explicit. Both are useful for "what was I working on" queries (Mode B), both are noise on per-turn injection.
   *Impact: high. Effort: low.* Section filter.

## High impact / medium effort

6. **Add a "preview" tier between active_notes and full bodies.** Today `active_notes` carries a snippet; add a flag (default in auto: on) that strips snippets to 1 line + heading breadcrumb. The agent can `read --raw` for full content. Mirrors Anthropic's just-in-time pattern.
   *Impact: high. Effort: medium.*

7. **Reframe scope priority as `priority × relevance`, not pure multiplier.** Specifically: a Boosted note with hybrid score < 0.4 should *not* hit the multiplier. This kills the "memory shouts" failure mode.
   *Impact: med. Effort: medium.* One scoring change in the rerank stage.

8. **Add `--pinned` to surface a small set of always-on notes (frontmatter `pinned: true`).** This is Letta's core memory equivalent. Caps at e.g. 200 tokens, always included in auto mode regardless of evidence_score. The agent's CLAUDE.md / persona / current-project notes go here.
   *Impact: high. Effort: medium.*

9. **Expose the link graph as a separate tool: `talon links <path> [--depth 2]`.** Today `linked_context` is a section of recall. Mode B agents want to navigate the graph deliberately — make it a first-class CLI verb.
   *Impact: high. Effort: medium.*

## Medium impact / medium effort

10. **Add a `--style` flag for output presentation: `xml` (default), `markdown`, `prose`.** Letta does XML, Zep does prose, mem0 does prose-list. Different agent harnesses prefer different shapes; it's a small surface area to support all three.
    *Impact: med. Effort: med.*

11. **Mark all auto-injected content with `<vault_recall data-injection="auto">` vs `data-injection="explicit">`.** Lets the agent (or downstream tools) tell the difference, which matters for poisoning audits and evals.
    *Impact: med. Effort: low.*

12. **Make `prior_messages` opt-in for query expansion, not default.** Sharded-prompt research shows prior turns can drown the current intent. Today it's `Vec<String>` with default empty — that's fine, but document the failure mode in `recall.md` and add a `--no-prior` killswitch for Mode A.
    *Impact: med. Effort: low.*

## High impact / high effort

13. **Re-tune `evidence_score` weights against an eval set.** Current weights (0.45 rerank + 0.20 lex + 0.15 graph + 0.10 recency + 0.10 frontmatter) were calibration v1. With LongMemEval and LoCoMo public, build a Talon-specific eval (vault + held-out queries) and tune. Mem0 reports 91.6 LoCoMo at 1.8k tokens; that's the benchmark to hit.
    *Impact: high. Effort: high.*

14. **Add a "freshness-aware" gate: down-weight notes whose `mtime` is older than `--freshness-floor` *only when frontmatter has a `volatile: true` flag*.** This is the bi-temporal validity idea, but pushed to the vault author rather than inferred. Avoids over-decaying stable knowledge.
    *Impact: med. Effort: high.* Requires schema convention.

## Punchy / aesthetic

15. **Rename `evidence_score` → `confidence` everywhere external.** It's what every other system calls it; matches Schneider's "confidence gate" prior; makes the skipped behavior obvious. Keep `evidence_score` internally if you want.
    *Impact: low. Effort: low.*

16. **Document Talon's three "we don't do this" claims** loudly in `recall.md`:
    - We don't write to the vault.
    - We don't store agent inferences.
    - We don't dedupe / quality-grade — the agent curates.
    These are the failures every other system has and Talon avoids by construction. They're the headline.
    *Impact: med (positioning). Effort: low.*

---

# 6. Injection mechanics deep-dive

§1–§5 covered architecture. This section answers the orthogonal question: **what literal bytes hit the prompt?** Same inventory, but the unit of analysis is the wrapper string, not the storage backend. Everything below is sourced to OSS code or docs by file path and line number; "COULDN'T FIND" is used in place of "appears to be" when the public surface stops short.

## 6.1 Per-system injected-text inventory

### 6.1.1 Hermes-Hindsight (the primary target)

The plugin Sean is replacing lives at [`NousResearch/hermes-agent/plugins/memory/hindsight/__init__.py`](https://github.com/NousResearch/hermes-agent/blob/main/plugins/memory/hindsight/__init__.py) (1,372 lines; the README and `plugin.yaml` are slim and the actual injection logic is here). It implements two distinct `MemoryProvider` hooks: `system_prompt_block()` (static, written into the system prompt at boot) and `prefetch()` (dynamic, attached to the current user message before each LLM call).

**Static block** — `system_prompt_block()`, lines 1036–1056:

```python
return (
    f"# Hindsight Memory\n"
    f"Active. Bank: {self._bank_id}, budget: {self._budget}.\n"
    f"Relevant memories are automatically injected into context. "
    f"Use hindsight_recall to search, hindsight_reflect for synthesis, "
    f"hindsight_retain to store facts."
)
```

There are three variants (`context` / `tools` / hybrid); all are 3–4 lines, ~50 tokens, and announce the bank id and budget tier to the model. The static block is the **only** thing that lives in the system prompt — see also issue [#13631](https://github.com/NousResearch/hermes-agent/issues/13631), which establishes the contract that "the cached system prompt must be byte-stable for the session; any [memory] content that varies turn-to-turn must ride on the current user message." This is a load-bearing design choice for KV-cache reuse and Talon's existing `prompt-xml` should respect it.

**Dynamic block** — `prefetch()`, lines 1058–1074:

```python
header = self._recall_prompt_preamble or (
    "# Hindsight Memory (persistent cross-session context)\n"
    "Use this to answer questions about the user and prior sessions. "
    "Do not call tools to look up information that is already present here."
)
return f"{header}\n\n{result}"
```

The `result` body comes from line 1108 — `text = "\n".join(f"- {r.text}" for r in resp.results if r.text)` — i.e. **a flat bullet list of prose sentences**. No score, no timestamp, no entry id, no path. The model gets one preamble (~30 tokens) and N opaque facts. The `recall` tool path (line 1301) numbers entries (`f"{i}. {r.text}"`) instead, but the auto-injected path uses bullets.

- **Source citation**: `plugins/memory/hindsight/__init__.py` lines 1036–1056 (static), 1058–1074 (preamble), 1108 (per-entry), 1300–1302 (tool-path empty/numbered).
- **Position**: static → system prompt, ordered after agent identity, before tool definitions. Dynamic → prepended to the **current user message** (Hermes core fans out `prefetch()` and inlines its return value before the user's text). Cache-stable.
- **Order within the block**: server-side rerank order from `client.arecall(...)`, opaque to the plugin. Most-relevant-first.
- **Per-entry format**: `- {text}` (bullets). No metadata visible to the model.
- **Skip / empty handling**: line 1067 — `if not result: return ""` (silent omit, no header). The tool-call path returns the literal string `"No relevant memories found."` (line 1300) instead.
- **Token-budget enforcement**: budget is a server-side preset (`low/mid/high`, default `mid`), and the client passes `max_tokens=4096` (line 466 default). Over-budget trimming happens upstream in the `hindsight-client` library; the plugin does not re-truncate.
- **Retain cadence**: `retain_every_n_turns` defaults to **1** (line 458) — every turn writes back. This is the most aggressive retain setting in the inventory and is the proximate cause of the "stale memory" complaint flagged in §3.1: every turn deposits another extraction the agent will later confidently retrieve.

### 6.1.2 mem0

mem0 is a *library*, not an injector — it returns a wrapped dict and lets the caller render. From [`mem0/memory/main.py`](https://github.com/mem0ai/mem0/blob/main/mem0/memory/main.py) line 1486–1488:

```python
return {"results": original_memories}
```

Each entry is `{"id": "...", "memory": "...", "score": 0.8, "metadata": {...}}`. The published mem0 cookbook examples render this as a numbered prose block under a hard-coded header — typically `"You have access to the following memories:\n" + "\n".join(f"{i}. {m['memory']}" for i, m in enumerate(results, 1))`. The four prompt templates in [`mem0/configs/prompts.py`](https://github.com/mem0ai/mem0/blob/main/mem0/configs/prompts.py) are *extraction* prompts, not *injection* templates — fact-retrieval, user-only extraction, agent-only extraction, and the ADD/UPDATE/DELETE update prompt. mem0 ships extraction; the host application ships injection.

- **Source citation**: `mem0/memory/main.py:1486-1488` (return shape); `mem0/configs/prompts.py` (extraction, not injection).
- **Position**: caller-controlled. The reference Hermes integration (mem0 docs, Hermes integration page) injects on the user message, same path as Hindsight.
- **Order**: hybrid score order (semantic + BM25 + entity, fused).
- **Per-entry format**: caller-controlled; `score` and `metadata` are present in the dict but seldom rendered.
- **Skip / empty handling**: empty list, no fallback string. Caller decides.
- **Token-budget enforcement**: ~1,800 tokens/turn on LoCoMo benchmarks; not enforced by the SDK.

### 6.1.3 Letta

The richest wrapper in the inventory. From [`letta/schemas/memory.py`](https://github.com/letta-ai/letta/blob/main/letta/schemas/memory.py) `_render_memory_blocks_standard`, lines 287–316:

```python
s.write("<memory_blocks>\nThe following memory blocks are currently engaged in your core memory unit:\n\n")
for idx, block in enumerate(renderable):
    label = self._display_label(block.label or "block")
    value = block.value or ""
    desc = block.description or ""
    chars_current = len(value)
    limit = block.limit if block.limit is not None else 0

    s.write(f"<{label}>\n")
    s.write("<description>\n")
    s.write(f"{desc}\n")
    s.write("</description>\n")
    s.write("<metadata>")
    if getattr(block, "read_only", False):
        s.write("\n- read_only=true")
    s.write(f"\n- chars_current={chars_current}")
    s.write(f"\n- chars_limit={limit}\n")
    s.write("</metadata>\n")
    s.write("<value>\n")
    s.write(f"{value}\n")
    s.write("</value>\n")
    s.write(f"</{label}>\n")
    if idx != len(renderable) - 1:
        s.write("\n")
s.write("\n</memory_blocks>")
```

Wrapped in `compile()` (lines 461–505), which then optionally appends `<tool_usage_rules>...</tool_usage_rules>` and per-source `<directories>`. **Three renderers exist**: standard (above), `_render_memory_blocks_git` (for git-enabled agents), and `_render_memory_blocks_line_numbered` (Anthropic models on `sleeptime_agent` / `memgpt_v2_agent` / `letta_v1_agent`). `react_agent` and `workflow_agent` skip memory entirely.

- **Source citation**: `letta/schemas/memory.py:287-316` (renderer), `:461-505` (compile flow).
- **Position**: composed into the system prompt at session boot. Does **not** ride on the user message — Letta's working/archival split handles per-turn retrieval differently.
- **Order**: definition order (the order the agent created/edited blocks).
- **Per-entry format**: nested XML — `<{label}><description>...</description><metadata>- read_only=true\n- chars_current=N\n- chars_limit=M</metadata><value>...</value></{label}>`. The model sees the block name, the human-written description, and a character budget for self-managed editing.
- **Skip / empty handling**: line 287 — `if len(renderable) == 0: s.write(""); return`. Empty silent skip; no `<memory_blocks>` wrapper at all when there's nothing.
- **Token-budget enforcement**: per-block `limit` field, exposed *to the model* as `chars_limit` so it can self-regulate via `core_memory_replace`. No global cap.

### 6.1.4 Zep / Graphiti

From [`help.getzep.com/advanced-context-block-construction`](https://help.getzep.com/advanced-context-block-construction) (the docs publish the literal default; the same template is what `thread.get_user_context()` returns in `mode="basic"`):

```
FACTS and ENTITIES represent relevant context to the current conversation.
# These are the most relevant facts and their valid date ranges
# format: FACT (Date range: from - to)
<FACTS>
{facts}
</FACTS>

# These are the most relevant entities
# ENTITY_NAME: entity summary
<ENTITIES>
{entities}
</ENTITIES>
```

A real `{facts}` line: `Emily is experiencing issues with logging in. (2024-11-14 02:13:19+00:00 - present)`. A real `{entities}` line: `Emily0e62: User account with suspended status due to payment failure.` In `mode="summary"` the body is replaced with a natural-language paragraph; basic mode returns the raw `<FACTS>` / `<ENTITIES>` form.

- **Source citation**: `help.getzep.com/advanced-context-block-construction` (default template, public). Also rendered by the Zep server, not the SDK — the client receives a pre-built string.
- **Position**: caller-controlled, but Zep's reference apps inject on the system message; v3 docs explicitly recommend this.
- **Order**: graph-search relevance; valid-at-newest within tied edges.
- **Per-entry format**: `FACT (YYYY-MM-DD HH:MM:SS+00:00 - present)`. `present` is literal — when an edge is invalidated, Zep replaces `present` with the invalidation timestamp. **The model sees the date range; this is the sharpest provenance signal in the inventory.**
- **Skip / empty handling**: empty `<FACTS>` and `<ENTITIES>` blocks still render; the wrapper text is constant. (Cache-stability win.)
- **Token-budget enforcement**: configurable templates with `%{edges limit=N}` / `%{entities limit=N}`; defaults are 10 facts + 5 entities.

### 6.1.5 Hindsight (vectorize.io, the OG; distinct from §6.1.1)

Same engine that the Hermes plugin embeds. The OpenAI Agents SDK integration ([blog post 2026-04-17](https://hindsight.vectorize.io/blog/2026/04/17/openai-agents-persistent-memory)) auto-prepends a recalled memories block to the system prompt. The SDK source is split across `hindsight-clients/` in [`vectorize-io/hindsight`](https://github.com/vectorize-io/hindsight); the docs surface a different preamble than the Hermes plugin uses:

> "Relevant memories from past conversations (prioritize recent when conflicting). Only use memories that are directly useful to continue this conversation; ignore the rest:"

This preamble is in the SDK wrapper, not the Hermes plugin. The Hermes plugin overrides it with the shorter `# Hindsight Memory (persistent cross-session context)` header (§6.1.1).

- **Source citation**: vectorize-io/hindsight SDK docs ([sdks/integrations/hermes](https://hindsight.vectorize.io/sdks/integrations/hermes)). Source code path inside `hindsight-clients/python/hindsight/agents.py` is referenced but not browseable in the public mirror as of writing — **COULDN'T FIND** the literal Python source for the SDK preamble; the doc string above is the authoritative public surface.
- **Position**: system prompt prepend in the OpenAI Agents SDK; user-message ride-along in Hermes.
- **Order, per-entry format, skip**: as §6.1.1.
- **Token-budget enforcement**: `low / mid / high` presets; `recall_max_tokens=4096` default.

### 6.1.6 supermemory

From [`supermemory.ai/docs/user-profiles`](https://supermemory.ai/docs/user-profiles) — the documented injection template (verbatim, JS template literal):

```
You are assisting a user.

ABOUT THE USER:
${profile.static?.join('\n') || 'No profile yet.'}

CURRENT CONTEXT:
${profile.dynamic?.join('\n') || 'No recent activity.'}

Personalize responses to their expertise and preferences.
```

`profile.static` is an array of long-term facts; `profile.dynamic` is an array of recent-activity strings; both are pre-rendered to one fact per line. The third return value, `searchResults`, is rendered separately (typically as a numbered list under a `# Relevant Memories` header in the docs samples — but the samples are *examples*, not a fixed template, so I won't quote them as canonical).

- **Source citation**: `supermemory.ai/docs/user-profiles` (literal template). Source code in [`supermemoryai/supermemory`](https://github.com/supermemoryai/supermemory) — **COULDN'T FIND** the corresponding TS source path that emits this template; the docs are authoritative.
- **Position**: system prompt. Static + dynamic refresh on `profileFrequency` (default 50 turns); `searchResults` refresh every turn.
- **Order**: arbitrary — `join('\n')` preserves array order; arrays are server-ordered.
- **Per-entry format**: one fact per line, no metadata, no scores, no timestamps. **The lowest-information entry format in the inventory.**
- **Skip / empty handling**: the wrapper is constant. Empty arrays render as `'No profile yet.'` / `'No recent activity.'` (literal strings, not silent).
- **Token-budget enforcement**: `maxRecallResults=10` per section; "up to 70% token savings" is achieved upstream by the Infinite Chat proxy, not this template.

### 6.1.7 Anthropic memory tool

From [`anthropics/claude-cookbooks/tool_use/memory_cookbook.ipynb`](https://github.com/anthropics/claude-cookbooks/blob/main/tool_use/memory_cookbook.ipynb), the system prompt that introduces the tool is — verbatim from cell 14:

```python
system="You are a code reviewer."
```

That's it. No memory preamble, no instructions about reading `/memories` first, no schema. The agent's behaviour ("check memory before answering") is a *learned* pattern from the tool description (`memory_20250818`), not from prompt engineering. The cookbook explicitly notes (cell 4) that "Claude can write down what it learns for future reference" — that's the only on-prompt cue.

The literal file content that ends up at `/memories/concurrency_patterns/thread_safety.md` (cookbook cell 26 example):

```markdown
# Thread Safety / Race Condition Pattern

**Symptom**: Inconsistent results in concurrent operations
**Cause**: Shared mutable state (lists/dicts) modified from multiple threads
**Solution**: Use locks, thread-safe data structures, or return results instead
**Red flags**: Instance variables in thread callbacks, unused locks, counter increments
```

When Claude `view`s this, the response is the file contents with **prepended line numbers**, e.g. `1: # Thread Safety / Race Condition Pattern\n2: \n3: **Symptom**: ...`. This is the only "wrapper" — and it's a tool-result wrapper, not a system-prompt one.

- **Source citation**: `tool_use/memory_cookbook.ipynb` cells 14, 17, 26; [memory tool docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/memory-tool).
- **Position**: nothing is auto-injected. The agent calls `view /memories` on its own.
- **Order**: filesystem order. The agent decides which file to read.
- **Per-entry format**: file contents with line numbers (e.g. `1: ...\n2: ...`).
- **Skip / empty handling**: `view /memories` on an empty store returns `Directory: /memories\n(empty)` (cookbook output).
- **Token-budget enforcement**: orthogonal — context editing (`clear_tool_uses_20250919`) at 30k–40k input tokens in production; cookbook demos use 5k.

### 6.1.8 LangMem / LangGraph

[`langchain-ai/langmem`](https://github.com/langchain-ai/langmem) ships `create_search_memory_tool` and `create_manage_memory_tool`. Memories are stored under a namespace (default `("memories", "{user_id}")`) and exposed *as tools*; LangMem itself does not inject a wrapper. The closest thing to a template is in the `MemoryManager` extraction prompts (analogous to mem0's `prompts.py`), not at injection time.

The reference docs at [langchain-ai.github.io/langmem/guides/memory_tools](https://langchain-ai.github.io/langmem/guides/memory_tools/) demonstrate caller-side injection — typically a system message of the shape `"You have access to the following memories:\n{memories}"` — but this is not a published default. The injection is owned by the host LangGraph node.

- **Source citation**: **COULDN'T FIND** a literal default injection template. LangMem is tools + storage, not an injector. Examples in docs are illustrative.
- **Position**: caller-controlled.
- **Order**: caller-controlled; default search returns highest-similarity first.
- **Per-entry format**: caller-controlled; the `Item` returned by search is `{"key", "value", "score", "namespace"}`.
- **Skip / empty handling**: caller-controlled.
- **Token-budget enforcement**: none.

### 6.1.9 Microsoft Amplifier

From [`microsoft/amplifier-core`](https://github.com/microsoft/amplifier-core) — `coordinator.py` enforces injection budgets but does not hard-code an injection template; the actual wrapper text is provider-supplied via the Mount Plan's `session` section. Defaults from [DeepWiki / Amplifier 10.2](https://deepwiki.com/microsoft/amplifier-core/10.2-context-injection-budgets):

- **10 KB per injection**, **10,000 tokens per turn**.
- Token estimation: `len(content) / 4` (4 chars per token heuristic).
- Per-injection size limit is *hard*: `ValueError` on overflow, blocks the injection.
- Per-turn token budget is *soft*: logs a warning, still proceeds.

- **Source citation**: `amplifier_core/coordinator.py:87, 117-136, 359-363, 439-502` (per DeepWiki). The Python repo no longer exposes `amplifier/memory/builder.py` — the memory coordinator is the Rust kernel with hooks.
- **Position**: hook-supplied; Mount Plan controls placement (system / user / tool).
- **Order**: hook-supplied.
- **Per-entry format**: hook-supplied. Amplifier mediates *budgets*, not formats.
- **Skip / empty handling**: empty content does not invoke the hook.
- **Token-budget enforcement**: dual (size + token), as above. **The cleanest published budget primitive in the inventory** — Amplifier's contribution is the per-turn 10k cap that other systems achieve implicitly via reranker top-K.

### 6.1.10 OpenAI Agents SDK session memory

From [developers.openai.com/cookbook/examples/agents_sdk/session_memory](https://developers.openai.com/cookbook/examples/agents_sdk/session_memory). Sessions expose `get_items() -> List[TResponseInputItem]` and the SDK *replays* prior items as plain `{role, content}` messages on the next call. There is **no wrapper, no preamble, no FACTS section** — verbatim message-list replay. When the session summarizes, it injects two synthetic messages flagged with `meta: {"synthetic": True, "kind": "history_summary_prompt"}`, but these are still ordinary user/assistant messages from the model's perspective.

- **Source citation**: OpenAI Agents SDK cookbook (above).
- **Position**: messages list, before the new user message.
- **Order**: chronological.
- **Per-entry format**: native message dicts.
- **Skip / empty handling**: empty session → empty replay; the new user message is the first.
- **Token-budget enforcement**: session-level summarization at a configurable token threshold; below it, all turns replay verbatim.

### 6.1.11 Redis Agent Memory Server

From [`redis/agent-memory-server/agent_memory_server/api.py`](https://github.com/redis/agent-memory-server/blob/main/agent_memory_server/api.py), `memory_prompt` endpoint, lines 952–1135. Verbatim from lines 1020–1120:

```python
if working_mem.context:
    _messages.append(
        SystemMessage(
            content=TextContent(
                type="text",
                text=f"## A summary of the conversation so far:\n{working_mem.context}",
            ),
        )
    )
# ... then recent_messages are appended verbatim ...
if long_term_memories.total > 0:
    long_term_memories_text = "\n".join(
        [f"- {m.text} (ID: {m.id})" for m in long_term_memories.memories]
    )
    _messages.append(
        SystemMessage(
            content=TextContent(
                type="text",
                text=f"## Long term memories related to the user's query\n {long_term_memories_text}",
            ),
        )
    )
else:
    _messages.append(
        SystemMessage(
            content=TextContent(
                type="text",
                text="## Long term memories related to the user's query\n No relevant long-term memories found.",
            ),
        )
    )

_messages.append(
    base.UserMessage(
        content=TextContent(type="text", text=params.query),
    )
)
```

Two system messages — `## A summary of the conversation so far:` and `## Long term memories related to the user's query` — followed by the (token-trimmed) prior message replay, followed by the user query. **The "no relevant memories found" sentinel is rendered explicitly** rather than omitted, which keeps the system-prompt shape stable across turns.

- **Source citation**: `agent_memory_server/api.py:1020-1126`.
- **Position**: two prepended system messages; user query last.
- **Order**: working-memory summary first, then chronological message replay, then long-term memory bullets.
- **Per-entry format**: `- {text} (ID: {id})`. ID is exposed to the model — unique among the systems surveyed; lets the agent reference back to a specific memory in its response.
- **Skip / empty handling**: `## Long term memories related to the user's query\n No relevant long-term memories found.` — explicit sentinel, **not** silent omit.
- **Token-budget enforcement**: working-memory replay is token-trimmed by removing oldest first (lines 1032–1051); long-term is capped by the upstream `SearchRequest.limit`.

### 6.1.12 ChatGPT memory (acknowledged, proprietary)

OpenAI does not publish the literal injection template. Inspectable via DevTools as a synthetic system message of the form `Model set context:\n1. ...\n2. ...` (UI-confirmed in the memory-edit panel). Quoting that as canonical would be guessing — **COULDN'T FIND** an authoritative public source.

## 6.2 Cross-system mechanics matrix

| System | Wrapper tokens | Position | Order | Score visible? | Date / mtime visible? | Skip behaviour |
|--------|---------------|----------|-------|----------------|----------------------|----------------|
| **Hermes-Hindsight (auto)** | ~30 | Current user message | Server rerank | No | No | Silent omit |
| **Hermes-Hindsight (tool)** | ~10 (numbered list) | Tool result | Server rerank | No | No | `"No relevant memories found."` |
| **mem0** | Caller | Caller | Hybrid score | In dict, rarely rendered | In dict, rarely rendered | Caller |
| **Letta** | ~25 + per-block ~40 | System prompt | Block creation order | No | No (but `chars_limit` is) | Empty silent skip (no `<memory_blocks>`) |
| **Zep (basic)** | ~70 | System prompt | Graph relevance | No | **Yes, `(YYYY-MM-DD HH:MM:SS+00:00 - present)`** | Wrapper preserved, body empty |
| **Vectorize Hindsight (OpenAI SDK)** | ~50 | System prompt | Server rerank | No | No | Likely silent omit (couldn't verify) |
| **supermemory** | ~30 | System prompt | Array order | No | No | Constant fallback strings |
| **Anthropic memory tool** | 0 | None — agent pulls | Filesystem order | No | mtime via filesystem `view` | Tool returns `(empty)` |
| **LangMem** | Caller | Caller | Cosine similarity | In `Item`, rarely rendered | In namespace, rarely rendered | Caller |
| **Microsoft Amplifier** | Hook-supplied | Hook-supplied | Hook-supplied | Hook-supplied | Hook-supplied | `ValueError` on size overflow |
| **OpenAI Agents SDK** | 0 | Messages list | Chronological | No | No (but message order implies time) | Empty replay |
| **Redis Agent Memory Server** | ~25 + ~25 (two headers) | Two system messages + replay | Working: chrono. LTM: hybrid score | No | No | Explicit sentinel string |
| **Talon today (`prompt-xml`)** | ~30 + per-section ~10 | Caller | Hybrid + scope mult. | **Yes (`score="..."` attr)** | Frontmatter only, not inline | Silent omit + `evidence_score` skip |

The matrix sharpens three points already implied by §1–§2:

1. **Talon is one of two systems that exposes per-entry score to the model** (the other is Redis, which exposes `id`). Score-in-prompt is unusual; the rest of the field treats it as ops metadata.
2. **Only Zep exposes inline timestamps to the model.** Every other system either omits time entirely or relies on the agent inferring it from message order. §5 recommendation 3 ("steal Zep's `(YYYY-MM-DD - present)` style") is the consensus-deviation move worth making.
3. **Skip behaviour splits cleanly two ways.** Cache-prioritising systems (Zep, supermemory, Redis) preserve the wrapper and render a fallback string. Cache-agnostic systems (Hermes-Hindsight auto, Letta, Talon, Anthropic) silently omit. The Hermes-on-user-message position is what makes silent omit safe — it doesn't churn the cached system prompt.

## 6.3 Three worked examples

Live recall against `/tmp/talon-dogfood-vault` (chef vault). For each query I show what Talon emits today, then what mem0, Letta, Zep, and a proposed "Talon-auto" mode would inject for the same query, with token counts (4 chars/token, Amplifier convention).

### 6.3.1 Query: "what's the lamb dish for spring"

**Talon today (`prompt-xml`, full sections), abbreviated:**

```xml
<vault_recall source="talon" vault="/tmp/talon-dogfood-vault" evidence_score="0.9000">
  <active_notes>
    <note path="projects/Spring 2026 Menu/Dish - Lamb Neck.md" title="Dish - Lamb Neck" score="1.3879"># Lamb Neck
    **Price:** $34 | **Plate cost:** ~$9.50 | **Margin:** 3.6x
    ## Concept
    Slow-braised lamb neck, served warm with intensity. ...
    [full snippet ~1,800 chars]</note>
    <note path="raw/Recipe Clip - Smoked Lamb Neck.md" title="Recipe Clip - Smoked Lamb Neck" score="0.2588">...[1,400 chars]</note>
    <note path="projects/Spring 2026 Menu/Spring 2026 Menu.md" title="Spring 2026 Menu" score="1.2641">...[~600 chars]</note>
  </active_notes>
  ...
</vault_recall>
```

≈ **3,800 chars / 950 tokens**, three notes, score-on-attribute, path-on-attribute, no inline mtime.

**mem0 (rendered from `{"results": [...]}`):**

```
You have access to the following memories:
1. Lamb neck is the spring 2026 showstopper protein, priced at $34 with a 3.6x margin and ~$9.50 plate cost.
2. The lamb neck is slow-braised at 180°C for 3 hours, inspired by Diana Henry's smoked lamb neck recipe.
3. Spring 2026 menu has four anchors plus two rotational specials, debuting May 5.
```

≈ **350 chars / 90 tokens**. No paths, no provenance, agent-derived prose. Rich for an LLM but lossy — the actual technique numbers ("180°C, covered, 3 hours") get summarised away on extraction. **Best for an agent that doesn't need to cite back; worst for an agent that does.**

**Letta:**

```
<memory_blocks>
The following memory blocks are currently engaged in your core memory unit:

<spring_menu>
<description>
Calle Sur's Spring 2026 menu, debuting May 5. Active dishes and project notes.
</description>
<metadata>
- read_only=false
- chars_current=2840
- chars_limit=4000
</metadata>
<value>
Spring 2026 Menu has four anchors plus two rotational specials.
- Charred Spring Onion ($18, opener)
- Fava and Whey ($16)
- Lamb Neck ($34, showstopper, slow-braised 3hrs at 180°C)
- [draft] Artichoke + black garlic
Plate costs target $10 except lamb neck. Inspired by Diana Henry's smoked lamb (technique adapted, not literal).
</value>
</spring_menu>

</memory_blocks>
```

≈ **700 chars / 175 tokens**. The block is a *summary written by the agent at some prior turn*, not a citation. Letta wins for "always-in-context persona" but is structurally unable to surface a fresh recipe note that wasn't proactively edited into a block. **Best for stable persona/project context; structurally wrong for vault retrieval.**

**Zep:**

```
FACTS and ENTITIES represent relevant context to the current conversation.
# These are the most relevant facts and their valid date ranges
# format: FACT (Date range: from - to)
<FACTS>
Lamb Neck is the spring 2026 showstopper protein at $34. (2026-04-12 09:14:00+00:00 - present)
Lamb Neck plate cost is ~$9.50 with a 3.6x margin. (2026-04-12 09:14:00+00:00 - present)
Lamb Neck is slow-braised at 180°C for 3 hours, adapted from Diana Henry's smoked lamb neck recipe. (2026-04-15 11:02:00+00:00 - present)
The lamb braise can be made one day ahead and reheated gently on service. (2026-04-15 11:02:00+00:00 - present)
</FACTS>

# These are the most relevant entities
# ENTITY_NAME: entity summary
<ENTITIES>
Lamb Neck: Spring 2026 menu dish, $34, 3.6x margin, slow-braised 3hrs at 180°C.
Spring 2026 Menu: Calle Sur's seasonal menu debuting May 5; four anchors + two rotational specials.
Diana Henry: Cookbook author whose Smoked Lamb Neck recipe inspired the dish technique.
</ENTITIES>
```

≈ **900 chars / 225 tokens**. **Inline timestamps are the standout** — the agent sees that the "3 hours at 180°C" fact was added April 15, four days before the menu commit, and can weight accordingly. But every fact is server-extracted; the agent cannot read the source note for the sauce-reduction step.

**Proposed "Talon-auto" (the §5.1 mode, applied):**

```xml
<vault_recall mode="auto" evidence_score="0.9000">
  <active_notes>
    <note path="projects/Spring 2026 Menu/Dish - Lamb Neck.md" mtime="2026-04-15" score="1.39">
      Lamb Neck — $34 plate, 3.6x margin. Slow-braised 3hrs at 180°C, adapted from
      Diana Henry's smoked lamb (raw/Recipe Clip - Smoked Lamb Neck.md). Make-ahead
      ok, reheat gently.
    </note>
    <note path="projects/Spring 2026 Menu/Spring 2026 Menu.md" mtime="2026-04-19" score="1.26">
      Spring 2026 Menu, debuting May 5. Anchors: Spring Onion, Fava+Whey, Lamb Neck.
    </note>
  </active_notes>
</vault_recall>
```

≈ **480 chars / 120 tokens**. Two notes, inline `mtime`, breadcrumb summaries instead of full body, paths preserved. **Citations + freshness + budget.** The agent can `talon read projects/Spring 2026 Menu/Dish - Lamb Neck.md` for the full braising recipe.

**Commentary.** For a focused factual query like this, mem0 wins on raw token efficiency but is the worst on citation fidelity — the very fact that "$9.50 plate cost" survives mem0's extraction is luck, not contract. Letta is wrong shape: this isn't persona, it's vault. Zep is the most defensible — timestamped facts, named entities — but it can only surface what was extracted; the make-ahead instruction *might* not be there. Talon-auto is the "best of both" for an agent that has `read` available: 120 tokens of citations + breadcrumbs, full bodies one tool call away.

### 6.3.2 Query: "what should I be working on this week"

**Talon today** returns five sections; the top hit is `artifacts/Weekly Prep List 2026-W16.md` (score 0.92) which contains a literal day-by-day prep list. Total payload: ~3,400 chars / 850 tokens, mixing the prep list with `Costing Fundamentals` (score 2.20 from frontmatter boost — useful but unrelated), a koji recipe clip (low score, noise), and a vault hygiene meta note. **Frontmatter score boosting is over-firing here**: `Costing Fundamentals` outranks the actual weekly prep list because of frontmatter tagging.

**mem0** would return prose like `1. The chef is preparing for Spring 2026 menu launch May 5. 2. Weekly prep is structured around Tuesday morning lamb braise inspections...` — useful but loses the day-of-week structure of the actual prep list.

**Letta** would surface a `current_project` block written at some earlier turn — likely outdated.

**Zep** would surface "FACT: This week's prep priorities are X, Y, Z. (2026-04-21 09:00:00+00:00 - present)". Concise but server-summarized.

**Talon-auto** would *correctly* drop the koji clip and meta note (low score after `min_confidence=0.4` gate from §5 rec 4). But it should also handle this query specially: it's a "broad / agentic / pinned-style" query where the right answer is **the most recent prep list**, not a hybrid-search result. **The §5 rec 8 "pinned notes" feature is the natural answer**: if `frontmatter: pinned-this-week: true` is set on the W16 prep list, it bypasses the score gate.

**Commentary.** Broad/temporal queries are the weakest match for hybrid retrieval. Zep wins because graph + temporal is its native primitive; Talon's recency half-life is its weakest signal here. The pinned-note primitive (§5.8) is the right answer — it's literally Letta's `core` block but driven by frontmatter rather than agent self-edit.

### 6.3.3 Query: "tasting counter financials"

**Talon today** returns `Tasting Counter Pitch Deck Notes` (0.93), `Tasting Counter` (1.34), `Financial Projections 2026` (0.04 — *but it's in `private/`*), and the equipment list + layout sketches. ~3,800 chars / 950 tokens. **The interesting case**: the `private/Financial Projections 2026.md` note hits the query but with a near-zero relevance score (0.04). It's surfacing because of vault-wide search, not because of relevance. **A `min_confidence=0.4` gate (§5 rec 4) would correctly drop it.**

**mem0** would return prose with `1. Tasting counter pitch is targeting an investor/lender for $95–$120/cover omakase. 2. ...`. mem0 has no concept of `private/` — if those notes were ever added, their content would be extracted and conflated with public facts. **This is the §3.6 ("memory shouts") + §3.4 (confabulation as fact) failure compounded.**

**Letta** would have whichever block the agent edited; same blind-spot problem.

**Zep** would surface FACTs like "Tasting counter target margin is 50%. (2026-04-19 - present)" — and would conflate the public pitch deck with the private projections unless the host explicitly tagged them.

**Talon-auto with a `--scope public` filter (proposed)** would correctly include the pitch-deck notes (which are in `artifacts/` and `projects/`) and *exclude* the `private/` note. This is the §5 rec 7 "Buried = never auto-injected" principle, but enforced by directory rather than priority — and it's a stronger signal because directory placement is unambiguous.

**Commentary.** Cross-domain queries that touch private folders are the *adversarial test* every memory system fails. mem0/Zep/Letta have no notion of vault directories; everything is one bag. Talon's **directory + frontmatter scope + Buried priority** stack is the only design in the inventory that has multiple independent signals to keep `private/` content out of an auto-recall. The §5 rec 7 reframing of `priority × relevance` is necessary but not sufficient — directory-level scope filters matter as much.

## 6.4 Implications back to Talon (refining §4 and §5)

Five updates to the existing recommendations, anchored to deep-dive evidence.

**(a) The 600-token auto budget is right; if anything, lean lower.** Hermes-Hindsight ships 4096 `recall_max_tokens` as the *upper* bound on the SDK side; the actual auto-injected payload after server-side budget tier selection is closer to 1k–2k. supermemory's `maxRecallResults=10` per section + ~20 chars/fact = ~600 tokens combined static+dynamic. Zep's default 5–10 facts + 4–5 entities ≈ 700 tokens. **The convergence is 600–800 for "static-ish" or "always-on", up to 2k for "with bodies".** Talon's `auto=600` keeps full bodies and is therefore tight; if `--style breadcrumb` (§6.3.1's worked example) drops bodies in favour of one-line summaries, **400 tokens is achievable** without information loss for a vault-with-`read`-tool agent.

**(b) Zep's `(YYYY-MM-DD HH:MM:SS+00:00 - present)` is the right shape, but compact it.** Full ISO timestamps are visually noisy; the meaningful signal is the date and the literal `present`. The Hermes-Hindsight plugin exposes *no* time at all; Letta hides it; Anthropic exposes it via tool-result file metadata. The §5 rec 3 instinct is right — but the format should be `mtime="2026-04-15"` (date only) or `(2026-04-15 — present)` with the long dash, not the full ISO string. The "present" sentinel only matters when entries can be invalidated; in a markdown vault, every note is "present" by definition (the file still exists), so a plain `mtime` is enough. **Refined recommendation 3**: inline `mtime="YYYY-MM-DD"` as an attribute on `<note>`, drop the `- present` decoration, mention the convention in the system block once.

**(c) Hermes-Hindsight's user-message ride-along is the anti-pattern Talon must replicate, not avoid.** This is the deep-dive's sharpest single finding. KV-cache stability is a *first-class* concern: every byte that varies in the system prompt invalidates the cache. The Talon `prompt-xml` output is documented as a system-prompt block, but if Hermes is the target host, **it must be inserted on the user message, not the system prompt** — see [hermes-agent issue #13631](https://github.com/NousResearch/hermes-agent/issues/13631). **New recommendation**: document the contract in `recall.md` — "the prompt-xml output is intended to ride on the *current user message*, not the system prompt; system-prompt placement is supported but defeats KV-cache reuse on prefix-caching backends (Anthropic, vLLM, sglang)."

**(d) The Hermes-Hindsight failure modes Talon should avoid by design.**
- **Retain-every-turn (`retain_every_n_turns=1`, line 458) is the proximate cause of stale-memory poisoning** (§3.1). Each turn writes back; each future retrieval makes the agent more confident. **Talon's "we don't write to the vault" stance (§4.7) is exactly the right counter.** Document it as Hermes-Hindsight's worst feature, by name.
- **Prefetch returns prose bullets with no metadata** (line 1108). The agent gets sentences, not citations. Every Hindsight-fed answer is an unsourced claim. **Talon's `path=` and `score=` attributes are the antidote.** Document the contrast in `recall.md`.
- **`recall_max_input_chars=800` truncates the user query before search** (line 1085). For long composite queries (multi-message threads), Hindsight silently drops the suffix. **Talon's `prior_messages` expansion + `--no-prior` killswitch (§5 rec 12) is the correct shape.**

**(e) New recommendation: a Redis-style "no relevant memories" sentinel for cache-stable hosts.** §5 rec 4 says "tighten the gate; `skipped=true` is the gate working." The deep-dive shows Redis Agent Memory Server takes the *opposite* tack: render a literal `## Long term memories related to the user's query\n No relevant long-term memories found.` to keep the system-prompt shape constant. **Both are right, in different deployment modes.** When Talon outputs to a system-prompt host (e.g. an OpenAI Agents SDK app), `<vault_recall><skipped reason="below confidence threshold"/></vault_recall>` is cache-friendlier than a vanishing block. **New recommendation 17**: add `--skip-style {silent, sentinel}` (default `silent`); document `sentinel` as the right choice for system-prompt placement and prefix-caching hosts.

**(f) Refining recommendation 8 (pinned notes) with concrete budget.** §5 rec 8 caps pinned notes at "e.g. 200 tokens." The deep-dive convergence at 600–800 for the always-on tier suggests **pinned should be 100–200 tokens (a single small note: pinned breadcrumb + path), not a full body**. The full-body version is what `read` is for. Pinned-as-breadcrumb keeps the §6.3.2 worked example honest: the pinned `Weekly Prep List 2026-W16.md` is a path + one-line summary, not 700 tokens of prep checklist.

---

# Sources

Inline above. Selected anchors:
- mem0: [token-efficient algorithm](https://mem0.ai/blog/mem0-the-token-efficient-memory-algorithm), [State of Agent Memory 2026](https://mem0.ai/blog/state-of-ai-agent-memory-2026), [arXiv 2504.19413](https://arxiv.org/abs/2504.19413).
- supermemory: [README](https://github.com/supermemoryai/supermemory), [profileFrequency](https://deepwiki.com/supermemoryai/clawdbot-supermemory/6.5-profile-frequency-and-context-injection), [Infinite Chat](https://supermemory.ai/docs/model-enhancement/context-extender).
- Hindsight: [arXiv 2512.12818](https://arxiv.org/abs/2512.12818), [vectorize-io repo](https://github.com/vectorize-io/hindsight), [Your agent is not forgetful](https://hindsight.vectorize.io/blog/2026/04/23/your-agent-is-not-forgetful).
- Letta: [memory blocks](https://docs.letta.com/guides/core-concepts/memory/memory-blocks), [Tim Kellogg compression](https://timkellogg.me/blog/2025/06/15/compression).
- Zep: [Graph Overview](https://help.getzep.com/graph-overview), [Context Templates](https://help.getzep.com/context-templates), [Zep paper (arXiv 2501.13956)](https://arxiv.org/abs/2501.13956), [State of the Art Agent Memory](https://blog.getzep.com/state-of-the-art-agent-memory/).
- Anthropic: [Memory tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/memory-tool), [Memory cookbook](https://github.com/anthropics/claude-cookbooks/blob/main/tool_use/memory_cookbook.ipynb), [Effective context engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents).
- LangMem: [github](https://github.com/langchain-ai/langmem), [Semantic search for LangGraph memory](https://www.langchain.com/blog/semantic-search-for-langgraph-memory).
- Microsoft Amplifier: [context budgets](https://deepwiki.com/microsoft/amplifier-core/10.2-context-injection-budgets).
- Hermes-Hindsight plugin: [`__init__.py`](https://github.com/NousResearch/hermes-agent/blob/main/plugins/memory/hindsight/__init__.py), [memory provider plugin guide](https://hermes-agent.nousresearch.com/docs/developer-guide/memory-provider-plugin), [issue #13631 (KV-cache + auto-injection)](https://github.com/NousResearch/hermes-agent/issues/13631).
- Letta render code: [`letta/schemas/memory.py`](https://github.com/letta-ai/letta/blob/main/letta/schemas/memory.py).
- Zep default template: [`help.getzep.com/advanced-context-block-construction`](https://help.getzep.com/advanced-context-block-construction).
- supermemory: [`supermemory.ai/docs/user-profiles`](https://supermemory.ai/docs/user-profiles).
- Redis Agent Memory Server: [`agent_memory_server/api.py`](https://github.com/redis/agent-memory-server/blob/main/agent_memory_server/api.py).
- Anthropic memory cookbook: [`memory_cookbook.ipynb`](https://github.com/anthropics/claude-cookbooks/blob/main/tool_use/memory_cookbook.ipynb).
- Failure modes: [How Long Contexts Fail](https://www.dbreunig.com/2025/06/22/how-contexts-fail-and-how-to-fix-them.html), [Memory poisoning](https://christian-schneider.net/blog/persistent-memory-poisoning-in-ai-agents/), [HaluMem (arXiv 2511.03506)](https://arxiv.org/abs/2511.03506), [Beyond Last-K Turns](https://rahulrraj.blogspot.com/2026/04/beyond-last-k-turns-building-memory.html), [73-turn drift](https://blog.smeuse.org/posts/ai-agent-memory-drift-73-turns), [OpenAI Agents SDK session memory](https://developers.openai.com/cookbook/examples/agents_sdk/session_memory).
- Lost in the middle: [Stanford 2023 paper](https://cs.stanford.edu/~nfliu/papers/lost-in-the-middle.arxiv2023.pdf), [GM-Extract follow-up (arXiv 2511.13900)](https://arxiv.org/abs/2511.13900).
