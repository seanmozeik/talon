# LLM Wiki Inspiration Notes

**Date:** 2026-04-29
**Status:** research note, not an implementation plan
**Subject:** What Talon can learn from LLM Wiki without turning Talon into a desktop app

## 0. Source Links

- Upstream project: <https://github.com/nashsu/llm_wiki>
- Local source snapshot reviewed: `/home/yolo/.opensrc/repos/github.com/nashsu/llm_wiki/main`
- Karpathy LLM Wiki pattern referenced by both projects: <https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f>
- LLM Wiki license in the reviewed snapshot: GPL-3.0, `LICENSE`

Important license note: this document is for product and architecture inspiration. Do not copy LLM Wiki source code into Talon. If any scoring math or algorithmic constants are later ported rather than independently designed, follow the repository rule: cite the exact source file and line in an inline code comment and update `LICENSE-3RD-PARTY.md`. Because the LLM Wiki snapshot is GPL-3.0, exact code reuse would need explicit license review.

## 1. Why This Document Exists

LLM Wiki is inspiring because it is another concrete implementation of the same broad Karpathy pattern that motivated Talon: build a persistent LLM-maintained wiki instead of doing one-off retrieval every time a user asks a question.

The projects are meaningfully different:

- LLM Wiki is a Tauri/React desktop application with GUI panels, project state, multi-conversation chat, ingest queues, graph visualization, review surfaces, and web clipping.
- Talon is a standalone Rust CLI and MCP-over-stdio tool for Obsidian vault search, read, sync, related-note traversal, recall, lint, status, and agent-oriented output.
- LLM Wiki tries to build and maintain a wiki directly. Talon is more conservative: index, retrieve, read, lint, recall, and integrate with agent hosts.
- LLM Wiki has a GUI where review cards, graph insights, panels, and progress bars make sense. Talon needs equivalent value through terminal commands, JSON output, MCP tools, and `--agent` contracts.

The goal is not to chase feature parity. The goal is to preserve the useful ideas so they can be evaluated later when Talon has bandwidth.

## 2. High-Level Takeaway

The most valuable inspiration from LLM Wiki is not its search stack. Talon's retrieval layer is already more mature in several ways:

- hybrid BM25, fuzzy title, exact alias, and semantic retrieval,
- weighted reciprocal rank fusion,
- query expansion,
- cross-encoder reranking,
- chunk anchors,
- scope-aware ranking,
- recall suppression across turns,
- regression and fixture tests.

The stronger inspiration is LLM Wiki's **wiki maintenance loop**:

- purpose-aware generation and research,
- two-stage analysis before writing,
- persistent ingest/review queues,
- graph-derived insights,
- constrained LLM edits,
- source traceability,
- saved query/research pages that feed back into the wiki.

For Talon, the right translation is:

- deterministic graph and lint commands first,
- optional LLM-assisted suggestions second,
- explicit writes only when requested,
- agent-readable JSON everywhere,
- no GUI assumptions in the core.

## 3. What LLM Wiki Built

LLM Wiki's README describes a full desktop implementation of the Karpathy pattern:

- raw sources, generated wiki pages, and schema/purpose files,
- two-step ingest,
- knowledge graph visualization,
- Louvain community detection,
- graph insights,
- vector search,
- persistent ingest queue,
- deep research,
- async review system,
- Chrome web clipper,
- multimodal image ingestion,
- multi-format document extraction,
- multi-conversation chat,
- saved answers that can be written back into the wiki.

Relevant implementation files in the local snapshot:

- `src/lib/ingest.ts` - two-step analysis/generation ingest, review block parsing, source-summary fallback, cache handling.
- `src/lib/ingest-queue.ts` - persistent serial ingest queue with retry/cancel behavior.
- `src/lib/graph-relevance.ts` - retrieval graph and four-signal relevance scoring.
- `src/lib/wiki-graph.ts` - graph building, Louvain communities, cohesion scoring.
- `src/lib/graph-insights.ts` - surprising connections and knowledge gap detection.
- `src/lib/enrich-wikilinks.ts` - constrained LLM suggestions for wikilink insertion.
- `src/lib/deep-research.ts` - web search, synthesis, save-to-wiki, and auto-ingest loop.
- `src/lib/optimize-research-topic.ts` - purpose/overview-aware research topic generation.
- `src/lib/search.ts` and `src/lib/embedding.ts` - token/vector search path.
- `src/lib/context-budget.ts` - proportional context allocation.
- `src/lib/sweep-reviews.ts` - stale review auto-resolution by rules and LLM judgment.
- `src-tauri/src/commands/fs.rs` and `src-tauri/src/commands/extract_images.rs` - native extraction and file support.

## 4. What Talon Already Has

Talon already has several foundations that map cleanly to the better LLM Wiki ideas:

- `talon search` with hybrid/fulltext/semantic/title modes.
- `talon read` with Obsidian reference support and heading reads.
- `talon related` for link/backlink traversal.
- `talon lint` for structural vault checks.
- `talon recall` for agent memory injection.
- MCP tool dispatch for host integration.
- SQLite indexes for notes, links, frontmatter, chunks, and vector metadata.
- Scope filters and scope priority.
- Optional inference endpoints for embedding, expansion, rerank, and ask.
- Chunking with title/path/heading-enriched embedding text.
- Recent recall suppression via session ledger state.
- Search/ranking eval scaffolding and regression tests.

Relevant Talon files:

- `crates/talon-core/src/query/search.rs`
- `crates/talon-core/src/query/related.rs`
- `crates/talon-core/src/query/lint.rs`
- `crates/talon-core/src/search/hybrid_pipeline.rs`
- `crates/talon-core/src/search/rrf.rs`
- `crates/talon-core/src/search/fuse.rs`
- `crates/talon-core/src/text/chunker.rs`
- `crates/talon-core/src/links.rs`
- `crates/talon-cli/src/mcp/tool`
- `docs/2026-04-29-talon-recall-efficiency-design.md`

This means Talon can take selected concepts without adopting LLM Wiki's implementation style.

## 5. Feature Ideas Worth Preserving

### 5.1 Purpose-Aware Retrieval Profiles

LLM Wiki adds `purpose.md`, separate from schema. The idea is good: the wiki needs a statement of intent, not only structural rules.

Talon equivalent:

- Add optional config for profile/context files, for example `purpose.md`, `wiki/overview.md`, `README.md`, or user-chosen files.
- Use those files as bounded context for `ask`, `recall`, and possibly LLM query expansion.
- Do not silently stuff large files into every prompt. Use strict token caps and diagnostics.
- Keep `--intent` as an override or additive hint.

Possible config shape:

```toml
[context_profile]
files = ["purpose.md", "wiki/overview.md"]
max_tokens = 700
use_for = ["ask", "recall", "expansion"]
```

CLI/MCP surfaces:

- `talon status` reports whether configured profile files exist.
- `talon recall --explain` or diagnostics reports when profile context affected recall.
- MCP status can expose profile freshness and token budget.

Why this fits Talon:

- It strengthens existing `intent` support.
- It preserves CLI explicitness.
- It avoids making Talon a wiki author by default.

Open questions:

- Should profile files be indexed like notes, or read directly as special context?
- Should scope filters apply to profile files?
- Should this be called "purpose", "profile", or "context profile" to avoid overfitting to LLM Wiki?

### 5.2 Ranked Related Notes

LLM Wiki's relatedness model combines four signals:

- direct links,
- shared raw sources from frontmatter,
- common neighbors using Adamic-Adar,
- page type affinity.

Talon's current `related` command traverses the link graph and returns direct related notes with edge counts. That is useful and deterministic, but it does not rank second-order "this is probably related" candidates as richly as it could.

Possible Talon feature:

```text
talon related NOTE --ranked
talon related NOTE --ranked --depth 2 --explain
talon related NOTE --ranked --json
```

Output could include:

- final score,
- direct link count,
- backlink count,
- shared source count,
- common neighbor score,
- scope,
- mtime,
- explanation strings in JSON.

Algorithm sketch:

1. Resolve source note.
2. Load active notes and resolved links from SQLite.
3. Build in/out neighbor sets.
4. Load `sources` frontmatter values from `note_frontmatter_fields`.
5. Compute candidates from:
   - direct outlinks and backlinks,
   - two-hop neighbors,
   - source-overlap neighbors.
6. Score candidates with independently chosen Talon constants.
7. Sort by score, then deterministic path.

Why this fits Talon:

- It builds on existing SQLite link/frontmatter indexes.
- It does not require LLM calls.
- It can improve recall expansion and `search --related`.
- It is agent-friendly if explanations are structured.

Risks:

- Source overlap can over-rank broad source summary pages.
- Index/log/overview pages should probably be downweighted or excluded.
- Type affinity requires trustworthy page type metadata. Many Obsidian vaults will not have it.

Recommendation:

- Start with direct links, backlinks, source overlap, and Adamic-Adar.
- Treat type affinity as optional and config-driven.
- Add ranking tests on fixture vaults before using this in recall.

### 5.3 Graph Health Commands

LLM Wiki has graph insights for:

- isolated pages,
- sparse communities,
- bridge nodes,
- surprising cross-community connections.

Talon already has lint checks for orphans, broken links, dangling refs, and unreferenced notes. It does not yet have graph-health analysis.

Possible command set:

```text
talon graph stats
talon graph gaps
talon graph bridges
talon graph communities
talon graph related NOTE
```

Possible `talon graph gaps` output:

- isolated notes,
- low-link notes inside dense scopes,
- sparse clusters,
- notes that mention known titles but do not link them,
- notes with high search relevance but no graph connection.

Possible `talon graph bridges` output:

- notes connecting many communities,
- notes with high betweenness-like behavior,
- notes that are important enough to maintain carefully,
- source summary pages that are over-central and should not dominate navigation.

Why this fits Talon:

- It extends `lint` without requiring writes.
- It gives users a maintenance workflow.
- It produces useful context for agents.
- It can feed future `suggest-links`.

Open question:

- Should graph health live under `talon graph`, `talon lint --graph`, or both?

Pragmatic answer:

- Add a core graph analysis module and expose both:
  - `talon graph gaps` for humans,
  - `talon lint --check graph` for CI/agents.

### 5.4 Community Detection

LLM Wiki uses Louvain community detection through `graphology-communities-louvain`. It then computes cohesion as actual intra-community edges divided by possible intra-community edges.

Talon equivalent:

- Build an undirected weighted graph from resolved links.
- Optionally include shared-source edges at lower weight.
- Run a Rust community detection crate or a simple first version based on connected components / label propagation.
- Compute:
  - node count,
  - intra-edge density,
  - top nodes by degree,
  - bridge notes across communities,
  - sparse communities below a threshold.

Potential value:

- Better `related` expansion.
- Better `recall` diversity.
- Better lint/maintenance commands.
- Better `ask` context packaging: include one representative per community instead of five notes from the same cluster.

Why not rush:

- Community detection can be unstable across small graph changes.
- Output needs deterministic tests.
- A CLI user will trust this only if the explanation is clear.

Recommended first step:

- Implement deterministic graph metrics first: degree, connected component, source overlap, common neighbors, bridge count.
- Add Louvain or label propagation only after those are useful.

### 5.5 Constrained Wikilink Suggestions

This is one of the strongest ideas from LLM Wiki. Instead of asking the LLM to rewrite a page, it asks for JSON suggestions of exact terms to link to existing pages. The program then applies only exact bracket insertions.

Talon equivalent:

```text
talon suggest-links NOTE --dry-run
talon suggest-links NOTE --format patch
talon suggest-links NOTE --apply
talon suggest-links --scope wiki --limit 50 --json
```

Core rules:

- Never alter frontmatter.
- Never alter fenced code blocks.
- Never link terms already inside `[[...]]`.
- Require target pages to exist.
- Require suggested terms to be literal substrings in the page.
- Limit one link per target per page.
- Default to `--dry-run`.
- Emit unified diff, JSON, or both.

Two implementation modes:

- deterministic mode: title/alias matching only,
- LLM mode: JSON suggestions constrained to existing index entries.

Why this fits Talon:

- It improves graph quality.
- It keeps writes explicit.
- It is safe for agents.
- It can be benchmarked by before/after broken-link and graph-connectivity metrics.

Potential command output:

```json
{
  "path": "wiki/concepts/example.md",
  "suggestions": [
    {
      "term": "retrieval augmented generation",
      "target": "wiki/concepts/rag.md",
      "line": 18,
      "confidence": "high",
      "source": "title-alias"
    }
  ]
}
```

### 5.6 Review Queue

LLM Wiki's ingest can produce review items:

- contradiction,
- duplicate,
- missing page,
- suggestion.

It stores those items and later sweeps stale reviews using rules and LLM judgment.

Talon equivalent:

```text
talon review list
talon review add --type missing-page --path NOTE --message ...
talon review resolve ID
talon review sweep
talon lint --semantic
```

MCP equivalent:

- `talon_review_list`
- `talon_review_resolve`
- `talon_review_sweep`

Storage options:

- SQLite table in Talon's DB,
- `.talon/review.jsonl` inside the vault container,
- ordinary markdown file in the vault.

Recommendation:

- Prefer SQLite for machine state.
- Offer export to markdown later.

Why this matters:

- Talon currently reports structural problems. A review queue would let it track semantic maintenance work that should not block normal use.
- Agents could leave review items instead of editing the vault prematurely.

Important boundary:

- Review items should not become hidden TODOs that only Talon sees. Human-readable output and export matter.

### 5.7 Deep Research as an External Workflow

LLM Wiki's Deep Research flow:

1. Generate or accept a research topic.
2. Run multiple web searches.
3. Synthesize findings into a wiki page.
4. Save the page under `wiki/queries`.
5. Auto-ingest that page to extract entities/concepts.

Talon should not bake web search into the core too quickly. A CLI can support the workflow without owning all of it.

Possible Talon shapes:

```text
talon research topic-from-gap GAP_ID
talon research import FILE.md
talon distill FILE.md --dry-run
talon ask "research this gap" --save-query
```

Better first step:

- Add a "research prompt" or "gap prompt" output mode that an agent can use with its own web tools.
- Let the agent bring back a markdown artifact.
- Talon indexes, reads, and suggests links for that artifact.

Why:

- Web search APIs and browsing freshness are not core Talon strengths.
- Agent hosts already have web tools.
- Talon should remain a durable local vault memory/index tool.

### 5.8 Saved Query Pages

LLM Wiki can save valuable answers into `wiki/queries/`, then auto-ingest them so the knowledge graph absorbs the result.

Talon already has `ask`. A useful later addition:

```text
talon ask "..." --save wiki/queries/name.md
talon ask "..." --save-query
talon ask "..." --save-query --suggest-links
```

Requirements:

- Cite source notes used by the answer.
- Store frontmatter with query, timestamp, source paths, and model metadata.
- Do not auto-create entity/concept pages by default.
- Optionally run `suggest-links` after saving.

This fits the CLI well because the user explicitly asks to save.

### 5.9 Source Traceability and Deletion Hygiene

LLM Wiki relies heavily on frontmatter `sources: []` to connect generated pages back to raw sources. It also has cascade deletion behavior that removes or edits generated pages when a source is deleted.

Talon already parses frontmatter and has reverse indexes. Future uses:

- `talon sources NOTE` - show source lineage.
- `talon impacted SOURCE` - show pages that cite a source.
- `talon delete-plan SOURCE` - dry-run cascade impact.
- `talon lint --check stale-sources` - source file referenced but missing.
- `talon related --by-source SOURCE` - source-overlap navigation.

This is especially relevant if Talon ever grows `distill` or `save-query`.

### 5.10 Multimodal and Document Extraction

LLM Wiki supports PDFs, Office files, spreadsheets, images, and image captioning. Talon currently focuses on Obsidian vault markdown and embeddings.

Potential Talon stance:

- Do not make Talon a universal document extraction suite by default.
- Allow external pipelines to write markdown into the vault.
- Consider a plugin or companion command later for source extraction.
- If image captions are added, treat them as explicit indexed artifacts with provenance.

Useful idea to keep:

- Image captions should be cached by content hash.
- Captions should be factual, not interpretive.
- Captions should be tied to source path and page/slide metadata.

But this is not near-term core Talon work.

## 6. Algorithmic Ideas

### 6.1 Four-Signal Graph Relevance

LLM Wiki's relevance model is simple enough to reason about:

- direct link strength,
- source overlap,
- common neighbors,
- type affinity.

Talon can independently design a Rust version around the data it already indexes.

Candidate Talon score components:

```text
score =
  direct_link_weight * direct_edge_count
  + backlink_weight * backlink_edge_count
  + source_overlap_weight * shared_source_count
  + common_neighbor_weight * adamic_adar(source, candidate)
  + scope_multiplier
  - structural_page_penalty
```

Avoid copying exact LLM Wiki constants. Design Talon constants and tune with fixtures.

Potential diagnostics per result:

```json
{
  "score": 5.42,
  "signals": {
    "directLinks": 1,
    "backlinks": 2,
    "sharedSources": 1,
    "commonNeighbor": 0.91,
    "scopeMultiplier": 1.1,
    "structuralPenalty": 0.0
  }
}
```

### 6.2 Adamic-Adar for Common Neighbors

Adamic-Adar gives more value to common neighbors that are themselves specific, not giant hubs. That maps well to Obsidian vaults where index pages and broad overview pages otherwise connect too much.

Formula concept:

```text
sum over common neighbors n of 1 / log(degree(n))
```

Talon should:

- exclude or heavily downweight structural pages,
- cap contribution from extremely high-degree notes,
- keep deterministic sorting,
- test against fixture graphs with hubs and topic clusters.

### 6.3 Source-Overlap Ranking

If two generated notes cite the same source, they probably belong near each other. Talon already stores frontmatter fields and can exploit this.

Risks:

- If many notes cite a broad book or project, source overlap becomes too broad.
- Generated source summary pages may become artificial hubs.
- User-authored notes may not have `sources`.

Mitigations:

- Use inverse document frequency style weighting for sources.
- Cap per-source contribution.
- Treat exact shared source as one signal, never the whole score.

Possible source IDF:

```text
source_weight(s) = 1 / log(2 + number_of_notes_citing_s)
```

### 6.4 Communities for Diversity, Not Just Display

In a GUI, communities are useful for graph coloring. In Talon, communities are more useful for selection and diversity:

- search result diversification,
- recall context diversification,
- gap detection,
- bridge node detection,
- summarizing a vault's structure.

Possible recall application:

- retrieve top 30 candidates,
- assign or approximate communities,
- select top candidates with a per-community cap,
- allow bridge notes to bypass the cap.

This could reduce the "five near-duplicate notes from the same local cluster" failure mode.

### 6.5 Query Expansion and Graph Expansion

LLM Wiki describes graph expansion after token/vector search. Talon already has query expansion and hybrid retrieval. The missing part is graph-aware expansion after search.

Possible Talon pipeline variant:

1. hybrid retrieval,
2. ranked graph expansion from top seeds,
3. fuse original retrieval score with graph score,
4. optional rerank or context selection.

This should not replace current hybrid ranking until measured. It is more appropriate for:

- `recall`,
- `ask` context assembly,
- `related --ranked`,
- optional `search --include-related`.

### 6.6 Context Budgeting

LLM Wiki has proportional context budgets for wiki pages, chat history, index, and system prompt. Talon recall already has a token budget, but `ask` and future saved-query workflows could benefit from explicit allocation.

Possible Talon budget buckets:

- answer/system reserve,
- user query and prior messages,
- profile files,
- index/overview,
- retrieved notes,
- related/graph expansion,
- citations/source metadata.

Useful principle:

- Budgets should be visible in diagnostics. Hidden context allocation makes retrieval hard to debug.

### 6.7 CJK Tokenization

LLM Wiki's token search includes CJK bigrams and individual character fallback. Talon uses SQLite FTS/BM25 and fuzzy matching; CJK behavior should be audited separately.

Possible future task:

- Add a CJK search fixture.
- Measure current BM25/title/vector behavior.
- If lexical recall is weak, consider CJK bigram shadow fields during indexing.

This is worth preserving as a research question, not an immediate conclusion.

### 6.8 Review Sweeping

LLM Wiki's stale review cleanup is a nice pattern:

- resolve obvious missing-page reviews by exact page/title match,
- resolve duplicate reviews when affected pages changed,
- ask LLM only for remaining ambiguous cases,
- be conservative.

Talon could apply this if it gains a review queue. The key lesson is to use deterministic rules before LLM calls.

## 7. CLI and MCP Translation

Desktop concepts need translation, not direct copying.

| LLM Wiki GUI Concept | Talon Translation |
|---|---|
| Graph view | `talon graph stats/gaps/bridges/communities --json` |
| Insight cards | lint findings or graph findings |
| Review panel | `talon review list`, MCP review tools |
| Activity panel | structured progress lines, JSON events, `status` |
| Deep Research button | prompt/tool handoff to agent, saved markdown import |
| Save to Wiki | `talon ask --save-query` |
| Purpose settings | config profile files |
| Web clipper | external import pipeline writes markdown, Talon syncs |
| Lightbox/image search | later plugin or artifact metadata, not core |

Agent-first surfaces matter. Every feature should have:

- stable JSON output,
- compact `--agent` output when useful,
- deterministic IDs,
- source paths,
- optional explanations,
- no implicit writes unless the command name makes writes obvious.

## 8. Prioritized Future Backlog

### P1: Ranked Related Notes

Why first:

- Builds on existing data.
- No LLM needed.
- Improves `related`, recall, and agent tools.
- Easy to test with fixture graphs.

Possible deliverable:

- `talon related --ranked --explain`
- new core graph scoring module,
- fixture tests for direct link, source overlap, common neighbors, and hub downweighting.

### P2: Graph Gaps and Bridges

Why second:

- Natural extension of lint.
- Useful for vault maintenance.
- No LLM required.

Possible deliverable:

- `talon graph gaps --json`
- `talon graph bridges --json`
- `talon lint --check graph`

### P3: Suggest Links Dry Run

Why third:

- High product value.
- Improves graph quality.
- Default dry-run keeps trust.

Possible deliverable:

- deterministic title/alias link suggestions,
- JSON and patch output,
- no LLM in first version.

Later:

- optional LLM mode constrained to JSON suggestions.

### P4: Purpose/Context Profile Files

Why fourth:

- Useful for `ask` and `recall`.
- Needs careful token budgeting and diagnostics.

Possible deliverable:

- config section,
- status validation,
- budgeted context injection into `ask` planning and/or recall expansion.

### P5: Review Queue

Why fifth:

- Productively captures ambiguous maintenance work.
- More stateful than current Talon behavior.
- Needs storage decisions.

Possible deliverable:

- SQLite review table,
- `review list/resolve/sweep`,
- graph/lint commands can emit review items only when explicitly asked.

### P6: Saved Query Pages

Why sixth:

- Builds on `ask`.
- Lets useful answers become vault artifacts.
- Should wait until citation and provenance shape is settled.

Possible deliverable:

- `talon ask --save-query`,
- frontmatter with query, date, model, sources,
- optional post-save `suggest-links`.

### P7: Research Handoff

Why later:

- Web research is not core Talon.
- Better handled by agent hosts initially.

Possible deliverable:

- `talon graph gaps --research-prompts`
- structured prompts/queries for agents to run externally.

## 9. Things Not To Copy

Do not copy these directly:

- GUI layout and panels.
- LanceDB choice. Talon's SQLite/vector approach is more appropriate for a standalone binary.
- Their token/vector search implementation. Talon's search stack is stronger and already tested.
- Auto-ingest as default. Talon should not silently write generated wiki pages.
- Broad LLM rewrites. Prefer constrained suggestions and explicit patches.
- Web search API coupling in core.
- Exact GPL-licensed source code.

## 10. Design Principles For Any Future Work

1. Deterministic before LLM.
2. Dry-run before write.
3. JSON before prose for agents.
4. Explain scores when scores affect user-visible ranking.
5. Keep vault edits auditable as patches or explicit saved files.
6. Prefer indexing existing user-authored vaults over inventing a generated wiki taxonomy.
7. Treat source provenance as a first-class signal.
8. Do not let structural pages dominate graph algorithms.
9. Test ranking changes with fixture graphs and regression metrics.
10. Keep GUI-specific concepts out of core naming.

## 11. Possible Data Model Additions

These are not recommendations to implement now. They are placeholders for later design.

### Graph Scores

Could be computed on demand at first. Cache only if needed.

```text
graph_node:
  note_id
  vault_path
  degree_in
  degree_out
  degree_total
  component_id
  community_id optional
```

### Review Items

```text
review_item:
  id
  type
  title
  message
  source_path optional
  affected_paths json
  suggested_queries json
  status pending/resolved/dismissed
  created_at
  resolved_at optional
  resolution optional
```

### Saved Query Metadata

```text
saved_query:
  path
  query
  created_at
  model
  source_paths
  source_chunk_ids
  answer_hash
```

## 12. Evaluation Ideas

For graph relevance:

- fixture vault with clear clusters,
- fixture with a hub page that should not dominate,
- fixture with shared sources but no direct links,
- fixture with direct links and source overlap competing,
- expected top-k related notes.

For `suggest-links`:

- do not touch frontmatter,
- do not touch code fences,
- do not double-link,
- exact target exists,
- alias display syntax is correct,
- diff output is stable.

For purpose-aware retrieval:

- query ambiguous without profile,
- query disambiguated by profile,
- diagnostics show profile use,
- profile budget never exceeds configured cap.

For review queue:

- missing-page review auto-resolves after page appears,
- contradiction review does not auto-resolve by default,
- LLM sweep failure leaves state unchanged.

For saved query pages:

- source citations are preserved,
- rerunning sync indexes the saved page,
- optional suggest-links only changes body links.

## 13. Rough Command Sketch

This is intentionally speculative.

```text
talon graph stats
talon graph gaps --limit 20 --json
talon graph bridges --json
talon graph communities --json

talon related "wiki/foo.md" --ranked --explain --limit 10

talon suggest-links "wiki/foo.md" --dry-run
talon suggest-links "wiki/foo.md" --format patch
talon suggest-links "wiki/foo.md" --apply

talon review list --json
talon review resolve REVIEW_ID --reason "merged duplicate"
talon review sweep

talon ask "what should I remember about X?" --save-query
talon ask "what should I remember about X?" --save-query --suggest-links
```

MCP tools could mirror only the stable subset:

```text
talon_graph_gaps
talon_graph_bridges
talon_suggest_links
talon_review_list
talon_review_resolve
```

## 14. Relationship To Current Talon Plans

This note complements, but should not disrupt, the current recall efficiency work.

Potential overlaps:

- Ranked graph relevance can improve recall candidate expansion.
- Community-aware diversity can improve automatic recall context selection.
- Purpose/profile files can improve recall query construction.
- Review items can give agents a safer alternative to writing notes.
- Saved query pages can become explicit, auditable memory artifacts.

Do not merge these ideas into recall all at once. Recall should stay latency-conscious. Graph features should be measurable independently before becoming automatic recall behavior.

## 15. Suggested Next Reading If This Is Reopened

Start with these files:

LLM Wiki:

- `README.md`
- `src/lib/graph-relevance.ts`
- `src/lib/graph-insights.ts`
- `src/lib/wiki-graph.ts`
- `src/lib/enrich-wikilinks.ts`
- `src/lib/ingest.ts`
- `src/lib/deep-research.ts`

Talon:

- `crates/talon-core/src/query/related.rs`
- `crates/talon-core/src/query/lint.rs`
- `crates/talon-core/src/query/search.rs`
- `crates/talon-core/src/search/hybrid_pipeline.rs`
- `crates/talon-core/src/search/rrf.rs`
- `crates/talon-core/src/text/frontmatter.rs`
- `crates/talon-core/src/links.rs`
- `docs/2026-04-29-talon-recall-efficiency-design.md`

Questions to answer before implementation:

1. Which idea has the smallest useful non-LLM version?
2. Can it be expressed as JSON and tested deterministically?
3. Does it require writes? If yes, can it be a dry-run patch first?
4. Does it change ranking? If yes, what fixture or eval proves the change helps?
5. Does it interact with scopes?
6. Does it need new persistent state?
7. Could an agent host do this better outside Talon?

## 16. Final Recommendation

If this document turns into work later, the best first project is:

```text
talon related --ranked --explain
```

That project is the cleanest bridge between LLM Wiki's best algorithmic idea and Talon's existing architecture. It is deterministic, testable, useful in the CLI, useful to MCP agents, and likely to improve recall without introducing automatic writes.

After that, build graph gaps/bridges. Only then consider LLM-assisted suggestions, review queues, saved query pages, or research workflows.
