# Recall Query Distillation And Model Budget Design

Reported: 2026-04-30

This document captures the agreed design for making Claude Code MCP recall reliable on large prompts while improving retrieval quality. It replaces the current recall mental model of "take whatever the hook receives and run ordinary query expansion" with a bounded, token-aware query distillation and phrase retrieval pipeline.

## Problem

Talon is used in production as an MCP server for Claude Code. It works well for the first few turns, then hooks can become extremely slow on large messages while CPU sits at 100%.

The original suspected causes were synchronous embedding/rerank work on the hook path, oversized prompt or answer payloads, sidecar context overflow, and possible stress or leaks when Claude Code sends large hook messages.

Investigation found the important path:

- `talon_hook_recall` runs synchronously during `UserPromptSubmit`.
- Recall used the full user prompt, and until the cleanup in `559b88c`, the server could also store prior assistant response text for next-turn context.
- Recall then built a single query and ran the full non-fast hybrid path: BM25/title probe, optional LLM expansion, embedding, vector retrieval, fusion, and rerank.
- The expansion prompt is designed for short search reformulations, not for distilling a large conversation turn.
- Sidecar failures are mostly handled, but failure after sending oversized payloads is too late for a hook. The local process still has to allocate, serialize, tokenize, send, wait, and possibly retry or time out.

Even if the sidecar returns 413/422 quickly, a synchronous hook can already have spent too much time. If the sidecar tokenizes a huge request, CPU-spins, OOMs, or restarts, the hook can appear hung until the HTTP timeout. In a Claude Code hook, that is a user-visible reliability failure.

The immediate reliability cleanup is already done:

- The Claude plugin no longer installs a `Stop` hook.
- `talon_hook_turn_end` was removed from the Claude-facing MCP hook set.
- Recall no longer reads or stores assistant output.
- Claude recall now uses only the current `UserPromptSubmit` prompt.

That cleanup is necessary but not sufficient. Large user prompts can still be too large or too noisy to send directly to expansion, embedding, rerank, or ask synthesis.

## Product Direction

Talon is an Obsidian memory and recall tool. Claude Code is the current host, but recall should optimize for vault memory, not codebase analysis. It should not ingest tool-call transcripts, tool results, or assistant output by default. The user's prompt is the intent signal.

Recall should remain semantic by default. Large input should not mean "always fast lexical." Large input should mean "distill into model-sized retrieval queries, then run semantic recall." Fast lexical is the fallback when semantic work fails or exceeds the hook deadline.

Ask should also become model-budget aware. A context-overflow error is not acceptable as normal behavior because it wastes model calls and user time. Talon should know the model context window, trim or refuse locally before sending, and avoid requests known to overflow.

## Research Context

The relevant RAG category is query transformation, not plain query expansion.

Common approaches:

- Query rewriting: turn conversational or noisy input into a standalone retrieval query.
- Query decomposition: split multi-part requests into separate focused searches.
- Query expansion and multi-query: generate alternate phrasings for an already-valid query.
- HyDE/query2doc: generate a hypothetical answer or document and embed that.
- Keyword/keyphrase extraction: cheaply extract high-signal terms and phrases from one text.

Microsoft's RAG guidance treats query augmentation, decomposition, rewriting, HyDE, and keyword extraction as distinct retrieval techniques. That distinction matters here: Talon's current query expansion prompt is reasonable for short ambiguous searches, but not for huge agent/user turns.

Relevant references:

- Microsoft Azure Architecture Center, "Retrieval Augmented Generation (RAG) in Azure AI Search": https://learn.microsoft.com/en-us/azure/architecture/ai-ml/guide/rag/rag-information-retrieval
- Ma et al., "Query Rewriting for Retrieval-Augmented Large Language Models": https://arxiv.org/abs/2305.14283
- Wang et al., "Query2doc: Query Expansion with Large Language Models": https://arxiv.org/abs/2303.07678
- Gao et al., "Precise Zero-Shot Dense Retrieval without Relevance Labels" (HyDE): https://arxiv.org/abs/2212.10496
- `yake-rust` crate documentation: https://docs.rs/yake-rust/latest/yake_rust/
- `keyword_extraction` crate documentation: https://docs.rs/crate/keyword_extraction/1.4.2
- `rake` crate documentation: https://docs.rs/rake/latest/rake/

The consensus for Talon:

- Do not start with HyDE for hooks. It adds a slow LLM call and creates another context-window problem.
- Do not run generic expansion on raw recall prompts.
- Use deterministic phrase extraction and a query-distiller prompt for oversized inputs.
- Test whether the existing expansion model, `bonsai`, is good enough as the query distiller.

## Configuration

Clean cutover. No backwards compatibility for `max_tokens`.

`max_tokens` becomes `max_output_tokens` everywhere it means generated output length. Each model also gets a `context_tokens` value. These limits live near the model definitions, not in a global unrelated limits table.

Known current defaults:

- `bonsai` context window: 32768 tokens.
- `qwen-smol` context window: 65536 tokens.

Target shape:

```toml
[expansion]
provider = "openai-compatible"
base_url = "http://host.docker.internal:8000/v1"
model = "bonsai"
context_tokens = 32768
max_output_tokens = 768

[ask]
model = "qwen-smol"
context_tokens = 65536
max_output_tokens = 2048
planning_reasoning_effort = "none"
synthesis_reasoning_effort = "medium"

[inference.models]
query_embedding = "embed"
query_embedding_context_tokens = 512
document_embedding = "embed"
chunk_embedding = "embed_chunked"
reranker = "rerank"
reranker_context_tokens = 512

[mcp.hooks]
recall_deadline_ms = 20000
```

The exact names can be adjusted during implementation, but the semantics are fixed:

- `context_tokens`: total usable model context window for input plus output reserve.
- `max_output_tokens`: maximum generated output tokens.
- Embedding/reranker context fields protect sidecar input size.
- The hook deadline is a wall-clock budget for the entire synchronous recall hook.

## Token Budgeting

Talon already depends on `tokenx-rs` and uses token estimation for recall output budgeting. That should become the shared budgeting layer for all model-bound inputs.

Every model or sidecar request must be checked before sending:

- Expansion/distillation chat requests.
- Embedding query requests.
- Reranker query and candidate payloads.
- Ask planning requests.
- Ask synthesis requests.

If a payload is too large, Talon must trim, distill, or skip locally. It should not discover context overflow by paying for a failed remote call.

For chat models, budget accounting must reserve output:

```text
system tokens
+ user/input tokens
+ structured prompt overhead
+ max_output_tokens
<= context_tokens
```

For ask synthesis:

```text
system tokens
+ question tokens
+ planned query tokens
+ source snippet tokens
+ citation/path metadata tokens
+ max_output_tokens
<= ask.context_tokens
```

Ask must trim source snippets before sending. Context-overflow requests should not be sent.

## Claude Hook Input Policy

For the Claude Code plugin:

- `SessionStart` may register a session.
- `UserPromptSubmit` calls recall.
- `SessionEnd` may touch session lifecycle.
- There is no `Stop` hook.
- No assistant final response is passed.
- No transcript parsing is performed.
- No tool calls or tool results are processed.

This avoids noisy and unreliable inputs. The user's submitted prompt is the strongest available intent signal. Assistant output is derivative, can be long, can include code or tool summaries, and created the wrong incentives for recall.

Hermes integration is out of scope for this design. It can be revisited separately.

## Recall Query Distillation

Recall currently treats the query as already search-shaped and optionally expands it. The new design treats recall input as a source document that may need to be transformed into retrieval queries.

The recall pipeline should be:

```text
UserPromptSubmit prompt
  -> estimate tokens
  -> deterministic phrase/literal extraction
  -> if prompt fits budgets:
       use prompt as the main query
     else:
       use the query model with a distiller prompt
       produce a compact retrieval query and selected phrases
  -> build retrieval query set
  -> run semantic and lexical retrieval over query set
  -> fuse results
  -> rerank when inside deadline and model budgets
  -> build recall payload
  -> if semantic path fails or exceeds deadline:
       fallback to fast lexical using the best compact query/phrases
```

Small prompts do not need LLM rewriting. They are treated as already rewritten. Phrase extraction still runs because it is cheap and may improve retrieval.

Large prompts must not be sent raw to expansion, embedding, or rerank. They are distilled first.

## Query Distiller LLM Call

The existing query expansion model should be reused as the query model, but with a different system prompt and output schema.

The current expansion task is:

```text
Generate 2 to 4 short search reformulations.
```

The recall distillation task is:

```text
Given a user prompt for an Obsidian memory system, extract the retrieval intent.
Return a compact query and phrases worth searching. Ignore tool chatter, code blocks,
logs, boilerplate, and unrelated implementation detail. Prefer concrete project,
decision, concept, person, place, and artifact phrases.
```

Target JSON output:

```json
{
  "search_query": "mcp hook recall large prompts context overflow rerank sidecar",
  "phrases": [
    "MCP hook recall",
    "large prompt context overflow",
    "rerank sidecar",
    "query distillation",
    "Claude Code UserPromptSubmit"
  ],
  "identifiers": [
    "talon_hook_recall",
    "UserPromptSubmit",
    "context_tokens",
    "max_output_tokens"
  ]
}
```

The LLM call receives only a token-budgeted view of the prompt plus deterministic extraction hints. It should not receive raw unbounded input.

The distiller is used only when the prompt exceeds the relevant model/sidecar budget or when the prompt is judged too noisy for direct semantic retrieval.

## Phrase Extraction

The design cares about phrases more than isolated words.

Useful examples:

- "context overflow"
- "MCP hook recall"
- "query embedding model"
- "Claude Code UserPromptSubmit"
- "rerank sidecar"

Weak examples:

- "hook"
- "query"
- "model"

Phrase extraction should combine deterministic local heuristics with an unsupervised keyphrase algorithm.

Deterministic extraction:

- Quoted strings.
- Obsidian wikilinks.
- Tags.
- Markdown headings.
- File paths and vault paths when present.
- Identifiers with underscores, hyphens, camel case, or code-like casing.
- Proper-noun-ish multiword phrases.
- Repeated multiword phrases.

Candidate crates:

- `yake-rust`: preferred first candidate. YAKE is unsupervised, single-document, corpus-independent, and phrase-oriented.
- `keyword_extraction`: useful if it gives access to multiple algorithms behind one abstraction.
- `rake`: simpler phrase extractor, but more likely to produce awkward long phrases.

The first implementation should evaluate `yake-rust` and `keyword_extraction`, then choose the simpler reliable option.

Do not blindly deduplicate terms or phrases. Repetition is a signal. Instead, preserve weights:

```rust
struct WeightedPhrase {
    text: String,
    weight: f32,
    source: PhraseSource,
}
```

Weights can reflect frequency, position, user prompt source, exact literal source, YAKE score, and identifier/path importance.

## Query Set Construction

Recall should not do generic query expansion after distillation. The retrieval query set comes from:

- Main query: raw prompt if small, or `distilled.search_query` if large.
- Phrase queries: grouped top weighted phrases.
- Identifier/literal query: paths, tags, wikilinks, identifiers.

The main query is used for semantic retrieval and rerank. Phrase and identifier queries give lexical and hybrid retrieval additional handles.

Example query set:

```text
main:
  "mcp hook recall large prompts context overflow rerank sidecar"

phrases:
  "MCP hook recall" "large prompt context overflow" "query distillation"
  "rerank sidecar" "embedding query model" "hook deadline"

literals:
  talon_hook_recall UserPromptSubmit context_tokens max_output_tokens
```

The existing fusion/RRF machinery can combine these query result lists. Rerank should use the compact main query, never the raw oversized prompt.

## Weighting

For Claude recall, only the user prompt is ingested. These weights are still useful inside a prompt because some segments are more valuable than others.

Initial dogfood weights:

- User prompt prose: 1.0
- Explicit user literals, paths, wikilinks, and identifiers: 1.5
- User extracted phrases: 1.2
- Code block prose: 0.0
- Code identifiers/errors inside code blocks: 0.4 only if they look user-intent-relevant
- Logs, stack traces, and command output bulk: 0.0

Because Talon is an Obsidian recall tool, not a code-context tool, code-heavy bulk should be ignored by default. If the user prompt is explicitly about a code symbol and that symbol appears outside a code block, the identifier extractor will still capture it.

These weights are expected to change through dogfooding.

## Hook Deadline And Fallback

The hook must have a configurable wall-clock deadline. The current target is 20 seconds.

The deadline applies to the whole recall hook, not just HTTP calls. It includes:

- Prompt token estimation.
- Phrase extraction.
- Optional LLM distillation.
- Embedding.
- Retrieval.
- Rerank.
- Output formatting.

Fallback behavior:

- Semantic recall is attempted first when there is enough remaining deadline.
- If distillation fails, use local phrase extraction and prompt tail.
- If embedding fails or times out, continue lexical.
- If rerank fails or times out, return fused pre-rerank results.
- If semantic work consumes the deadline, return fast lexical using the best compact query and phrase queries available.
- The hook should return no context rather than block past the deadline.

Sidecar HTTP timeouts are not enough because they are longer than an acceptable hook wait and do not account for local preprocessing. Hook deadline must be enforced at the orchestration level.

## Ask Context Enforcement

Ask uses the same model-budget layer.

The ask planner and synthesizer must know:

- Planning model context window.
- Planning model output reserve.
- Synthesis model context window.
- Synthesis model output reserve.

Before sending a planning or synthesis request, Talon must estimate the full request size and trim locally if needed.

Synthesis trimming priority:

- Preserve the user's question.
- Preserve source paths and titles.
- Preserve highest-ranked snippets first.
- Trim lower-ranked snippets before higher-ranked snippets.
- Drop low-ranked sources when trimming snippets is insufficient.

No request known to exceed `ask.context_tokens` should be sent.

## Testing And Evaluation

The design requires direct dogfood tests, not only unit tests.

Golden inputs should include:

- A short direct query that should bypass LLM distillation.
- A huge conversational prompt that must be distilled.
- A pasted planning conversation.
- A prompt with many repeated important phrases.
- A prompt with code blocks that should mostly be ignored.
- A prompt with Obsidian paths, tags, and wikilinks.
- A prompt whose best recall depends on a phrase, not individual words.
- A prompt that forces semantic failure and verifies lexical fallback.
- An ask request with too many sources that must trim before synthesis.

Metrics:

- Hook total wall time.
- Whether distillation ran.
- Input tokens before and after distillation.
- Extracted phrase count.
- Query set size.
- Expansion/distillation HTTP duration.
- Embedding duration.
- Rerank duration.
- Whether fallback fired.
- Recall precision in dogfood examples.
- Ask synthesis prompt tokens versus context window.

The first quality question is whether `bonsai` can produce high-quality distillation JSON with the new prompt. If it cannot, deterministic phrase extraction still provides a safe baseline, and the query model can be revisited.

## Expected Outcome

After this design is implemented:

- Claude Code recall hooks no longer process assistant output or Stop hook payloads.
- Large prompts are never sent raw to expansion, embedding, rerank, or ask synthesis.
- Model input and output budgets are explicit in config.
- Recall uses query distillation and phrase retrieval instead of generic expansion on giant prompts.
- Semantic recall remains the default.
- Fast lexical retrieval is a deadline/error fallback, not the normal path for large input.
- Ask avoids known context-overflow requests.
- Hook latency is bounded by `mcp.hooks.recall_deadline_ms`.
